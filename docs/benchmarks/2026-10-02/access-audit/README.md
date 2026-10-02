# Durable protected-API audit verification

The shipped control plane now supports opt-in synced, HMAC-chained admission and
completion records through `HV2_ACCESS_AUDIT` and `HV2_ACCESS_AUDIT_KEY_FILE`.
See [operator configuration and limits](../../../ACCESS_AUDIT.md). Admission is
durable before handler dispatch; completion is durable before returning its
response. A storage failure stops subsequent protected requests, while an
already-dispatched operation can have an uncertain committed outcome. This is
protected API access history, not tenant or guest activity auditing.

| Verification | Result |
| --- | --- |
| Windows core audit tests | 13 passed |
| Windows cluster library / real HTTP tests | 40 / 23 passed |
| Linux core audit tests | 13 passed |
| Linux cluster library / real HTTP tests | 40 / 23 passed |
| Strict cluster Clippy, both platforms | Passed |
| Final shipped-process fixture | 11 checks passed; 20 independently verified records |
| Final KVM/TLS fixture | 22 checks passed; 204 independently verified records |
| Final KVM cleanup | Zero guests remaining; all 22 owned processes stopped |

The process fixture rejects partial configuration, malformed keys, another
writer, a wrong-key restart, a partial tail and an edited record. Two restarts
preserve one chain. An owned process's file-size limit injects an admission
write failure: two mutations return 503 before dispatch, the process stays alive,
health remains available and the log is unchanged. The fixture removes its
temporary API-key policy and stops all its processes. Audit keys are synthetic
test data (`42` repeated 32 times); no operator or production key is archived.

Real-HTTP tests separately inject an audit write failure before dispatch and
after a mutating handler, verify the handler count, then verify later requests
are stopped. Twenty concurrent failed appends produce only one sink write
attempt. Record privacy checks cover credentials, unknown keys, bodies, queries
and path values. Core tests cover exclusive writers, verified restart, partial
and oversized records, and fractional timing MAC round trips.

The first HTTP restart run exposed a MAC failure caused by fractional JSON
numbers failing an exact parse round trip. `windows-float-roundtrip-failure.log`
is retained. Exact float parsing is now enabled for the shared audit chain and
a regression covers more than 1000 fractional timings. The protected access
schema itself records integer microseconds. Initial strict-lint findings and
their corrected results are retained too.

The first process run used a pre-lint-cleanup binary; `native-first-*` and
`build-context-first.json` retain those results. The final binary passed the
same fixture as `native-final-*`. The independent verifier was then hardened
to keep checks active under optimized Python and reject duplicate JSON fields
and boolean sequence numbers; it also rejects a changed MAC and partial tail.
`verify-access-audit-first.py` retains the earlier verifier. The final verifier
passed the process fixture (`native-verified-*`) and a second complete KVM run
(`kvm-verified*`). Earlier passing KVM evidence (`kvm*`) is retained separately.
The two KVM runs each passed all 22 checks and verified 204 records.

KVM checks cover named creation, duplicate refusal, node-side atomic completion
and descriptor recovery, authenticated OpenSSH and binary TCP transport,
untrusted key/certificate rejection, a 35-second idle hold, pause/resume, fork
ownership and deletion. The audit verifies API outcomes including 201, 401,
403, 409 and 101. Upgrade completion refers to the handshake, not subsequent
stream bytes. Both runs have zero unmatched admissions; a crash can legitimately
leave an unmatched admission in production.

The isolated build uses the verified registration-mutations source with the
scoped compiled overlays in `build-context.json`. The three provisional boot
files are excluded. The daemon, CLI, kernel and SSH guest image are unchanged.
The final runtime identities were rechecked after KVM execution. This is local
WSL nested KVM functional evidence, with no latency, throughput or competitor
win claim. Automatic retention/rotation, remote collection, tenant roles,
guest/proxy activity auditing and per-resource attribution remain incomplete.

`manifest.json` hashes every archived file except itself. Run `python verify.py`
to check hashes, source provenance, final process/KVM outcomes, and independently
recompute both final audit chains.
