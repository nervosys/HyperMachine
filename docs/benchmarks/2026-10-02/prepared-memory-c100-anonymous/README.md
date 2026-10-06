# C100 anonymous residency reanalysis

This reuses the unchanged raw smaps from the 400 passing diagnostic restores in [the original C100 archive](../prepared-memory-c100/README.md). No guests were rerun. Input SHA and frozen parser/analyzer identify the exact source evidence.

| Guest-sized mapping metric, median MiB | HyperMachine | Firecracker |
|---|---:|---:|
| PSS | 244.675 | 221.605 |
| Private dirty | 216.197 | 203.510 |
| Anonymous | 216.170 | 203.510 |
| Private clean | 0.010 | 0.006 |

The anonymous residency difference is 12.660 MiB. The C100 private-dirty difference is therefore mostly consistent with anonymous residency, rather than solely dirty file-cache classification. This is more specific than the earlier dirty counter, but still does not identify whether host restore, guest execution or device work produced those pages. It does not account for the entire guest-sized PSS gap or total host-process memory gap. No runtime change or performance win is established.

The separately instrumented single-guest boundary study lacks anonymous counters and cannot transfer this C100 conclusion to its before-run observations. Its extended-counter build remains a separate study.
