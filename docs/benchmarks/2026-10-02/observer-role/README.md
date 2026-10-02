# Single-team observer role verification

This is functional verification, without a performance or competitor comparison.
Policies now accept `operator` (the backward-compatible default) or `observer`.
The observer role caps even admin scope with an explicit inventory GET/HEAD
allowlist. Resource scopes still apply. Tenant ownership and sharing remain absent.

Windows and Linux each passed 42 library tests, 24 real HTTP integration tests
and strict Clippy. The HTTP test checks observer access against a populated
store, refused capability reads/mutations, unchanged guest count, and continued
legacy operator access. Parser tests reject unknown and null roles.

The clean Linux release control plane was built from the previously verified
audit-batching source with only the two archived cluster overlays. The three
provisional boot edits in the working tree were excluded. Binary and source
hashes are recorded in `build-context.json` and the raw KVM report.

The owned WSL KVM run passed all 23 cases through verified API TLS, node mTLS,
Redis and a real 1-vCPU/1024-MiB guest. Its observer key had admin scope. It
listed the guest without an envd token, refused detail, volume and upload
capabilities, refused create/exec/delete, and left the guest intact without the
attempted file mutation. The tunnel check returned 403. Existing operator CLI,
SSH, alias, lifecycle and tunnel checks passed. Cleanup left zero sandboxes and
stopped all 22 owned processes. All 222 admission/completion records verified,
with no unfinished admission or fixture credentials in logs.

Run `python verify.py` here to verify manifest hashes and raw audit/report
consistency. The synthetic audit key is public fixture data (0x42 repeated 32
times). Operator credentials and private fixture keys are omitted.

These checks do not establish tenant isolation, per-resource ownership,
immediate bearer revocation or management of membership and sharing. Role
replacement uses the existing atomic policy replacement path; this KVM run
tests startup role enforcement, rather than a role-changing SIGHUP.
