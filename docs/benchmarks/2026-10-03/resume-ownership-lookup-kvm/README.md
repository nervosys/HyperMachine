# Updated resume ownership guard: KVM lifecycle regression

The updated daemon passes two real KVM profiles, IPv4/two peers and IPv6/eight peers, with 27 checks each. Both leave zero guests and reap daemon, secondary node, control plane, Redis, CLI and native gateway. Runtime input SHA256 hashes and the updated 133-file permitted source catalog are verified.

The profiles exercise administrator running/paused legacy adoption and replay, cross-node adoption/resume races, actual execution and owner-preserving fork after migration. A both-protocol allocation remains byte-identical while a guest pauses on node two and resumes on node one. The same gateway restores TCP/UDP on the same public port; exact 1 MiB TCP and empty/binary/maximum UDP payload checks pass. Final deletion clears allocation and closes sessions.

These are regression checks of the updated daemon's normal resume/migration behavior. They do not cancel an HTTP request during ownership lookup: synchronized cancellation and completed-error cleanup are covered separately in the 49-test daemon evidence under ../resume-ownership-lookup-cancellation. Client disconnects do not necessarily cancel an accepted server operation, so normal HTTP lifecycle success is not substituted for that cancellation proof.

Builds ran only in the isolated checkout. The node executable is frozen; control plane, CLI, gateway, kernel and image are unchanged dependencies from the accepted migration fixture. This development-profile run makes no timed performance claim. Machine-crash recovery, startup/registration cancellation, in-memory-only paused cancellation, public operation and managed failover remain unverified or incomplete.
