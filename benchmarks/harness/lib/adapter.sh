# Adapter ABI (informational)

Every adapter under `benchmarks/harness/adapters/<hv>.sh` is invoked with a
verb as `$1` and verb-specific arguments after it. Adapters MUST be safe to
re-invoke and MUST NOT depend on shared in-process state — all state lives on
disk under `$BENCH_DIR/.state/<hv>/<vm_id>/`.

## Verbs

| Verb        | Args                                  | Exit | stdout                       |
|-------------|---------------------------------------|------|------------------------------|
| `setup`     | image cpus mem_mib disk_path netif    | 0    | vm_id (opaque token)         |
| `start`     | vm_id                                 | 0    | pid                          |
| `wait_ssh`  | vm_id ssh_port timeout_s              | 0/124| elapsed seconds              |
| `snapshot`  | vm_id name                            | 0/78 | -                            |
| `restore`   | vm_id name                            | 0/78 | -                            |
| `stop`      | vm_id                                 | 0    | -                            |
| `destroy`   | vm_id                                 | 0    | -                            |
| `metrics`   | vm_id                                 | 0    | `rss_kib=N,cpu_pct=F`        |

Exit code 78 = "operation not implemented for this hypervisor" — the
orchestrator treats this as **skip**, not failure.

Each adapter is sourced via `bash <adapter>.sh <verb> <args...>`, so they may
use `set -euo pipefail` independently of the caller.
