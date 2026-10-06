# Explicit named certificate-group retirement

The deploy hook now supports `--retire-group` for an exact named group. It
requires an existing default certificate, removes only the selected entry and
preserves the default and unrelated entries. The atomic-manifest, owned-pidfd,
private journal, reload and rollback transaction verifies the operator-pinned
fallback leaf through new TLS handshakes, sending no application data. This is
pin verification rather than hostname/issuer verification of the fallback, which
may intentionally not cover the retired hostname. Immutable generations are retained.

Sixteen owned ACME/live TLS checks pass, including exact retirement, pinned
fallback activation, unchanged immutable-file inventory, idempotent retry and
subsequent reprovisioning. Eight provisioning and 21 renewal regression tests
pass under Python -O on Linux. Owned processes and the challenge thread stop.
No public CA is contacted and private account/key files are removed.

The exact hook, fixture and accepted control-plane inputs are bound to hashes.
An owned wrapper hard-exits with code 86 after retirement manifest publication,
before reload. The previous live leaf remains, the private journal survives,
and retry reconciles the manifest, verifies fallback activation and clears the
journal. No immutable generation is created or deleted by recovery. The report
embeds the exact fault-wrapper source and hash; production has no fault switch.
Existing issuance and renewal behavior remains checked. This is an explicit
operator operation: automatic retirement of completed discovered jobs, retirement
without a default, additional retirement crash boundaries and KVM verification remain
incomplete. Named-group retirement does not revoke certificates at the CA, delete
immutable key files, remove domain bindings or close established connections.
No performance or managed-product superiority is claimed.
