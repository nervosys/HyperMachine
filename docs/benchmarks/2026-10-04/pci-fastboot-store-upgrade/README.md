# Old-release store adoption after PCI boot changes

Both owned same-host/same-path upgrade profiles pass 21 checks each. The prior frozen release creates a required base template and guest, saves a unique marker inside the guest, pauses it to disk and shuts down cleanly. The candidate reopens the same store, resumes the same guest with its ID and access token preserved, validates its marker, forks an isolated child, and completes two more parent disk pause/resume cycles while the child retains its own marker. Both guests are deleted, inventory is empty and both daemon phases exit zero. Owned stores are removed only after successful cleanup; reports, logs and cache names remain.

| Verified upgrade behavior | PCI | Default MMIO |
| --- | --- | --- |
| Old paused guest state and identity | Preserved | Preserved |
| Forked child isolation after adoption | Passed | Passed |
| Candidate disk resume cycles | 2 | 2 |
| Old template key after restart | Retained alongside new PCI key | Reused unchanged |
| Candidate startup Linux image loads | 1 for new base | 0 |
| Passed checks | 21/21 | 21/21 |

The logs corroborate cache behavior: the candidate restores an existing MMIO template without a Linux load, while PCI creates its new base and keeps the prior base directory available for the stored guest. This closes the previously unverified old-store adoption gate for these two exact binaries and this guest fixture. Keep prior template directories while paused guests can reference them. This does not establish general version migration, different store paths/hosts, mixed transports, other kernels, network/volume stores, interruption or crash recovery.

Before binary SHA is dc315d313fe2f156ab7aaa593e02c4e0d04ffa024cfb5c69c85d2780a270676e; after is 5d136c14b7ac36bd23af194ea82e844573a3ca18bfa7640e8e3d5e669db80f23. The same immutable kernel/buffered guest, eight-CPU affinity and one-vCPU/1024-MiB defaults apply. The source catalog is the permitted previous nine-file catalog plus daemon-only candidate hash, not full build closure. No sources were built or changed; protected root sources were excluded. Frozen binary/input hashes were checked before and after both profiles.

There are eighteen exact saved-marker command validations, four candidate disk resume cycles, four deleted guests and four normal daemon exits across both profiles. The token is compared inside the owned fixture and is not archived; reports retain the successful check. Raw evidence proves functional compatibility, not a timing, memory, throughput or competitor win.

Reproduce driver.py using the unchanged frozen inputs with a fresh exclusive output directory. python3 verify.py checks archive hashes, input/binary catalog consistency, all 42 check records, command and cycle counts, actual template keys/load/restore logs, exits and cleanup. Historical failing cold experiments remain unchanged in their own archives.
