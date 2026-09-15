//! The loop that connects a guest's network device to the world.
//!
//! Three pieces existed and none of them touched: a device the guest can drive
//! ([`VirtioNetMmio`]), a translator ([`NatTable`]), and
//! a host interface ([`TapDevice`](crate::tap::TapDevice)). A VM with all three
//! attached still had no network, because nothing carried a frame from one to
//! the next. This is that carrier, and it is deliberately the only thing in
//! this file.
//!
//! ```text
//!   guest driver -> virtqueue -> VirtioNetMmio -> NAT out -> host link
//!   guest driver <- virtqueue <- VirtioNetMmio <- NAT in  <- host link
//! ```
//!
//! # Why it polls
//!
//! One direction has a signal and the other does not. A frame the host offers
//! wakes the guest, because `queue_received` runs the wake hook `VM::attach_net`
//! installed. A frame the guest sends arrives in `tx_pending` during the guest's
//! own kick — on the vCPU thread, inside the device lock, with no way to notify
//! anything from there. So the guest-to-host direction is polled, and the
//! interval is the latency floor for outbound traffic. It is named and passed
//! in rather than hidden, because it is the number someone measuring this will
//! want to change.
//!
//! # What NAT failure means here
//!
//! A frame NAT declines to translate is dropped and counted, not passed
//! through. Passing it through is the failure that matters: an untranslated
//! frame carries the guest's private source address onto the host's network,
//! where the reply has nowhere to come back to — a link that works for
//! everything except what it was built for, and only intermittently.

use std::sync::Arc;

use hv2_core::devices::virtio_net_mmio::{VirtioNetMmio, MAX_FRAME_LEN};

use crate::nat::NatTable;
use crate::Result;

/// The host side of a bridge: somewhere frames go and come from.
///
/// A trait rather than [`TapDevice`](crate::tap::TapDevice) directly, because a
/// TAP device needs a privileged process and a configured host interface, and a
/// loop that can only be exercised with both is a loop nothing tests. The TAP
/// implementation is [`TapLink`]; the tests below use a link that keeps frames
/// in a queue.
#[async_trait::async_trait]
pub trait HostLink: Send + Sync {
    /// Send one frame towards the world. Returns bytes accepted.
    async fn send(&self, frame: &[u8]) -> Result<usize>;

    /// Take one frame from the world, or an empty vector if none is waiting.
    ///
    /// Must not block: this is called in a loop that also has to service the
    /// other direction.
    async fn recv(&self) -> Result<Vec<u8>>;
}

/// A [`HostLink`] backed by a real TAP device.
pub struct TapLink {
    tap: crate::tap::TapDevice,
}

impl TapLink {
    /// Open the TAP device described by `config`.
    ///
    /// The frames either side of this are bare Ethernet, so `vnet_hdr` must be
    /// off. `TapConfig` defaults it *on*, which is right for a caller handing
    /// the kernel virtio frames whole and wrong here: the device has already
    /// stripped the header on the way out and puts one back on the way in. A
    /// mismatch is not an error anything reports -- every frame is simply
    /// offset by twelve bytes, in both directions, and looks like corruption.
    /// So it is refused rather than quietly corrected, because a caller who
    /// set it meant something by it.
    ///
    /// # Errors
    ///
    /// Refuses a config with `vnet_hdr` set. Otherwise propagates whatever the
    /// platform says about the interface — on Linux usually a permissions
    /// answer, since creating one needs `CAP_NET_ADMIN`. An interface someone
    /// already made persistent and owns needs no privilege at all, which is
    /// the arrangement worth having.
    pub async fn open(config: crate::tap::TapConfig) -> Result<Self> {
        if config.vnet_hdr {
            return Err(crate::NetError::Config(
                "a bridge carries bare Ethernet frames, so this TAP device must be opened                  without a vnet header: TapConfig::with_vnet_hdr(false)"
                    .to_string(),
            ));
        }
        let mut tap = crate::tap::TapDevice::new(config);
        tap.create().await?;
        Ok(Self { tap })
    }

    /// The interface name the host knows this by.
    #[must_use]
    pub fn name(&self) -> &str {
        self.tap.name()
    }
}

#[async_trait::async_trait]
impl HostLink for TapLink {
    async fn send(&self, frame: &[u8]) -> Result<usize> {
        self.tap.write(frame).await
    }

    async fn recv(&self) -> Result<Vec<u8>> {
        self.tap.read().await
    }
}

/// What a bridge has moved, and what it could not.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BridgeStats {
    /// Frames carried from the guest to the host link.
    pub out_frames: u64,
    /// Frames carried from the host link to the guest.
    pub in_frames: u64,
    /// Frames dropped because NAT would not translate them.
    pub untranslatable: u64,
    /// Frames dropped because the guest's receive backlog was full, or the
    /// frame was one the device would not take.
    pub refused_by_guest: u64,
}

/// A guest network device joined to a host link, with NAT in between.
pub struct Bridge<L: HostLink> {
    device: Arc<parking_lot::Mutex<VirtioNetMmio>>,
    link: L,
    /// `None` bridges frames untranslated, which is what a guest on a private
    /// switch with the host wants. `Some` is the routed case.
    nat: Option<NatTable>,
    stats: BridgeStats,
}

impl<L: HostLink> Bridge<L> {
    /// Join `device` to `link`, translating through `nat` if given.
    pub fn new(
        device: Arc<parking_lot::Mutex<VirtioNetMmio>>,
        link: L,
        nat: Option<NatTable>,
    ) -> Self {
        Self {
            device,
            link,
            nat,
            stats: BridgeStats::default(),
        }
    }

    /// What this bridge has moved so far.
    #[must_use]
    pub fn stats(&self) -> BridgeStats {
        self.stats
    }

    /// Carry everything waiting, in both directions, and return.
    ///
    /// Does not block on either side: a direction with nothing in it costs one
    /// check. Returns how many frames moved, so a caller pacing itself can tell
    /// a busy link from an idle one.
    ///
    /// # Errors
    ///
    /// Propagates a host link failure. A frame NAT refuses is not an error: it
    /// is dropped, counted, and the loop carries on, because one malformed
    /// packet from a guest must not take the link down.
    pub async fn pump(&mut self) -> Result<u64> {
        let mut moved = 0;

        // Guest to host. Everything queued, rather than one frame, so a burst
        // is not spread across as many polling intervals as it has frames.
        loop {
            let Some(mut frame) = self.device.lock().take_transmitted() else {
                break;
            };
            if let Some(nat) = self.nat.as_mut() {
                if let Err(e) = nat.translate_outbound(&mut frame) {
                    self.stats.untranslatable += 1;
                    tracing::debug!("bridge: dropping an outbound frame: {e:?}");
                    continue;
                }
            }
            self.link.send(&frame).await?;
            self.stats.out_frames += 1;
            moved += 1;
        }

        // Host to guest, bounded by what the guest can hold: a link busier than
        // the guest reads would otherwise spin here filling a backlog that
        // drops the frames at the far end of it.
        for _ in 0..MAX_BURST {
            let mut frame = self.link.recv().await?;
            if frame.is_empty() {
                break;
            }
            if frame.len() > MAX_FRAME_LEN {
                self.stats.refused_by_guest += 1;
                continue;
            }
            if let Some(nat) = self.nat.as_mut() {
                if let Err(e) = nat.translate_inbound(&mut frame) {
                    self.stats.untranslatable += 1;
                    tracing::debug!("bridge: dropping an inbound frame: {e:?}");
                    continue;
                }
            }
            // This is also what wakes the guest: the device's wake hook, which
            // `VM::attach_net` installed, publishes and raises the interrupt.
            if self.device.lock().queue_received(frame) {
                self.stats.in_frames += 1;
                moved += 1;
            } else {
                self.stats.refused_by_guest += 1;
            }
        }

        Ok(moved)
    }

    /// Pump forever, sleeping `idle` whenever there was nothing to carry.
    ///
    /// Never returns except on a host link error, which is fatal to a bridge:
    /// a TAP device that has stopped answering is not something a retry loop
    /// recovers, and pretending otherwise gives a guest a link that silently
    /// carries nothing.
    ///
    /// # Errors
    ///
    /// Propagates the host link failure that ended it.
    pub async fn run(mut self, idle: std::time::Duration) -> Result<()> {
        loop {
            if self.pump().await? == 0 {
                tokio::time::sleep(idle).await;
            }
        }
    }
}

/// Frames taken from the host link in one pump, at most.
///
/// Not a tuning parameter so much as a fairness one: without a bound, a link
/// with a steady inbound stream is never left, and the guest's own frames wait
/// behind it indefinitely.
const MAX_BURST: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_core::memory::GuestMemory;
    use std::collections::VecDeque;
    use tokio::sync::Mutex as AsyncMutex;

    /// A host link that keeps frames in memory.
    ///
    /// The point of `HostLink` being a trait: a TAP device needs
    /// `CAP_NET_ADMIN` and a configured host interface, so a bridge that could
    /// only be exercised through one would be a bridge nothing ran.
    #[derive(Default)]
    struct Loopback {
        sent: AsyncMutex<Vec<Vec<u8>>>,
        waiting: AsyncMutex<VecDeque<Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl HostLink for Loopback {
        async fn send(&self, frame: &[u8]) -> Result<usize> {
            self.sent.lock().await.push(frame.to_vec());
            Ok(frame.len())
        }

        async fn recv(&self) -> Result<Vec<u8>> {
            Ok(self.waiting.lock().await.pop_front().unwrap_or_default())
        }
    }

    fn device() -> Arc<parking_lot::Mutex<VirtioNetMmio>> {
        Arc::new(parking_lot::Mutex::new(VirtioNetMmio::new([
            0x52, 0x54, 0x00, 1, 2, 3,
        ])))
    }

    /// The frame in guest memory laid out as a driver lays it out, so that
    /// the device really does drain a ring rather than being handed a frame
    /// the host made up.
    fn guest_sends(mem: &GuestMemory, dev: &mut VirtioNetMmio, frame: &[u8]) {
        const DESC: u64 = 0x2000;
        const AVAIL: u64 = 0x2400;
        const USED: u64 = 0x2800;
        const DATA: u64 = 0x2c00;
        const HDR: usize = 12;

        {
            let q = &mut hv2_core::devices::VirtioMmioDevice::queues(dev)[1];
            q.set_size(8);
            q.set_desc_addr(DESC);
            q.set_avail_addr(AVAIL);
            q.set_used_addr(USED);
            q.set_ready(true);
        }

        let mut buffer = vec![0u8; HDR];
        buffer.extend_from_slice(frame);
        mem.write_bytes(DATA, &buffer).expect("frame");

        let mut desc = [0u8; 16];
        desc[0..8].copy_from_slice(&DATA.to_le_bytes());
        desc[8..12].copy_from_slice(&(buffer.len() as u32).to_le_bytes());
        mem.write_bytes(DESC, &desc).expect("descriptor");
        mem.write_bytes(AVAIL + 4, &0u16.to_le_bytes())
            .expect("ring entry");
        mem.write_bytes(AVAIL + 2, &1u16.to_le_bytes())
            .expect("avail idx");

        hv2_core::devices::VirtioMmioDevice::notify(dev, 1, mem).expect("kick");
    }

    /// A frame the guest really transmitted reaches the host link.
    #[tokio::test]
    async fn a_frame_the_guest_sent_reaches_the_host_link() {
        let mem = GuestMemory::new(0x10000).expect("guest memory");
        mem.allocate_region(0x10000, false).expect("region");
        let dev = device();
        let mut bridge = Bridge::new(dev.clone(), Loopback::default(), None);

        guest_sends(&mem, &mut dev.lock(), &[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(bridge.pump().await.expect("pump"), 1);
        assert_eq!(bridge.stats().out_frames, 1);
        assert_eq!(
            bridge.link.sent.lock().await.as_slice(),
            &[vec![0xde, 0xad, 0xbe, 0xef]],
            "and it arrives without the virtio header"
        );
    }

    /// And the other direction, which is the half a one-sided test would miss.
    #[tokio::test]
    async fn a_frame_from_the_host_link_reaches_the_guest() {
        let dev = device();
        let mut bridge = Bridge::new(dev.clone(), Loopback::default(), None);

        bridge.link.waiting.lock().await.push_back(vec![1, 2, 3, 4]);
        assert_eq!(bridge.pump().await.expect("pump"), 1);
        assert_eq!(bridge.stats().in_frames, 1);
        assert_eq!(dev.lock().pending_receive_len(), 1);
    }

    /// An idle bridge does nothing and says so, which is what `run` paces on.
    #[tokio::test]
    async fn an_idle_bridge_carries_nothing() {
        let mut bridge = Bridge::new(device(), Loopback::default(), None);
        assert_eq!(bridge.pump().await.expect("pump"), 0);
        assert_eq!(bridge.stats(), BridgeStats::default());
    }

    /// An over-long frame from the host is dropped at the bridge rather than
    /// handed to a device that would refuse it anyway -- so the count says
    /// which side refused it.
    #[tokio::test]
    async fn an_oversized_frame_from_the_host_is_dropped_and_counted() {
        let dev = device();
        let mut bridge = Bridge::new(dev.clone(), Loopback::default(), None);
        bridge
            .link
            .waiting
            .lock()
            .await
            .push_back(vec![0u8; MAX_FRAME_LEN + 1]);

        bridge.pump().await.expect("pump");
        assert_eq!(bridge.stats().refused_by_guest, 1);
        assert_eq!(dev.lock().pending_receive_len(), 0);
    }

    /// One pump takes a whole burst rather than one frame, or a guest sending
    /// ten frames waits ten polling intervals for the last of them.
    #[tokio::test]
    async fn a_burst_is_carried_in_one_pump() {
        let dev = device();
        let mut bridge = Bridge::new(dev.clone(), Loopback::default(), None);
        {
            let mut waiting = bridge.link.waiting.lock().await;
            for i in 0..5u8 {
                waiting.push_back(vec![i, i, i]);
            }
        }
        assert_eq!(bridge.pump().await.expect("pump"), 5);
    }
}
