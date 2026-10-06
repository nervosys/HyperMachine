# Automatic claim discovery through ACME and KVM

Seventeen outer ACME checks and fifteen real KVM checks pass. Starting with an
empty worker job list and a default-only bundle, the worker reads authenticated,
certificate-verified control-plane HTTPS inventory under its journal lock. It
discovers the bound hostname through an explicit operator suffix policy, checks
ownership before issuance and activation, registers a fresh account, performs
HTTP-01 issuance and provisions the named TLS group. New trusted connections
reach the actual guest while an existing 16 MiB TLS download completes.

DNS-proof refusals, routing, reload refusal, fallback activation/removal, restart,
guest lifecycle and cleanup checks pass. Owned processes are reaped and cleanup
has no errors. The worker, planner, verifier and issuer fixture are frozen and
bound to input hashes. Accepted control/daemon build contexts are included.
Private accounts and keys are removed; no public CA is contacted.

The excluded startup failure used fixture argument slicing that left legacy TLS
flags alongside the bundle flag after adding API TLS. The corrected fixture
removes the explicit legacy flag pair while preserving API TLS. That failed report
and log are retained, with no passing claim or source binding to corrected tools.

Automatic domain-claim discovery and initial issuance are verified on this owned
single-domain setup. Certificate retirement, real ownership-change interruption,
public DNS/CA behavior, sustained/fleet scale and tenant self-service remain
incomplete. Checks are separate observations, not an atomic binding/TLS transaction.
No performance or managed-product superiority is established.
