# Real unbind with a pending discovered deployment

Seventeen outer owned ACME checks and sixteen KVM checks pass. An owned worker
wrapper causes activation failure after successful CA issuance, leaving an actual
durable retry-deployment job. Removing the real control-plane domain binding then
causes automatic discovery to refuse the cycle. Exact journal and TLS manifest
bytes, and the issued certificate hash, remain unchanged. Restoring the binding
with the fixture's DNS ownership proof allows activation of the retained issued
certificate. The normal guest stream and lifecycle checks subsequently pass.

This extends authenticated API HTTPS discovery, fresh-account initial issuance,
named-group provisioning and KVM guest TLS. An existing 16 MiB download completes
across certificate deployment. Owned processes are reaped with no cleanup errors.
Accepted control/daemon build contexts and exact source hashes are retained.
The report includes the owned fault-wrapper source and hash; production code has
no fault-injection switch. Private keys/accounts are removed from temporary storage.

This verifies disappearance of a real domain binding while deployment is pending.
Owner replacement and guest-port changes have policy tests but are not exercised
as live interruptions here. Certificate retirement, public DNS/CA operation and
fleet behavior remain incomplete. No managed-product performance win is claimed.
