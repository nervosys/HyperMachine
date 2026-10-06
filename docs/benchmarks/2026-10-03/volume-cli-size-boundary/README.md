# Volume CLI 4 GiB boundary verification

The shipped development CLI uploaded and downloaded exactly 4,294,967,296 bytes against an owned loopback HTTP fixture. Both directions matched the expected deterministic payload SHA-256. The binary hash stayed unchanged, all owned CLI processes exited, the fixture server joined, and temporary source/download files were removed.

| Direction | Verified bytes | Observed peak RSS/HWM |
|---|---:|---:|
| Upload | 4,294,967,296 | 15,716 KiB |
| Download | 4,294,967,296 | 17,908 KiB |

Linux /proc memory observations were sampled about every 2 ms. Final peaks between the last observation and process exit may be missed; these measurements exclude host page cache and fixture memory. The temporary payload files were on WSL /tmp (tmpfs). Timings in report.json include CLI startup and fixture processing, and download file sync; they are single-run loopback diagnostics rather than durable-storage performance benchmarks.

A separate shipped CLI integration regression creates a sparse source of 4 GiB plus one byte, requires prompt failure, checks the size-limit error, and verifies no network connection. All six volume CLI integration tests pass in the accepted isolated source checkout with offline locked dependencies. Existing download oversized-Content-Length refusal evidence remains in the volume-cli-tls archive.

Reproduce the maximum-size transfer with `python3 tools/check-volume-cli-streaming.py --cli /path/to/hm --output /fresh/output --mib 4096`. Allow over 8 GiB temporary storage and enough time for the bounded upload (120-second request timeout). Run `cargo test --offline --locked -p hm-cli --test volume_client` for the over-limit regression.

This verifies the client protocol boundary and exact bytes. The fixture is not the sandbox daemon or control-plane router; no 4 GiB volume-store transfer, TLS, network filesystem, failure during the maximum-sized transfer, or competitor performance was tested. No comparative win is claimed.
