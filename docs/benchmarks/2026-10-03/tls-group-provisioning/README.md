# Explicit first hostname certificate group

The deploy hook now accepts `--provision-new-group` for a previously absent,
non-overlapping hostname group. It requires an operator-owned bundle with an
existing default certificate, pins that fallback on the selected running process,
and preserves it during publication. Normal deployment still refuses missing
groups. Partial overlap and bundle limits are refused before publication.

The new group uses the existing immutable-generation, atomic-manifest, pidfd
signal, fresh verified TLS and rollback transaction. Pending recovery can select
the original default leaf or the proposed named leaf when provisioning is
explicitly enabled. Existing renewals keep their existing-group restriction.

The retained owned Pebble/Certbot run passes thirteen live ACME/TLS checks,
including ordinary-mode refusal, explicit first-group publication with the same
initial leaf, unchanged default, subsequent renewed leaf activation, idempotent
retry, untrusted rollback, key mismatch, wrong process/manifest, serialization,
expired-leaf recovery and unavailable challenge-server refusal. Eight provisioning
unit tests and twenty-one existing renewal tests pass with Python -O on Linux.
Owned processes are reaped and the challenge HTTP thread stops. No private keys,
account credentials or public CA traffic are retained.

Two failed fixture launches are retained as excluded logs: an unsupported test
helper argument, then a test variable collision at final input-hash verification.
Neither is counted as a pass. The final run binds the exact frozen hook, fixture,
Certbot, Pebble and accepted control-plane hashes.

This closes the deploy-hook requirement for adding an initial hostname group;
it does not yet automate initial CA issuance after a domain claim. Public DNS,
public CA behavior, new-group hard-crash injection and KVM guest traffic in this
new provisioning mode remain unverified. Existing KVM evidence concerns renewal.
No performance or managed-product superiority is claimed.
