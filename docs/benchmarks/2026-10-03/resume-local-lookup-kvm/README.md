# Shared and local paused-state KVM regression

The current daemon passes four real KVM profiles: shared-store migration has 27 checks each for IPv4/two peers and IPv6/eight peers; in-memory-only pause lifecycle has 22 checks each. All leave zero guests and reap every owned process. Reports record the paused-state mode and verified input hashes.

The checker now accepts --in-memory-pauses, omitting the primary daemon shared snapshot store and rejecting combination with shared-store adoption/migration checks. Local runs verify normal owner-preserving fork, pause/resume, exact TCP/UDP delivery and reservation/deletion lifecycle. Shared runs additionally verify actual cross-node migration preserving the same byte-identical both-protocol allocation and public port. No timed comparison block was requested, so local profiles contain 22 checks rather than the 23 checks in earlier performance profiles.

These normal runtime regressions do not inject cancellation. The separate 51-test evidence under ../resume-local-lookup-cancellation verifies synchronized cancellation/completion of the rollback helper. VM-startup/registration cancellation and machine-crash recovery remain incomplete. Development builds and loopback fixtures establish no performance or competitor advantage.

The updated node is frozen; control/CLI/gateway/kernel/image remain immutable dependencies. Source/harness snapshots, driver, build log and raw reports preserve scope. Builds ran only in the isolated checkout; protected root core files remain excluded.
