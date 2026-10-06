# Committed resume record with refused event publication

Both IPv4/two-peer and IPv6/eight-peer real KVM profiles pass 23 checks and complete cleanup. The owned Redis default user temporarily lacks XADD permission during unnamed resume. The running record commits with original owner/node identity, but lifecycle event publication fails and resume returns 503. The pending marker remains active: pause returns 409 and reconciliation with the wrong cluster credential returns 401. Restoring XADD allows authenticated reconciliation to return 200; exact resumed UDP/native TCP/UDP and final allocation/deletion cleanup pass.

The checker supports --resume-publication-fault-command XADD with --resume-publication-fault. SET remains the default for record-write refusal. Both commands are restored in finally. This run uses the same frozen current daemon as the preceding SET experiment; no production source or build changed. Runtime input hashes and source catalog are verified.

This establishes recovery of the same locally running guest after a committed record and refused event write. It does not inject cancellation, a committed event with lost reply, initial unnamed creation failure or a machine crash. Reconciliation can republish events after uncertain delivery; exactly-once delivery is not claimed. Startup cancellation and durable crash recovery remain incomplete. Owned loopback development runs establish no performance or competitor superiority.

Raw reports/logs, checker/source snapshots, driver, source context and manifest preserve evidence. Private keys, credentials and temporary policies are excluded. Build provenance is retained in ../unnamed-resume-publication-fault and ../unnamed-registration-uncertainty.
