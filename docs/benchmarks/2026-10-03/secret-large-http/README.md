# Owned KVM large-body HTTPS timing fixture

The optional timing mode passes fifteen owned KVM checks with thirty-four
successful HTTPS requests, including three large-body warm-ups and twelve timed
large requests. Its daemon/kernel/client-image hashes exactly match the ordered
lookup archive, which identifies this debug verification build and its sources.
No runtime implementation or accepted performance input changed for this run.

Each timed raw body has 15,000 placeholder lines, totaling 1,035,000 input bytes.
A full 128-binding policy substitutes the token sorting last, yielding exactly
180,000 bytes. Every upstream body, rewritten header and Content-Length is
checked. Guest files are built using BusyBox yes/head and removed after timing.
Curl verifies owned TLS; every invocation opens a new connection. The metric is
guest curl time_total, covering the complete request/response path and TLS setup
while excluding the surrounding host API exec call. The owned Python HTTPS
server's work is included. All owned guests, daemon, listener and keys are cleaned
up. Private policies, tokens and raw HTTP/API responses are not archived.

Observed debug-build median: 65.035 ms;
range: 63.193–67.962 ms.
This is a single unpinned debug-build fixture, not a matched baseline comparison,
release performance claim, reliable tail percentile or managed competitor result.
Its purpose is to establish complete-path measurement and correctness before
building matched release variants. Component speedups cannot be assumed to
translate directly into this latency. Exact lifecycle scheduling and failed-pause
recovery remain unforced, as in the preceding fixture.

Reproduce: `python3 tools/check-secret-substitution-kvm.py --bindings 128
--timing-repetitions 12 --daemon BIN --kernel KERNEL --initrd HTTPS_CLIENT_IMAGE
--output NEW.json`. Linux KVM, OpenSSL, ip and the separate HTTPS client image
are required. Omit timing-repetitions for the normal correctness-only suite.
