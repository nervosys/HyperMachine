# Initial certificate issuance through KVM guest traffic

Seventeen outer owned ACME checks and fifteen real KVM checks pass. The guest
fixture starts a fresh Certbot account and lineage, publishes an existing default
certificate without a named group, and runs the configured initial-issuance worker.
HTTP-01 issuance provisions the named group and fresh certificate-verified HTTPS
connections reach the actual guest. An existing 16 MiB TLS guest download completes
across deployment. Ownership-proof refusals, routing, failed reload preservation,
default activation/removal, restart and guest lifecycle checks also pass.

The accepted control plane, daemon, kernel and initrd hashes are retained with
their build contexts. All four Python tools are frozen and bound to report hashes.
Owned processes are reaped, sandbox inventory is empty and cleanup has no errors.
Private accounts and keys are removed from temporary storage; no public CA is used.

The first failed run discarded the child verifier report in its launcher. A
diagnostic rerun preserved the failure: its fallback test used the new app-only
certificate for a different hostname. The corrected test uses the original
certificate covering that fallback hostname and checks its actual leaf identity.
Both runs remain excluded; the diagnostic report records successful initial
issuance and guest streaming before the later fixture failure. Historical sources
for these excluded runs are not included; their hashes must not be treated as
bindings to the corrected tools.

This verifies configured initial issuance through owned KVM traffic, not automatic
discovery of jobs from domain claims, public DNS/CA operation, fleet availability
or competitor performance. Those parts of product parity remain incomplete.
