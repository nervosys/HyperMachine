# First certificate group: publication crash

An owned wrapper hard-exits the deploy process with code 86 immediately after
atomic first-group manifest publication, before the reload signal. The actual
control plane remains alive, the private pending journal exists, and its default
TLS leaf remains available. Retrying the unmodified hook recovers the checksummed
journal, publishes the named group, verifies TLS and clears the journal. The
HTTP-01 counter does not increase during retry; no CA reissuance occurs.

All fourteen owned Pebble/Certbot/live TLS checks pass. The first named group
initially uses the same leaf as the default, covering that recovery ambiguity;
the subsequent renewal activates a different verified leaf. Default preservation,
idempotency, untrusted rollback, key/process/manifest refusals, lock serialization,
expired-leaf recovery and unavailable challenge refusal remain checked.

The report includes the exact owned fault-wrapper source and hash. Production
code has no fault-injection switch. All owned processes are reaped and the HTTP
thread stops. No public CA is contacted. This verifies one first-publication crash
boundary, not every crash boundary, a new-group KVM workload or automatic initial
issuance after a domain claim. No performance superiority is claimed.
