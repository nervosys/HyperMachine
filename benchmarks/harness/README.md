# harness internals

See [`../README.md`](../README.md) for user-facing docs.

## Orchestrator (`run.sh` / `run.ps1`)

For each enabled hypervisor and workload from the config:

1. Resolve workload class:
   - **Orchestrator-owned**: `boot_cold`, `boot_warm` — runner drives the VM
     lifecycle and times specific phases.
   - **Self-driven**: `density`, `mgmt_latency` — workload script owns the
     VM lifecycle (it loops over many VMs internally).
   - **Per-sample**: everything else — runner starts one VM, runs a warm-up,
     then N timed samples, each invocation of the workload script measuring
     once. Cooldown sleep between samples.
2. Each workload invocation prints TSV `metric<TAB>value<TAB>unit` lines.
   The runner appends `value` to `raw/<metric>.raw`.
3. After all samples, per-metric `*.summary.csv` is produced with
   `n,min,p50,p95,p99,max,mean,stdev`.

## Adapter contract

See [`lib/adapter.sh`](lib/adapter.sh). Exit code `78` from `snapshot` or
`restore` means "not supported" — the runner skips the workload rather than
failing the run.

## Adding a new hypervisor

1. Drop `adapters/<name>.sh` (or `<name>.ps1`) implementing all 8 verbs.
2. Add a `[hypervisor.<name>]` block to `config.toml` with `enabled = true`.
3. That's it — every workload runs against it automatically.

## Adding a new workload

1. Create `workloads/<key>.sh`. It must print TSV `metric\tvalue\tunit` lines
   on stdout. Use env vars from the runner (`BENCH_SSH_*`) and any extra
   params from your config block.
2. Add a `[workload.<key>]` block to `config.toml`.
3. Map the config key to your script in `run.sh::workload_script` and
   `workload_env` (if it needs parameters).
4. If the workload needs special lifecycle handling, add a case branch in
   `run.sh`'s main loop (orchestrator-owned vs self-driven vs per-sample).
