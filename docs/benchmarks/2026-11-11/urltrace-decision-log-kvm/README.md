# The node's decision log keeps its first entries, on KVM

Same check, node and image as `../urltrace-noshell-kvm/README.md` (a shell-less guest, the tracer
started by an `argv` exec), with one change in the node and one new case (14).

## What changed and why

`GET /sandboxes/{id}/network/decisions` is the host's independent record of what a possibly
compromised guest did on the network. The gateway kept the last 512 decisions and dropped the
oldest, so a guest could make hundreds of refused lookups and push its real activity out of the
record, and a reader could not tell. Now:

- The gateway keeps the **first 256** decisions ever recorded and the **most recent 256**, and counts
  what fell between. Each decision has a `seq` (`u64` from 1, in recording order, never reused).
  `crates/hv2-net/src/gateway/mod.rs`: `DecisionLog`, `DecisionSnapshot`; `decisions()` returns the
  kept entries in sequence order; `decision_log()`, `decisions_recorded()`, `decisions_dropped()` are new.
- The endpoint's JSON gains `seq` and `at` on each decision (`at`: milliseconds since the Unix epoch;
  the gateway's own time is already a wall-clock `SystemTime`, so no conversion base was needed) and
  `recorded` and `dropped` at the top level. Nothing was removed. The machines route
  (`/machines/{name}/network/decisions`) shares the function and answers the same way.
- `tools/urltrace.py`: `decision_log_incomplete` is now set on a hop when the node's `dropped` > 0
  (the old counters heuristic remains only for a node that reports no `dropped`); the summary prints
  `DECISION LOG INCOMPLETE` in capitals. `seq` and `at` reach `trace.json` inside every entry.

## What ran

- Daemon: Linux release build of the working tree, sha256
  `2c4ca97223808059fb468e583ced2679f755a5e41060553cb883017bf66992b3`.
- Kernel `bzImage` sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`; image
  `urltrace.cpio.gz` sha256 `e1e285cfdbddb5a0f318ebdbf3ef4540da51ead79a2bc327fe9042afd6860ec7`;
  tracer sha256 `2ad9bb1538874f4ceb8deac8dff6cf1c7628578686d96bad24d4daafc3906eee`.
- `git rev-parse --short HEAD`: `9c4ba552`, **plus uncommitted changes** (the files above, the check
  and the docs); the binaries came from that working tree, not from a commit.
- Three consecutive runs, `run1` to `run3`, each of 17 cases, started 21:52, 22:02 and 22:12 (host
  clock). Each daemon log has 20 lines `Detected hypervisor platform: Kvm` and none with `Tcg`
  (`grep -c`). Before the runs: `cargo fmt --check`, `clippy --all-targets -D warnings`,
  `cargo test -p hv2-net` (228 passed) and `cargo test -p hv2-sandboxd -- network decisions exec_`
  (20 passed) were clean.

## Result

All 16 earlier cases (1, 2, 3, 4, 5a, 5b, 5c, 5d, 7, 8, 9, 10, 11, 12, 13, 6) and the new one passed
in all three runs. Every single-hop case (via `only_host`) now also asserts `dropped` == 0,
`recorded` == the number of entries, an integer `seq` and `at` on each, and no
`decision_log_incomplete` flag.

| Case | run1 | run2 | run3 |
|---|---|---|---|
| 1 to 13 and 6, as in `urltrace-noshell-kvm` | pass | pass | pass |
| 14 a guest cannot push its first entries out of the node's record | pass | pass | pass |

## Case 14

One sandbox pinned to example.com (`allowOut` plus the deny-all). One tracer exec against
`https://example.com/` (allowed), then one tracer exec per `https://flood-<n>.example.org/` with
`--timeout 3` until the node reported more than 700 recorded. Identical in the three runs, apart
from the flood's seconds:

| | run1 | run2 | run3 |
|---|---|---|---|
| flood execs | 180 | 180 | 180 |
| flood seconds | 541.0 | 540.9 | 541.1 |
| recorded | 723 | 723 | 723 |
| dropped | 211 | 211 | 211 |
| kept | 512 | 512 | 512 |
| lowest `seq` kept | 1, 2, 3, 4, 5 | same | same |
| first `seq` of the tail | 468 | 468 | 468 |

The first run left three entries, `seq` 1 to 3: `dns example.com allow` twice and `tcp
example.com 104.20.23.154:443 allow` ("allowOut name, by this gateway's DNS answer"). After the
flood those are still `seq` 1 to 3, byte-identical to what was fetched after the first run. Each
flood exec made four refused lookups (3 + 180 x 4 = 723); at 3 s a lookup attempt the flood is
slow, which is why it took 9 minutes. The assertions also held: `dropped` > 0 and `recorded` ==
kept + dropped; `seq` strictly increasing; every `at` an integer within the host's wall-clock
window of the case (5 s of slack) and non-decreasing; and `urltrace.compare()` on the same data set
`decision_log_incomplete` with `dropped` 211.

## Not verified

- The audit events sent to the tracing sink (`hv2_net::egress::audit`) were not examined; they do
  not carry `seq` (it is assigned in the log).
- A flood faster than this one (here about 0.4 decisions a second); the unit tests cover 1000 and
  5000 records, not timing or contention.
- Whether 256 first entries is enough for a hop that legitimately makes many connections before the
  interesting one. Activity in the middle of a long flood is lost by design, and only flagged.
- The head kept in case 14 is three entries long, so "the first 256 survive" is shown by the unit
  tests, and by `kept` == 512 with the tail starting at 468, not by 256 live entries.
- `at` is the node's wall clock; nothing checks that clock. The machines route was not run live.
- The docs were not reread against a fresh node of another platform; only Linux/KVM under WSL2 ran.
