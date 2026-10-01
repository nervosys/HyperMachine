# Halted-vCPU deadline timer control (2026-10-01)

The [pause/resume sweep](pause-bursts.md) retained one resume readiness failure
with a halted vCPU, empty LAPIC IRR and a nonzero TSC deadline. This test isolates
deadline-timer restoration from Linux scheduling, vsock, the agent and daemon
retries. It is evidence about one restore path, not an explanation of that failure.

The explicit hardware test `restored_deadline_timer_wakes_halted_guest` creates
a real-mode VM, configures a local APIC deadline timer, captures a halted vCPU
with interrupts enabled, then restores it into a fresh VM through the production
backend. Vector 0x22 enters a tiny handler that outputs 0x22 on port 0xe9; the
uninterrupted main path outputs 0x11. The restored timer reaches the handler
within a two-second bound. An otherwise identical restore omitting only the
deadline MSR stays halted through the bound, then an owned vCPU kick releases
and joins its runner thread. No unavailable-KVM fallback counts as a pass.

Both controls passed on the local nested-KVM host. This demonstrates that a
captured deadline can wake a halted guest through the current backend; it does
not establish all timing windows, oversubscription behavior, multi-vCPU clocks
or the cause of the SDK timeout. No runtime timer behavior was changed.

The backend restores LAPIC state before clock MSRs. The upstream
[KVM LAPIC implementation](https://github.com/torvalds/linux/blob/master/arch/x86/kvm/lapic.c)
requires deadline mode when setting a deadline and cancels/reinitializes the
timer when replacing LAPIC state. This informed the control; the local execution
result remains the evidence for this host.

Separately, CI run 36863784476 failed strict builds on the deprecated atomic
`fetch_update` call in snapshot ID generation. The replacement uses a weak
compare-exchange loop, retaining sequential consistency, the timestamp clamp
and existing saturation behavior. It supports the project's Rust 1.95 minimum
without requiring the newer replacement helper or suppressing warnings.
An eight-thread test checks 8,000 unique IDs and increasing IDs within each
thread. Snapshot type tests and strict core Clippy pass on minimum-version Linux
and current Windows Rust. Remote CI must still verify the new commit.

[Raw probe output and exact source hashes](deadline-timer-probe.json) retain
the compiler/kernel versions, commands, hardware controls, snapshot tests and
strict Linux Clippy. The executable probe coordinator is retained beside it.
