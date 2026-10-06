# Corrected prepared-resource C100 comparison

The accepted HyperMachine executable and Firecracker 1.17.0 use the same guest inputs, one CPU, 1,024 MiB, prepared file and live-process state, clock/RNG maintenance and guest command. Four counterbalanced pairs on the same shared WSL/KVM host pass all 800 restores. Resource GETs occur after every peer timer ends; cleanup and input hashes are verified.

| Metric | HyperMachine | Firecracker |
|---|---:|---:|
| Passed/planned | 400/400 | 400/400 |
| P50 readiness ms | 853.062 | 1018.022 |
| P95 readiness ms | 1420.131 | 1625.828 |
| P99 readiness ms | 1449.399 | 1645.604 |
| Median held PSS MiB | 333.953 | 272.305 |
| Median incremental PSS MiB | 253.772 | 272.305 |

Paired means favor HyperMachine in three of four pairs; paired P99 favors it in two of four. Held PSS is higher in all four pairs by 59.65–64.78 MiB. HyperMachine retains a persistent daemon; Firecracker starts a fresh VMM per restored child. Incremental memory subtracts the empty daemon and must not substitute for total held memory. These are local cache-warm measurements, not managed boxd or exe.dev results. No causal improvement over the previous coordinator or runtime change is established. Shared-host scheduling and benchmark Python-client work remain uncontrolled costs.

The frozen analyzer and checker validate matched contracts and reject 26 malformed reports. `phases.json` retains the aggregate create/restore and command phases, including the ten slowest successes.
