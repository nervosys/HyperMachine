# Matched release-mode private transport optimization

The receiver optimization is now compared in **matching release profiles**, in baseline–candidate–candidate–baseline order. All **512/512 scored operations**, 32 warmups and **32 functional KVM checks per cohort** pass. Every cohort deletes all guests and reaps all fixture processes. The baseline and candidate source catalogs differ only in `forwards.rs`: the candidate runs the atomic membership snapshot and independent live-node reads concurrently while retaining both setup authorization checks and active-stream revocation.

| Release cohort | Payload | Median paired setup overhead ms |
|---|---:|---:|
| 0-baseline | 64 B | 0.641 |
| 0-baseline | 1 MiB | 0.661 |
| 1-candidate | 64 B | 0.294 |
| 1-candidate | 1 MiB | 0.382 |
| 2-candidate | 64 B | 0.335 |
| 2-candidate | 1 MiB | 0.438 |
| 3-baseline | 64 B | 0.435 |
| 3-baseline | 1 MiB | 0.644 |

Each value is the nearest-rank median of 32 private-minus-standard setup differences from alternating request pairs within the cohort. Both candidate medians are lower than both surrounding baseline medians for each payload. Baseline overhead is 0.44–0.64 ms versus candidate 0.29–0.34 ms at 64 B; at one MiB it is 0.64–0.66 versus 0.38–0.44 ms. This supports the local setup improvement in release mode. Private setup remains slower than standard; no competitor, P99, throughput, memory or across-the-board superiority is established.

The release baseline (`d40cf2805f4e22c4ede26a408e2aa547a472a0c6a82bcfb35cc32b558bc107d9`) was built using the same accepted isolated tree, changing only the receiver file to its archived baseline snapshot, and restoring candidate source in a finally block. The candidate (`304a2f4a207e17d10e4debf1dadefd538af0b82157ffe9ed55b73d902044e21e`) is the previously frozen release build. Both build commands/logs, source snapshots and the baseline build context are retained. All other runtime input hashes match, and all reports explicitly declare release mode. Current root/isolate source equality was rechecked after the build and runs.

Methodology: owned host mTLS client, same target KVM guest image/kernel, fresh TCP/TLS for each request, concurrency one, two warmups followed by 32 scored operations per path/payload, alternating paired first path. Setup spans TCP initiation through authenticated HTTP 101. Source guest Ethernet/DNS/router/connector behavior is covered by functional checks but excluded from timing. Sequential 16-KiB echo chunks carry the payload in both directions; reported throughput is payload MiB divided by full echo time, not isolated one-way line rate. Held PSS covers the whole target daemon; CPU includes guest-vCPU/background work with 10-ms tick quantization. Nodes share one WSL host; ABBA reduces monotonic drift bias without proving statistical significance or exclusive resources.

Run `python3 analyze.py` to reproduce `analysis.json` from the retained 544 rows and verify rank calculations, paired order, summary counts/CPU/throughput/PSS, matching inputs, release declarations and cleanup. Drivers preserve original owned absolute input paths; reproduction elsewhere requires equivalent frozen inputs and fresh output paths. Baseline build temporarily changes only the isolated receiver source and restores it; never build protected root core files. All 138 permitted current source pairs and accepted isolated core hashes were verified. Independent-host, high-concurrency and source-guest performance comparisons, private UDP and complete product parity remain unverified or incomplete.
