# Volume CLI streaming memory verification

The shipped development CLI transferred 64 MiB and 256 MiB in each direction against an owned loopback HTTP fixture. Upload and download content SHA-256 matched deterministic expected bytes. Each CLI exited successfully, the fixture server joined, temporary source/download files were removed, and no download staging file remained. CLI binary hashes were identical before and after each run.

| Transfer size | Upload observed peak RSS/HWM | Download observed peak RSS/HWM |
|---|---:|---:|
| 64 MiB | 14,152 KiB | 15,672 KiB |
| 256 MiB | 13,460 KiB | 15,676 KiB |

Linux /proc status was sampled about every 2 ms while each CLI process was live. VmHWM preserves earlier peaks while observable, but a final peak between the last sample and exit can be missed. This is process memory, not host page cache or fixture-server memory. Four independent CLI processes were measured; this is evidence of bounded streaming for these sizes, not a universal memory guarantee.

Reproduce on Linux with `python3 tools/check-volume-cli-streaming.py --cli /path/to/hm --output /fresh/output --mib 64`, then repeat with 256 and another fresh output. The fixture streams fixed 128 KiB blocks and bounds CLI execution and server socket operations. Only volume bearer authentication is sent; platform API keys are omitted.

Raw reports retain wall-clock timing including CLI startup, request/response handling, and local download file sync. Upload fixture hashing and Python HTTP reads also contribute. These are single-run diagnostic times in a development build on loopback, not storage service benchmarks. No actual sandbox, control-plane routing, TLS, shared/network filesystem, 4 GiB boundary transfer, or competitor was exercised. No comparative performance win is claimed.
