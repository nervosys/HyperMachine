# Matched concurrent private versus standard UDP

Both sets of established tunnels stay open throughout a **private, standard, standard, private** comparison. Each block uses 32 synchronized workers and 100 exact round trips per worker, cycling 64, 1,280 and 65,507-byte payloads with worker/sequence identities. Each peer warms with 64 and 65,507-byte payloads before scored blocks. No timed retries occur. The same target KVM guest, release daemon, explicit echo buffer, workload and host are used. Private receiving admission remains saturated with 125 additional held routes alongside existing fixture routes; all 32 standard routes remain live.

| Block | Path | Exact datagrams | Echoed payload MiB/s |
|---|---|---:|---:|
| 1 | private | 3,200/3,200 | 21.790 |
| 2 | standard | 3,200/3,200 | 22.209 |
| 3 | standard | 3,200/3,200 | 22.069 |
| 4 | private | 3,200/3,200 | 21.771 |

**12,800/12,800 matched scored datagrams**, 3,200 additional capacity-traffic datagrams, 128 regression benchmark operations and **55 KVM checks** pass, with full guest/process cleanup. Every matched block has zero deltas in guest UDP receive/send/checksum/memory errors and echo-socket drops. Byte totals and throughput are independently recomputed. Each block echoes 70,596,704 payload bytes; throughput counts echoed payload once per round trip and excludes framing/TLS overhead. Wall time includes worker start and barrier setup, but excludes tunnel creation, warmups and counter capture.

Private throughput is 21.771–21.790 MiB/s versus standard 22.069–22.209 MiB/s in this cohort. Both private blocks are lower; this identifies a remaining small steady-traffic gap rather than parity or a win. Two blocks per path are not a confidence interval or proof of sustained throughput. Host scheduling, Python client work, guest echo processing and transport all contribute. CPU/PSS attribution is absent. Production authorization is retained, and no source change is justified solely from this difference.

The owned echo fixture requests 8 MiB receive buffering (Linux actual 16 MiB) to avoid its independently observed burst loss; the exact image and zero-drop correction are archived in `../private-capacity-concurrent-kvm/`. The same buffered image is used for both paths here. Production receiver/guest agent/client are unchanged. Frozen input hashes, checker/driver, raw block/worker outcomes, journals, report and daemon/stdout logs are retained. `--private-capacity-comparison` requires `--private-receiving-capacity`; drivers require fresh output paths. Run `python3 verify-results.py` for full raw/statistics/status/source/cleanup verification. Manifest pins all payloads.

This is shared owned WSL host-to-target mTLS transport to guest loopback UDP, not source-guest Ethernet, independent physical hosts, managed sandbox comparison or CPU/memory efficiency. Mixed TCP traffic, concurrent admission, injected cancellation and competitor throughput remain incomplete or unmeasured. Protected root core source was not read or built.
