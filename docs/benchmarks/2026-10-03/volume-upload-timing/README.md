# Verified local volume upload timing

One unchanged dev-profile daemon binary (identity in summary.json; build provenance in ../volume-atomic-upload/build.log) was tested in in-place/atomic/atomic/in-place order. Two daemons share one local WSL volume directory with separate snapshot stores. Each cohort passes all volume creation, duplicate, token, binary replacement, cross-node read, restart and cleanup checks; atomic cohorts additionally check incomplete-upload preservation and staging cleanup.

Each timing cohort performs three warm-up uploads and twelve measured sequential PUTs of the same 1,048,586-byte binary payload. Timing starts before the host HTTP PUT and ends after its response, including request transfer, server writes, metadata, file/directory flushes and response. Exact reads on both nodes follow every upload and are excluded from request timing. No guest is involved.

| Pair | In-place median (ms) | Atomic median (ms) |
| --- | ---: | ---: |
| AB | 19.660 | 22.905 |
| BA | 9.975 | 15.083 |

Median of the two run medians is 14.818 ms in-place and 18.994 ms atomic (28.2% higher). Atomic is slower in both pairs, but substantial time drift between cohorts prevents a stable overhead estimate. Report individual pairs rather than treating the aggregate as production guidance. Atomic mode provides previous-file preservation on ordinary pre-publication failures; default in-place mode retains inode identity. Both modes sync files and directories. There is no before/after claim about the earlier unsynced implementation.

This is dev-profile local filesystem characterization, not optimized-release, network storage, fleet throughput, P99, power-loss or competitor evidence. Hashes before/after ensure the binary and guest input bytes did not change. Full reports, sample arrays and logs are frozen here. No access tokens or uploaded bytes are archived.
