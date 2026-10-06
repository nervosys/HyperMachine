# Receipt-bound certificate retirement

Seventeen checks passed against the owned local Pebble HTTP-01 CA and accepted
control-plane binary. A mismatched retirement leaf receipt refused removal while
preserving manifest bytes, the served leaf and journal absence. A matching receipt
allowed exact-group retirement, recovery from a hard exit after publication,
idempotent retry and reprovisioning. Immutable generations remained present.
Owned service processes and the HTTP thread shut down successfully.

Nine group/receipt unit tests passed under Python optimization. The initial live
attempt terminated with a fixture-only TypeError from hashing an existing hex
fingerprint; the corrected final run above is the accepted evidence.

This verifies the explicit deployment hook. Automatic retirement scheduling,
retirement-specific KVM traffic and public CA deployment remain unverified.
