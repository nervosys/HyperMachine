# Owner sharing during live Redis outages

Thirteen local nested-KVM cases pass using the same frozen control plane, CLI, accepted default-MMIO daemon/kernel and guest as the preceding owner-sharing gate. Three new cases opt in with `--redis-outage`; ten prior owner API/CLI/TLS cases pass again. No production source or binary changed in this extension.

With a current active owner grant, the real guest is paused and the owned Redis is killed. Two browser requests return 401 with challenges. Node detail over independently authenticated mTLS and cluster token confirms the guest stays paused after each denial, without relying on the unavailable Redis/control-plane store. One denial is immediate after connection failure and the other reaches the five-second store timeout; both meet an eight-second functional deadline. These timings are failure-path checks, not a throughput or competitor benchmark.

The same Redis data directory restarts with appendonly=yes/appendfsync=always. The current grant/revision survives, and allowed browser traffic auto-resumes the real guest. A second hard Redis restart preserves an owner revocation; denied traffic leaves the guest paused, a stale grant replay returns 409, and a fresh revision regrant permits auto-resume. Logs show three distinct Redis processes and two AOF reloads. Both control-plane process restarts from the prior gate are also repeated.

All thirteen cases pass, both guests are deleted to empty inventory, all seven registered processes are waited/stopped, private fixture files are removed and all frozen input hashes remain unchanged. Independent checks verify reports, input/release hashes, process/AOF counts, deadline bounds and cleanup assertions. Logs are byte-identical copies with `.txt` added to archive filenames. Private certificates and generated credentials are not archived.

This verifies exact local store outage/recovery and no-wake behavior with Redis 8.0.2/AOF-always and a paused guest. It does not establish host power loss, managed replication/failover, large-scale outage recovery, arbitrary kernels/nodes, long-lived stream revocation, SSO, tenant/team isolation or performance superiority. Existing admitted streams retain original admission.

Run the preceding accepted-input driver with `--redis-outage` and a new output directory. Verify this archive with `python verify.py`.
