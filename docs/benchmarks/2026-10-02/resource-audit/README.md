# Opt-in sandbox target audit references

The control plane now supports `HV2_ACCESS_AUDIT_RESOURCES=true` alongside its
existing durable audit configuration. Admission and completion events for
protected sandbox-ID routes carry a domain-separated HMAC-SHA256 reference.
References use decoded IDs and remain stable under the same audit key. Raw IDs,
queries, bodies, checkpoint names and credentials remain excluded. Default and
explicitly disabled modes retain the original fields.

This identifies the requested target, including denied and nonexistent targets.
It does not establish existence or successful mutation, and does not attribute
collection/create results, aliases, templates, volumes, proxy streams or guest
activity. Empty, undecodable and over-256-byte IDs have no reference. The key is
zeroized on drop and redacted from debug output. Operators holding that key can
compute references; this is correlation with controlled disclosure, not anonymity
from those operators. See [operator configuration](../../../ACCESS_AUDIT.md).

## Verification

The isolated clean native source passed 43 library and 25 HTTP tests. Strict
Linux Clippy passed. Its release control-plane SHA-256 is recorded in
`build-context.json`; the sandbox daemon remains the accepted `b7d2aba...` build.
The three provisional boot changes in the worktree were excluded from both.

Native process checks passed under Python `-O`: three modes, two starts each,
36 requests and 72 independently verified records. They verify absent default
fields, decoded path equivalence, key-based lookup, restart correlation, denied
requests, distinct IDs, oversized ID omission, raw-value privacy and process
cleanup. Four invalid/unaudited configurations were refused. Four records with
valid recomputed MACs but mismatched/malformed references were rejected by the
independent verifier. A Rust known-answer check cross-checks the HMAC formula
and tests key changes and length boundaries.

The first KVM fixture run supplied the readiness image, which lacks the required
TCP fixture. It failed with `guest fixture did not start`; its report/logs remain
in `kvm.*`. It cleaned up the guest and owned processes. The audit check on that
incomplete run also failed its required status coverage.
That run is not counted as passing evidence.

The corrected run used the established SSH/TCP guest image (`d520964...`), and
passed all 20 KVM/TLS checks: observer authorization, binary OpenSSH/TCP transfer,
credential/certificate rejection, idle protection, pause/resume, fork and deletion.
All 160 HMAC audit records verified, with zero unfinished admissions. There were
102 referenced events, including 88 correlated to the original guest; its raw ID
was absent. All 22 owned processes stopped and remaining sandbox inventory was
empty. This is functional evidence on shared WSL nested KVM, not a performance
comparison, universal reliability result or tenant-isolation claim.

The native `rejected-TRUE.log` is archived as `rejected-uppercase-true.log` to
avoid colliding with `rejected-true.log` on Windows.

Frozen coordinators, compiled overlays, build/test logs and SHA-256 manifest
make the evidence reviewable. No executable, operator credential or private key
is included. The keys are deliberately synthetic: native process checks use decimal byte 42
(hex 0x2a) repeated 32 times; the KVM fixture uses hex byte 0x42 repeated 32 times.

```sh
python3 -O tools/verify-resource-audit.py docs/benchmarks/2026-10-02/resource-audit
python3 -O tools/check-resource-audit.py --control-plane /path/to/hv2-control-plane --output /new/owned/fixture
```

The archive verifier checks file identities, recomputes record MACs and target
counts, verifies runtime/coordinator identity and lifecycle cleanup, and requires
the failed initial fixture report to remain present. No benchmark ranking changes.
