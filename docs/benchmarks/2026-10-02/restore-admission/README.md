# Sixteen-slot prepared restore admission experiment

The isolated candidate bounds simultaneous prepared snapshot launch/readiness
to sixteen operations. The owned permit is acquired before VM construction and
released after successful `Restored` acknowledgement, with Drop on failure or
cancellation. Admission wait stays inside client readiness. Cold boot admission
and guest clock/RNG maintenance are unchanged. Only the isolated accepted
`main.rs` changes; the accepted runtime is **not replaced**.

All 3200 restores pass across two fresh C100 comparisons, each with two outer
baseline/candidate AB/BA pairs and two internal HM/Firecracker AB/BA pairs per
variant. Guest state, CPU/memory, state command and clock/RNG maintenance are
matched. All inputs and prepared sources remain unchanged; owned processes
stop and guest inventories are empty. These comparisons use no diagnostic
logging or allocator helper preload. The earlier four-restore candidate smoke
test is separate from the archive's paired attempt count.

| Cohort | HM variant | HM passed | Ready P50 ms | Ready P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---:|---:|---:|---:|---:|
| C100 | Accepted | 400/400 | 803.972 | 1040.757 | 337.119 | 275.208 |
| C100 | Sixteen slots | 400/400 | 723.175 | 1278.194 | 340.008 | 282.270 |
| C100 repeat | Accepted | 400/400 | 1407.854 | 5411.310 | 342.916 | 280.662 |
| C100 repeat | Sixteen slots | 400/400 | 941.948 | 1841.667 | 341.820 | 282.307 |

| Cohort/variant side | FC control passed | Ready P50 ms | Ready P99 ms |
|---|---:|---:|---:|
| C100 accepted side | 400/400 | 832.717 | 1082.093 |
| C100 candidate side | 400/400 | 876.139 | 1120.699 |
| Repeat accepted side | 400/400 | 906.353 | 2916.167 |
| Repeat candidate side | 400/400 | 959.826 | 1233.289 |

| Outer pair | HM mean reduction ms | HM P99 reduction ms | Held PSS reduction MiB | FC mean shift ms |
|---|---:|---:|---:|---:|
| C100 pair 0 | 160.121 | -54.085 | -9.977 | 60.997 |
| C100 pair 1 | 76.717 | -272.203 | 0.625 | 22.748 |
| Repeat pair 0 | 1327.065 | 3583.477 | 8.035 | -709.503 |
| Repeat pair 1 | 555.917 | 306.177 | -4.101 | 87.132 |

HM reductions are accepted minus candidate; positive favors the candidate.
FC shifts are candidate-side minus accepted-side; negative means that control
also became faster. Candidate paired means improve four of four times, but
tails improve only two of four and held PSS only two of four. In repeat pair 0,
the control also improves by 709.503 ms, indicating changed host conditions.
The first cohort's pooled candidate P99 worsens by 237.437 ms. This mixed tail
behavior does not justify default adoption or an across-the-board claim.
**This tested candidate is rejected from adoption**, while the broader idea
of bounded admission remains an unproved optimization hypothesis.

The host is shared WSL/nested KVM with uncontrolled background load. Fresh
nodes use newly prepared sources; the persistent HM HTTP daemon and fresh FC
VMM/Unix API are distinct paths, and device kernel arguments differ. Nested
batches do not provide independent hosts. PSS excludes kernel memory and
unmapped page cache. All failures would remain in the raw reports; latency is
conditional on successful attempts and memory on complete successful batches.
No managed competitor or fleet performance win is established.

## Failure recovery

A separate logging-enabled diagnostic temporarily withholds only the owned
named snapshot file. Two cycles each issue 32 simultaneous restores, twice
the admission bound. All 64 requests reach launch failure and return HTTP 500
with a launching error; both original snapshot files are restored byte-for-byte.
All 128 subsequent regular restores pass (64 HM, 64 FC), with zero retained
guests and all owned processes stopped. This verifies recovery after launch
errors without leaking the admission capacity. It is not a ranking cohort.
Queued-request cancellation, deadlines and fleet behavior remain untested;
source Drop behavior alone is not an integration proof of cancellation.

## Reproduction and verification

The candidate generator checks all 550 accepted source hashes before editing
an isolated copy, then checks every unchanged source afterward. The generated
candidate main hash is
`a353a2a96dd9b99ca764b17d3c839429fd40b0ffed7f2ef6652b84dbd583c411`;
the compiled candidate hash is
`5b9c35ca020cb4f47672edcdde454886125d2c730112ffea9f420d0e8e512d38`.
The accepted daemon hash remains
`2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f`.
The clean boot source bindings exclude the three provisional workspace core
files. Only main changes, and no startup allocator trim is included. ELF
compiler comments match between accepted and candidate binaries.

The initial release build finished in 3m19s. Its completion wrapper lost the
exit-status variable after compilation; a direct cached cargo build confirmed
exit zero before the candidate was copied or executed. The original failure
and verification are recorded in the build context.

```sh
python3 experiment-restore-admission.py /isolated/accepted/source \
  --source-context source-context.json --limit 16 --patch /new/path/candidate.patch
# In the isolated source tree:
cargo build --release --locked -p hv2-sandboxd
# Use binaries/images matching the manifest:
python3 bench-restore-admission.py --pairs 2 --concurrency 100 \
  --baseline /path/accepted --candidate /path/candidate \
  --candidate-context build-context.json --firecracker /path/firecracker \
  --kernel /path/kernel --initrd /path/initrd --output /new/path/report.json
python3 -O verify-restore-admission.py .
python3 -O check-restore-admission.py c100-report.json
python3 -O check-restore-admission.py --recovery recovery-report.json
```

The verifier recomputes analyses, exact generated source, compiler identity,
binary/image/driver hashes, matched restore contracts and cross-cohort control
entropy uniqueness. Fourteen malformed paired reports and twelve malformed
recovery reports are rejected on Linux and Windows with assertions disabled.
Windows coverage is offline analysis, not native KVM runtime behavior.
No binary or snapshot memory image is committed.
