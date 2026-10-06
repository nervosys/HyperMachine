# Restored guest readiness failure diagnostic

Separate diagnostic with 90-second API observation, no retries, and existing agent readiness-stage debug logs enabled. Daemon readiness remains 15 seconds and production binary/runtime inputs match the acknowledged-pause candidate. Not a passing original gate.

The main target accepted the vsock connection in 9.69 ms, then failed its restore request after 15001.60 ms. The subsequent clock fallback could not connect within another 15 seconds. Server diagnostics returned HTTP 503 after about 30.5 seconds; no final report or guest-count proof exists.

Before the diagnostic kick, no vCPU exits occurred in the sampled half second (5 total). The owner sample reports Halted, RFLAGS 0x202, TSC deadline above sampled TSC, LAPIC timer deadline mode and empty LAPIC IRR/ISR. IOAPIC IRR has GSI5 set, configured for edge-triggered vector 0x22. These observations narrow the stall to after initial connection acceptance; they do not prove whether clock/timer restoration, IRQ delivery or guest execution is causal. The diagnostics themselves kick the vCPU and must not be used as an unperturbed runtime benchmark.

Raw logs, focused extraction and executed checker/driver are hashed. Original 30-second failures remain recorded separately. No lifecycle fix or CPU improvement is accepted.
