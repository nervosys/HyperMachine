//! Machine models: the device set a guest expects to find, in one call.
//!
//! A [`VM`](crate::VM) starts with no devices at all, which is the right
//! default for something that also runs a 512-byte real-mode image. A Linux
//! kernel is not that: it expects the legacy PC platform to be there, and when
//! a piece of it is missing it does not complain, it hangs. Every device in
//! [`Machine::legacy_pc`] was found the same way -- a guest went quiet, an exit
//! histogram showed it spinning on one port, and the device behind that port
//! was absent or mis-mapped. Nothing about that list is discoverable from the
//! outside, so it lives here instead of in each caller.
//!
//! The failure mode they share is worth stating once: an I/O port with no
//! device behind it reads `0xff`. Every bit set. For a status register whose
//! "busy" or "in progress" bit the guest polls until it clears, that is a
//! permanent yes, and the guest waits forever.
//!
//! ```no_run
//! # use hv2_core::{machine::Machine, VMConfig, VM};
//! # async fn example() -> hv2_core::Result<()> {
//! let vm = VM::new(VMConfig::default())?;
//! Machine::legacy_pc().attach(&vm.devices()).await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::device::{Device, DeviceManager};
use crate::devices::{
    KeyboardDevice, PciConfigIo, RtcDevice, SerialDevice, PCI_CONFIG_IO_BASE, PCI_CONFIG_IO_LAST,
};
use crate::{Error, Result};

/// COM1's base port. `console=ttyS0` means this one and no other.
pub const COM1_BASE: u16 = 0x3F8;
/// COM1's last port. A 16550 is eight byte-wide registers.
pub const COM1_LAST: u16 = COM1_BASE + 7;
/// The CMOS index/data pair.
pub const RTC_BASE: u16 = 0x70;
/// The last CMOS port.
pub const RTC_LAST: u16 = 0x71;
/// The i8042 data port.
pub const I8042_BASE: u16 = 0x60;
/// The i8042 status/command port. The range in between is not the controller's,
/// but nothing else claims it and mapping it whole keeps a stray access from
/// reading `0xff`.
pub const I8042_LAST: u16 = 0x64;

/// One device in a machine model: what it is, where the guest finds it, and
/// what breaks without it.
///
/// Built by [`Machine::legacy_pc`] for the legacy set, and by
/// [`MachineDevice::new`] for anything a caller wants to add alongside it.
pub struct MachineDevice {
    name: String,
    first_port: u16,
    last_port: u16,
    irq: Option<u8>,
    why: &'static str,
    device: Arc<RwLock<dyn Device>>,
}

impl MachineDevice {
    /// Describe a device and the I/O port range it answers, inclusive at both
    /// ends -- the same convention as
    /// [`DeviceManager::register_io_port_range`].
    ///
    /// `why` is what breaks in a guest when this device is not there. It is
    /// required rather than optional because a machine model whose entries
    /// cannot each justify themselves is a list nobody can safely edit.
    pub fn new(
        name: impl Into<String>,
        device: Arc<RwLock<dyn Device>>,
        first_port: u16,
        last_port: u16,
        why: &'static str,
    ) -> Self {
        Self {
            name: name.into(),
            first_port,
            last_port,
            irq: None,
            why,
            device,
        }
    }

    /// Record the interrupt line this device raises.
    ///
    /// Documentation, not wiring: each device already knows its own line (a
    /// [`SerialDevice`] derives it from its base port, a [`KeyboardDevice`]
    /// hardcodes IRQ 1) and raises it through the sink the manager installs.
    /// This is here so a caller building an IOAPIC or an MADT can read the
    /// machine's interrupt map off the model instead of guessing it.
    #[must_use]
    pub fn with_irq(mut self, irq: u8) -> Self {
        self.irq = Some(irq);
        self
    }

    /// The name this device registers under.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The inclusive I/O port range this device answers.
    #[must_use]
    pub fn ports(&self) -> (u16, u16) {
        (self.first_port, self.last_port)
    }

    /// The interrupt line this device raises, when it raises one.
    #[must_use]
    pub fn irq(&self) -> Option<u8> {
        self.irq
    }

    /// What a guest does when this device is missing.
    #[must_use]
    pub fn why(&self) -> &'static str {
        self.why
    }
}

/// A machine model: the set of devices to attach to a VM before it boots.
///
/// Compose with [`Machine::with_device`]; attach with [`Machine::attach`].
/// Attaching does not consume the model, so the same description can be
/// inspected afterwards -- or attached to a second VM.
#[must_use]
pub struct Machine {
    devices: Vec<MachineDevice>,
}

impl Machine {
    /// An empty machine. Devices only, no platform assumptions.
    pub fn empty() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// The legacy PC device set: what a Linux kernel needs in order to get as
    /// far as mounting a root filesystem, plus the port pair it uses to find
    /// anything on a PCI bus.
    ///
    /// The first three earned their place by a guest hang each one fixed. The
    /// fourth is here because its absence is silent rather than fatal: a guest
    /// probing for PCI reads `0xff` from the unhandled port, concludes the
    /// machine has no PCI at all, and carries on booting without ever saying
    /// so.
    pub fn legacy_pc() -> Self {
        Self::legacy_pc_with_pci_root(Arc::new(parking_lot::RwLock::new(
            crate::pci::PciRootComplex::new(),
        )))
    }

    /// The legacy set, with the PCI window onto a root complex the caller
    /// keeps a handle to.
    ///
    /// A VM needs this: attaching a PCI device means adding its configuration
    /// space to the same root complex the guest reads through `0xCF8`, and
    /// there is no way to reach the one `legacy_pc` builds for itself.
    pub fn legacy_pc_with_pci_root(
        pci_root: Arc<parking_lot::RwLock<crate::pci::PciRootComplex>>,
    ) -> Self {
        Self {
            devices: vec![
                // Without this there is no console at all: `console=ttyS0`
                // resolves to nothing, and a kernel that boots perfectly is
                // indistinguishable from one that faulted on instruction one.
                MachineDevice::new(
                    "COM1",
                    Arc::new(RwLock::new(SerialDevice::new(
                        "COM1".to_string(),
                        u64::from(COM1_BASE),
                    ))),
                    COM1_BASE,
                    COM1_LAST,
                    "no serial means no console: the guest's output goes nowhere",
                )
                .with_irq(4),
                // Without this the kernel asks the CMOS for the time, waits for
                // the update-in-progress bit in register A to clear, reads
                // 0xff from the absent port -- bit set, forever -- and spins
                // there having printed most of a line.
                MachineDevice::new(
                    "RTC",
                    Arc::new(RwLock::new(RtcDevice::new())),
                    RTC_BASE,
                    RTC_LAST,
                    "no RTC means the kernel spins on the CMOS update-in-progress bit, which an \
                     absent port reads as permanently set",
                )
                .with_irq(8),
                // Without this the i8042 probe polls the status port for the
                // input and output buffer bits to settle, and 0xff says "busy"
                // just as permanently.
                MachineDevice::new(
                    "i8042",
                    Arc::new(RwLock::new(KeyboardDevice::new())),
                    I8042_BASE,
                    I8042_LAST,
                    "no i8042 means the keyboard-controller probe polls the status port forever, \
                     for the same reason",
                )
                .with_irq(1),
                // No hang without this, which is why it went unnoticed. A
                // guest that finds nothing at 0xCF8 simply decides the machine
                // has no PCI bus, and every device behind one becomes
                // invisible rather than broken. The `pci` module has modelled
                // buses, config space and capabilities all along; nothing had
                // ever connected that model to a port a guest reads.
                MachineDevice::new(
                    "PCI",
                    Arc::new(RwLock::new(PciConfigIo::with_root("PCI", pci_root))),
                    PCI_CONFIG_IO_BASE,
                    PCI_CONFIG_IO_LAST,
                    "without the configuration mechanism a guest cannot enumerate PCI at all, and                      reports no bus rather than an empty one",
                ),
            ],
        }
    }

    /// Add a device to this model.
    ///
    /// This is how a caller keeps the legacy set and its own hardware in one
    /// place -- a vsock transport, say -- rather than attaching the set and
    /// then reaching for the manager directly:
    ///
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use tokio::sync::RwLock;
    /// # use hv2_core::machine::{Machine, MachineDevice};
    /// # use hv2_core::device::Device;
    /// # async fn example(mine: Arc<RwLock<dyn Device>>, vm: &hv2_core::VM) -> hv2_core::Result<()> {
    /// Machine::legacy_pc()
    ///     .with_device(MachineDevice::new("mine", mine, 0x500, 0x50F, "nothing standard"))
    ///     .attach(&vm.devices())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_device(mut self, device: MachineDevice) -> Self {
        self.devices.push(device);
        self
    }

    /// The devices this model describes, in attachment order.
    #[must_use]
    pub fn devices(&self) -> &[MachineDevice] {
        &self.devices
    }

    /// Register every device in this model with `manager`, and map its ports.
    ///
    /// Attach before [`VM::provision`](crate::VM::provision): the kernel's
    /// first console write happens early, and a device registered after the
    /// guest is running has already missed it.
    ///
    /// # Errors
    ///
    /// [`Error::Device`] if a name is already taken or a port range overlaps
    /// one already mapped. Both are reported with the device's name attached,
    /// because the manager's own message names only the range -- and a caller
    /// staring at "0x60-0x64 overlaps" wants to know which of its devices did
    /// that.
    pub async fn attach(&self, manager: &DeviceManager) -> Result<()> {
        for device in &self.devices {
            self.attach_one(manager, device).await?;
        }
        Ok(())
    }

    /// Register only the devices `manager` does not already have.
    ///
    /// This is what a VM calls for itself. [`attach`](Self::attach) is right
    /// for a caller that means to install a whole machine and wants to be told
    /// if something is in the way; this is right where the model is filling in
    /// what nobody else provided, and a caller who has already attached their
    /// own COM1 should keep it rather than be refused.
    ///
    /// A device is "already there" by name. A different device occupying the
    /// same ports under another name is still a conflict, and still reported --
    /// silently declining to map a port range is how a guest ends up talking to
    /// something other than what the caller thinks it is talking to.
    ///
    /// # Errors
    ///
    /// [`Error::Device`] as [`attach`](Self::attach), for the devices it does
    /// register.
    pub async fn attach_absent(&self, manager: &DeviceManager) -> Result<()> {
        for device in &self.devices {
            if manager.get_device(&device.name).await.is_some() {
                tracing::debug!(
                    "machine: '{}' is already attached, leaving it alone",
                    device.name
                );
                continue;
            }
            self.attach_one(manager, device).await?;
        }
        Ok(())
    }

    /// Register one device and map its ports.
    async fn attach_one(&self, manager: &DeviceManager, device: &MachineDevice) -> Result<()> {
        manager
            .register_device(device.name.clone(), Arc::clone(&device.device))
            .await
            .map_err(|e| Error::Device(format!("attaching '{}': {e}", device.name)))?;
        manager
            .register_io_port_range(device.name.clone(), device.first_port, device.last_port)
            .await
            .map_err(|e| {
                Error::Device(format!(
                    "mapping '{}' at {:#x}-{:#x}: {e}",
                    device.name, device.first_port, device.last_port
                ))
            })?;
        Ok(())
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceType;

    /// Attach the legacy set to a bare manager -- the same object
    /// `VM::devices()` hands back, without needing a hypervisor backend to
    /// exist on the machine running the test.
    async fn legacy_manager() -> DeviceManager {
        let manager = DeviceManager::new();
        Machine::legacy_pc().attach(&manager).await.unwrap();
        manager
    }

    #[tokio::test]
    async fn every_legacy_port_the_kernel_touches_resolves_to_the_device_behind_it() {
        // The lookup path, not the constants: a guest reaches a device only
        // through `find_io_device`, so a model that registers a device but
        // fails to map its ports -- or maps them to the wrong name -- is
        // exactly as broken as one that omits the device, and only this test
        // can tell.
        let manager = legacy_manager().await;

        for (port, expected) in [
            (0x3F8_u16, "COM1"), // the console
            (0x3FF, "COM1"),     // the last UART register, scratch
            (0x70, "RTC"),       // CMOS index
            (0x71, "RTC"),       // CMOS data
            (0x60, "i8042"),     // keyboard data
            (0x64, "i8042"),     // keyboard status/command
            (0xCF8, "PCI"),      // CONFIG_ADDRESS
            (0xCFC, "PCI"),      // CONFIG_DATA
            (0xCFF, "PCI"),      // CONFIG_DATA, top byte lane
        ] {
            let handle = manager
                .find_io_device(port)
                .await
                .unwrap_or_else(|| panic!("port {port:#x} resolves to nothing"));
            assert_eq!(
                handle.device_name(),
                expected,
                "port {port:#x} resolved to the wrong device"
            );
        }
    }

    /// Reaching the port is not the same as getting an answer. Before the
    /// configuration mechanism was registered, a guest's first probe fell
    /// through to the unhandled-port path and read `0xff` in every byte --
    /// which a kernel reads as "no PCI here" and accepts silently. This drives
    /// the same sequence a guest does and insists on the two answers that
    /// distinguish a working bus from an absent one.
    #[tokio::test]
    async fn a_guest_probing_pci_gets_an_empty_bus_rather_than_no_bus() {
        let manager = legacy_manager().await;
        let pci = manager
            .find_io_device(PCI_CONFIG_IO_BASE)
            .await
            .expect("PCI");

        // Select bus 0, device 0, function 0, register 0, enable bit set --
        // the first thing any PCI probe writes.
        let select = 0x8000_0000u32;
        pci.write_register(u64::from(PCI_CONFIG_IO_BASE - pci.base_port()), select, 4)
            .await
            .unwrap();

        // The latch must read back. An unhandled port returns 0xffffffff here
        // too, so this is what separates "the device answered" from "nothing
        // is listening".
        let latched = pci
            .read_register(u64::from(PCI_CONFIG_IO_BASE - pci.base_port()), 4)
            .await
            .unwrap();
        assert_eq!(
            latched, select,
            "CONFIG_ADDRESS did not read back, so nothing is answering at 0xCF8"
        );

        // With nothing plugged in, the vendor id is all ones: an empty slot on
        // a bus that exists.
        let vendor = pci
            .read_register(u64::from(0xCFC - pci.base_port()), 4)
            .await
            .unwrap();
        assert_eq!(vendor, 0xFFFF_FFFF, "an empty slot reads as all ones");
    }

    #[tokio::test]
    async fn the_legacy_console_is_a_serial_device_and_not_merely_something_named_com1() {
        // A name match would pass even if the model registered a keyboard as
        // "COM1". The kernel does not read names; it reads a UART register
        // file, so the type behind the port is the thing that matters.
        let manager = legacy_manager().await;

        let com1 = manager.find_io_device(COM1_BASE).await.expect("COM1");
        assert_eq!(com1.base_port(), COM1_BASE, "COM1 must start at 0x3F8");

        let serials = manager.get_devices_by_type(DeviceType::Serial).await;
        assert_eq!(serials.len(), 1, "the legacy set has exactly one UART");
    }

    #[tokio::test]
    async fn a_byte_written_to_the_console_port_reaches_the_hosts_console_log() {
        // End to end through the path a guest actually uses: resolve the port,
        // write the transmit register, read it back off the manager. This is
        // what catches a UART mapped at the right port but with its registers
        // offset by one -- the character would land in the interrupt-enable
        // register and the console would stay empty.
        let manager = legacy_manager().await;

        let com1 = manager.find_io_device(COM1_BASE).await.expect("COM1");
        for byte in b"hi" {
            com1.write_register(0, u32::from(*byte), 1).await.unwrap();
        }

        let console = manager.console_output().await;
        assert_eq!(
            console
                .iter()
                .find(|(name, _)| name == "COM1")
                .map(|(_, bytes)| bytes.as_slice()),
            Some(b"hi".as_slice()),
            "the console port did not carry the bytes: {console:?}"
        );
    }

    #[tokio::test]
    async fn the_cmos_update_in_progress_bit_reads_clear_instead_of_the_0xff_an_absent_port_gives()
    {
        // The whole reason the RTC is in the set. Select register A through the
        // index port and read it: bit 7 set means "update in progress", and an
        // unmapped port would hand back 0xff, which has that bit set and never
        // clears. A guest polling it there never boots.
        let manager = legacy_manager().await;

        let rtc = manager.find_io_device(RTC_BASE).await.expect("RTC");
        rtc.write_register(0, 0x0A, 1).await.unwrap(); // index register A
        let status_a = rtc.read_register(1, 1).await.unwrap();

        assert_ne!(status_a, 0xFF, "an answering RTC never reads as all ones");
        assert_eq!(
            status_a & 0x80,
            0,
            "update-in-progress must read clear, or the kernel spins on it forever"
        );
    }

    #[tokio::test]
    async fn the_i8042_status_port_reads_clear_instead_of_the_0xff_an_absent_port_gives() {
        // Same failure, different device: the keyboard-controller probe polls
        // 0x64 until the output-buffer-full and input-buffer-full bits settle.
        // 0xff sets both, forever.
        let manager = legacy_manager().await;

        let i8042 = manager.find_io_device(I8042_LAST).await.expect("i8042");
        // 0x64 is the fifth port of the range, so offset 4 from the base.
        let status = i8042
            .read_register(u64::from(I8042_LAST - I8042_BASE), 1)
            .await
            .unwrap();

        assert_ne!(status, 0xFF, "an answering i8042 never reads as all ones");
        assert_eq!(
            status & 0x02,
            0,
            "input-buffer-full must read clear, or the probe never finishes"
        );
    }

    #[tokio::test]
    async fn a_caller_can_add_its_own_device_without_giving_up_the_legacy_set() {
        // Composability is the point of the model: attaching the standard set
        // must not be a decision to hand-register everything else. If this
        // regresses, callers go back to bypassing `Machine` entirely.
        let manager = DeviceManager::new();
        let mine: Arc<RwLock<dyn Device>> =
            Arc::new(RwLock::new(SerialDevice::new("COM2".to_string(), 0x2F8)));

        Machine::legacy_pc()
            .with_device(MachineDevice::new(
                "COM2",
                mine,
                0x2F8,
                0x2FF,
                "a second console, for a caller that wants one",
            ))
            .attach(&manager)
            .await
            .unwrap();

        assert_eq!(
            manager
                .find_io_device(0x2F8)
                .await
                .expect("COM2")
                .device_name(),
            "COM2"
        );
        assert_eq!(
            manager
                .find_io_device(COM1_BASE)
                .await
                .expect("COM1")
                .device_name(),
            "COM1",
            "adding a device must not displace the legacy set"
        );
    }

    #[tokio::test]
    async fn a_device_that_lands_on_a_legacy_port_range_is_refused_rather_than_shadowing_it() {
        // Silent shadowing here would be the worst outcome: the guest would
        // find the wrong device at 0x70 and hang exactly the way it hung before
        // the RTC existed, with a machine model that claims to have fixed it.
        let manager = DeviceManager::new();

        let err = Machine::legacy_pc()
            .with_device(MachineDevice::new(
                "impostor",
                Arc::new(RwLock::new(RtcDevice::new())),
                0x71,
                0x7F,
                "overlaps the CMOS on purpose",
            ))
            .attach(&manager)
            .await
            .expect_err("an overlapping range must not be accepted");

        let message = err.to_string();
        assert!(
            message.contains("impostor"),
            "the error must name the offending device, got: {message}"
        );
    }

    #[tokio::test]
    async fn every_device_in_the_model_says_what_breaks_without_it() {
        // The list is only maintainable if each entry carries its own reason;
        // this stops a future device being added with an empty justification
        // and nobody able to tell later whether it is still needed.
        for device in Machine::legacy_pc().devices() {
            assert!(
                !device.why().is_empty(),
                "{} was added to the legacy set without a reason",
                device.name()
            );
            let (first, last) = device.ports();
            assert!(
                first <= last,
                "{} has an inverted port range",
                device.name()
            );
        }
    }

    #[tokio::test]
    async fn attaching_to_a_real_vm_maps_the_same_ports_it_maps_to_a_bare_manager() {
        // The bare-manager tests above prove the model; this proves the model
        // reaches a VM through `vm.devices()`, which is the call every caller
        // actually writes. Skipped where no hypervisor backend exists, matching
        // the convention in `vm`'s own tests.
        let config = crate::VMConfig {
            name: "machine-test".to_string(),
            vcpu_count: 1,
            memory_size: 64 * 1024 * 1024,
            ..Default::default()
        };
        let Ok(vm) = crate::VM::new(config) else {
            eprintln!("skipping: no hypervisor backend available");
            return;
        };

        Machine::legacy_pc().attach(&vm.devices()).await.unwrap();

        for port in [COM1_BASE, RTC_BASE, I8042_LAST] {
            assert!(
                vm.devices().find_io_device(port).await.is_some(),
                "port {port:#x} is unmapped on a VM the machine was attached to"
            );
        }
    }

    /// `attach_absent` on a bare manager installs the whole set: the case
    /// where a VM is filling in a machine nobody else provided.
    #[tokio::test]
    async fn attach_absent_installs_a_bare_machine() {
        let manager = DeviceManager::new();

        Machine::legacy_pc()
            .attach_absent(&manager)
            .await
            .expect("a bare manager has nothing in the way");

        for name in ["COM1", "RTC", "i8042"] {
            assert!(
                manager.get_device(name).await.is_some(),
                "{name} should have been attached"
            );
        }
    }

    /// Attaching twice is what a caller who installed their own machine before
    /// provisioning will do, and it must leave their devices alone rather than
    /// refuse them. The port ranges are already mapped at this point, so a
    /// second `attach` would fail on the mapping even if the name were free --
    /// which is exactly what this distinguishes.
    #[tokio::test]
    async fn attach_absent_leaves_an_existing_machine_alone() {
        let manager = DeviceManager::new();
        Machine::legacy_pc()
            .attach(&manager)
            .await
            .expect("first attach");

        let before = manager
            .get_device("COM1")
            .await
            .expect("COM1 attached by the caller");

        Machine::legacy_pc()
            .attach_absent(&manager)
            .await
            .expect("a machine already installed is not a conflict");

        let after = manager.get_device("COM1").await.expect("COM1 still there");
        assert!(
            Arc::ptr_eq(&before, &after),
            "the caller's own COM1 should survive, not be replaced"
        );

        assert!(
            Machine::legacy_pc().attach(&manager).await.is_err(),
            "a strict attach over an installed machine is still an error"
        );
    }
}
