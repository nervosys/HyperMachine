# Borrowed boot images: no per-boot copy of the kernel and initrd

Before this change, every KVM boot cloned the kernel payload and the initrd
into fresh `Vec`s (14.8 MB here) only to copy them again into guest memory.
`LoadedBoot::data_regions_borrowed` hands out `Cow::Borrowed` slices of the
images already held by the daemon. Only the generated boot parameters and
command line are allocated per boot. Multiboot keeps its owned preparation.

## What was measured

`faults.py BINARY 8` starts `hv2-sandboxd` with the same flags and inputs as
`tools/bench-local-engines.py`. It then creates, execs in and deletes eight
sandboxes one at a time (plus one uncounted warm-up). For each create it reads
the daemon's minor page faults from `/proc`, and at the end the daemon's RSS.
Both are counts, so they do not depend on host load. The order was interleaved
baseline, candidate, candidate, baseline. Raw rows are in `results.jsonl`.
Run 1 used an earlier revision of the script, without the RSS read.

| | baseline | candidate |
|---|---|---|
| minor faults per create, max | 28,631 – 28,688 | 25,035 – 25,059 |
| minor faults per create, min | 21,312 – 21,333 | 21,309 – 21,316 |
| daemon RSS after 8 creates (run 2) | 89,440 / 118,968 KiB | 60,608 / 60,596 KiB |

The fault floor is the same: the guest-memory writes dominate it. The worst
case per create falls by about 3,600 faults, and the daemon's resident size
after eight boots falls from 89–119 MB to 61 MB and stays put. The baseline's
RSS differs between its two runs, consistent with freed 14.8 MB buffers not
always returning to the OS; the candidate allocates none to free.

## What was not measured

Boot latency. The host stayed at 70–100 % CPU from unrelated builds for the
whole session, and no timing was taken under that load. This change claims no latency improvement.

## Inputs

`artifact-sha256.txt` pins both daemon binaries (built from this branch
without and with the change), the kernel and the initrd.
