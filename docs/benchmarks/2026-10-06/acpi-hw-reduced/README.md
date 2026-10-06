# MMIO guests boot on a hardware-reduced ACPI platform

**Change:** a guest on the MMIO transport (the default) is now also given ACPI tables that declare
the platform hardware-reduced: RSDP → XSDT → FADT (revision 6, `HW_REDUCED_ACPI`) and a MADT
without `PCAT_COMPAT`, plus a DSDT that names the UART and every virtio-mmio window with its
interrupt (`crates/hv2-core/src/boot/acpi_tables.rs`). The virtio-mmio windows move from the
kernel command line into the DSDT. PCI-transport guests are unchanged and keep the MP table
alone, because the DSDT has no PCI host bridge and an x86 Linux that finds ACPI does not probe
PCI configuration space by itself. The daemon opts in with `VM::use_hw_reduced_acpi()`.

## Why

With only an MP table, Linux assumes a PC. Decoded from the trace
(`exit-counts/mptable-guest-io-sequence.txt`), every boot:

- initialised the 8259 PIC (ports `0x20`/`0x21`/`0xA0`/`0xA1`);
- read all 24 I/O APIC redirection entries, then masked each one with a read, a write and a
  read-back, at two MMIO exits per register access (select, then window);
- probed the CMOS RTC further.

Firecracker's guest of the same kernel does none of this
(`exit-counts/firecracker-guest-io-sequence.txt`): its tables say hardware-reduced, so Linux uses
no legacy PIC, and `enable_IO_APIC()` returns before the mask pass. It programs only the entries
its drivers request.

**A first attempt failed, and why matters.** Writing the tables with an empty DSDT made every
guest panic. Without a legacy PIC, Linux no longer maps ISA IRQs 0–15 to I/O APIC pins, so the
`virtio_mmio.device=…:5` command-line form handed the driver an IRQ number nothing routed. Every
pin stayed masked, the guest agent found no vsock device and exited, and init died. Describing
each device in the DSDT with an `Interrupt` resource, as Firecracker does, is what makes it work.

## Exit counts (load-independent)

Method as in [`guest-thp`](../guest-thp/README.md): `kvm_exit`, `kvm_pio` and `kvm_mmio`
tracepoints while `tools/bench-local-engines.py` booted three guests per engine, with Firecracker
1.17.0 as the control. The baseline is the huge-page daemon of #143, which is master's behaviour.
Scripts: `build-thp.sh`, `run-thp.sh`, `analyze.py`, `ioapic.py`, `window.py`.

| Per guest boot | Baseline (master) | Candidate (ACPI) | Firecracker 1.17.0 |
|---|---:|---:|---:|
| VM exits | 2,809–2,991 | **2,250–2,303** | 23,596–23,846 |
| I/O APIC MMIO accesses | 439 | **30** | 50 |
| RTC port I/O (`0x70`/`0x71`) | 124 | **40** | 40 |
| PIC port I/O | 38 | **0** | 0 |
| Nested page faults (mostly MMIO) | 659 | **251** | 21,582 |
| Boots ready | 3/3 | 3/3 | 3/3 per run |

**About 600 fewer exits per boot (21%).** The I/O APIC, RTC and PIC counts now match
Firecracker's. The candidate programs two redirection entries, RTE4 (UART) and RTE5 (vsock), and
nothing else (`exit-counts/candidate-ioapic.txt`). Daemon RSS at readiness is unchanged
(103 / 118 / 133 MB in both runs).

## What this does not establish

- **Latency.** The host was a shared WSL machine at high CPU from other work, and Firecracker's
  own readiness ranged 898–2,107 ms in this run. The `ready_ms` values are recorded but support
  no claim.
- **Guest behaviour beyond readiness** under ACPI: no ACPI power-off or reset register is
  described, so shutdown and reboot go the way they did before (the 8042 is still declared
  present).
- Networked guests: this run booted guests with vsock only. A networked guest's virtio-net window
  is described the same way, and the unit tests cover two windows, but no networked guest was
  traced.
- Anything about boxd or exe.dev.

Binaries and guest inputs are identified in `artifact-sha256.txt`; the bench reports carry the
same daemon hashes.
