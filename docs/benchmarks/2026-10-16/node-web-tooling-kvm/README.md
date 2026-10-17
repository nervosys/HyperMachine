# V8's Web Tooling Benchmark in a 4 vCPU, 8 GiB sandbox, on KVM (2026-10-16)

**What this is:** the Node.js benchmark closest to the parity table's
"Node.js build-and-test loop" row that can be run here: V8's
[Web Tooling Benchmark](https://github.com/v8/web-tooling-benchmark), in a
sandbox of 4 vCPUs and 8 GiB made from [the tools image](../../../TOOLS_IMAGE.md),
with the Internet, which is the shape the public
[hpc-sandbox-benchmarks](https://github.com/AnicetNgrt/hpc-sandbox-benchmarks)
leaderboard uses (4 vCPU, 8 GiB). That leaderboard's "Node.js web tooling"
row is in runs/s, higher is better. It runs Phoronix Test Suite's profile of
the same benchmark; this run is **not** that invocation.

## Method

[`tools/bench-node-tooling-kvm.py`](../../../../tools/bench-node-tooling-kvm.py)
starts an owned `hv2-sandboxd` with `--network --egress-default allow` and
the tools image as its template, creates a sandbox with `cpuCount` 4 and
`memoryMB` 8192, and inside it: `git clone` of the benchmark (commit
`4a12828c6a1eed02a70c011bd080445dd319a05f`), `npm install`, `npm run build`,
and then `node dist/cli.js` six times, each of which prints a geometric mean
in runs/s. Two replicates, each its own sandbox. Node.js is Ubuntu 24.04's
18.19.1 and npm 9.2.0. The guest saw 4 processors and 8,142,360 kB.

## Result

12 runs, in two sandboxes: **median 12.56 runs/s, from 9.33 to 15.26**
(mean 12.36).

| | replicate 1 | replicate 2 |
|---|---|---|
| runs/s, in order | 9.78, 11.19, 15.23, 13.86, 10.33, 13.53 | 9.33, 11.59, 14.25, 14.02, 15.26, 9.97 |
| median | 12.36 | 12.81 |
| `git clone` / `npm install` / `npm run build` | 2.4 s / 67.0 s / 29.8 s | 2.5 s / 57.8 s / 14.5 s |

A probe run of two beforehand gave 15.43 and 13.90 runs/s
(`runs/probe.json`; `npm install` 53.9 s, build 12.0 s). Every step exited 0.

**The host was not quiet.** The Windows host's CPU load, sampled every 30 s
during the first ten minutes, ran 42 to 100% (45, 62, 76, 100, 57, 94, 90,
55, 74, 79, 68, 42, 72, 70, 88, 83, 79, 76, 60, 63), from other sessions'
processes. The spread between runs, a factor of 1.6, is that, and a quiet
host would likely score higher. Do not read 12.56 as what this stack can do.
The host is also a WSL2 virtual machine, so the sandbox runs on nested
KVM.

## Against the leaderboard

As read from the leaderboard page on 2026-10-16 (its "Node.js web tooling"
row, medians in runs/s): Blaxel 19.8, Daytona (VM) 18.6, Modal (VM) 15.07,
Novita 13.52, **E2B 11.24**, Modal (gVisor) 9.56. This run's median 12.56
falls between Novita and E2B, but the hardware differs (an AMD Ryzen 9 9900X
here, unknown there), the host was loaded, the workload is the benchmark and
not PTS's profile, and the Node.js version differs. It is a rough placement,
not a comparison. **boat is not on that leaderboard.**

## What it does not support

- **The parity table's earlier figures** ("36 s, 27.7 runs a second", "113 s")
  are not on the leaderboard page that was read, and this benchmark is not
  that loop, so nothing here confirms or compares with them.
- A "build-and-test loop of 1,000 runs": not run.

## Not verified

- A quiet host; a second machine; the leaderboard's exact (PTS) invocation;
  the same Node.js version as the leaderboard's; larger or smaller sandboxes;
  a cold template (the template was already built).
