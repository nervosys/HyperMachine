# Retained sparse named-image candidate

This isolated revision retains named memory images when a name is deleted, so dependent children and persisted layers can still use them. Failed unpublished captures still remove their sidecars. No production runtime change is adopted.

The release build passed. Candidate Clippy passed with only the existing too-many-arguments lint waived; strict lint cleanliness is not claimed. All 15 Linux offline reclamation tests passed, including live-store lock refusal, stale plans, checksums, missing dependencies, corrupt snapshots, symlinks and hard links. These are synthetic dependency tests, not proof of guest survival across collection.

Both accepted and candidate real KVM guests preserved prepared files, an independent child write, and a live process after deleting the source name and pausing/resuming the child. Guest inventory returned to zero, owned nodes exited zero, and input hashes stayed unchanged. This repairs the previous candidate regression.

The concurrency-one cohort is a local smoke comparison with counterbalanced fresh daemons and Firecracker controls; see c1/analysis.json and raw reports under c1. All 16 restores passed (four per daemon variant and eight Firecracker controls).

| Measurement | Accepted baseline | Retained-image candidate |
|---|---:|---:|
| Mean restore readiness | 35.46 ms | 49.90 ms |
| Median held daemon PSS | 70.88 MiB | 72.24 MiB |
| Median named capture time | 13.68 ms | 744.29 ms |
| Median named-source logical size | 1.62 MiB | 1024.05 MiB |

Logical size does not measure physical allocation for sparse files. Both outer pairs had worse candidate mean/tail readiness and higher held PSS. Firecracker control mean shifts were +51.89 ms and -0.87 ms, showing host variation; four samples per variant cannot establish tail distributions. The candidate is deferred, with no high-concurrency or managed competitor win established. Restart, replacement, collection with real persisted guests, shared-store races, and physical storage/capture tradeoffs remain adoption requirements.
