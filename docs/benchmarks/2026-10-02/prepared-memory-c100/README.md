# C100 prepared memory decomposition

All 400 diagnostic restores pass with the accepted HyperMachine executable and matched Firecracker inputs. Two counterbalanced pairs use concurrency 100, one CPU and 1,024 MiB per guest, prepared file/live-process state and clock/RNG maintenance. Resource checks follow all timed attempts. Owned guests and processes clean up; input hashes remain unchanged. Raw smaps are read after the held-memory observation; these extended-lifetime diagnostics are excluded from performance rankings.

| Engine | Total mapping PSS MiB | Guest-sized PSS MiB | Guest-sized private dirty MiB | Other mapping PSS MiB | Empty daemon PSS MiB |
|---|---:|---:|---:|---:|---:|
| hypermachine | 328.728 | 244.675 | 216.197 | 84.053 | 59.156 |
| firecracker | 271.277 | 221.605 | 203.510 | 49.672 | 0.000 |

Columns are independent medians and need not sum. Guest-sized file mappings are identified by size and path, not proven ownership. PSS excludes kernel memory and unmapped page cache. Sequential procfs reads are not atomic. Private dirty includes both host restore writes and guest activity.

Pair 0: HyperMachine minus Firecracker total mapping PSS is 56.339 MiB. Category differences are anonymous +23.062 MiB, file -0.285 MiB, guest_sized +23.304 MiB, heap +27.891 MiB, special +0.000 MiB, stack -17.633 MiB. These differences sum to the paired total.

Pair 1: HyperMachine minus Firecracker total mapping PSS is 58.562 MiB. Category differences are anonymous +25.742 MiB, file -0.285 MiB, guest_sized +22.836 MiB, heap +27.891 MiB, special +0.000 MiB, stack -17.621 MiB. These differences sum to the paired total.

The gap remains split between host allocations and guest-sized mappings. About 23 MiB of guest-sized PSS difference persists at C100, including about 13 MiB of private-dirty difference in aggregate medians. This does not establish why those pages differ or justify discarding restore writes. Earlier trim and buffer candidates retain their separately measured latency regressions; none is adopted. Raw mapping replay rejects seven malformed contracts with assertions disabled.
