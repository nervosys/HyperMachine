# Pending registration excluded from idle eviction

On a full node, the previous selector could repeatedly choose the oldest auto-resume guest even when its registration was uncertain. Pause correctly refused that guest; selection then chose it again, preventing a newer eligible idle guest from releasing a slot. The fix excludes pending registrations before selecting an eviction victim.

The controlled baseline reproduces starvation with three slots: one eligible guest, one guest with auto-resume disabled, and one older pending guest after touching the eligible guest. An owned Redis XADD refusal keeps registration uncertain. The baseline selected the same pending guest 552 times before the creation admission wait ended, then the fixture reported the expected failure. The baseline has no structured terminal cleanup report; no passing or complete-cleanup claim is made for it.

With the fixed frozen daemon, SET/IPv4-two-peer and XADD/IPv6-eight-peer profiles pass 34 checks each. The same admission scenario pauses the eligible guest and creates a replacement while preserving the older pending guest and the guest with auto-resume disabled. Both uncertain guests remain discoverable. Restoring event publication recovers both automatically; paused and running guests are deleted. Named recovery/name reuse, fork, owner authorization, native TCP/UDP and resume checks remain passing. All fixed-profile guests and owned processes are cleaned up, and runtime input hashes match frozen binaries.

All 58 ordinary daemon tests pass; two existing explicit KVM tests remain ignored in the ordinary suite and were not rerun for this single eligibility-condition change. No new test mirrors the predicate: the controlled before/after KVM scenario exercises actual selection, pause, admission, registration recovery and cleanup.

Earlier fixture attempts are excluded: one assumed unfiltered inventory omitted paused guests; another used two slots but the later workflow requires a parent and two fork children. The final fixture uses the running-state query and three slots with an auto-resume-disabled guest, preserving the full eviction condition.

This is a functional availability fix under owned publication refusal, not a competitor performance benchmark, load/loss guarantee or daemon-crash recovery result. Reproduce with the archived drivers and frozen hashes, using fresh output paths. Protected modified root core files remain excluded from the isolated build.
