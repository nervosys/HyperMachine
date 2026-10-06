# Configured initial issuance and TLS deployment

An explicitly configured renewal job can now include `initial_issuance` with
an owned HTTP-01 webroot, account email and `agree_tos: true`. A truly absent
lineage and absent saved renewal config select Certbot `certonly`; existing or
interrupted state uses renewal. Dangling lineage symlinks do not grant first-use
authority. New issuance uses only the configured hostname list. Its activation
may add a non-overlapping named group through the existing deployment transaction.
The durable scheduler commits success only after verified TLS activation.

Sixteen owned Pebble/Certbot/live TLS checks pass, including actual missing-lineage
HTTP-01 issuance, new named-group activation with unchanged default, and a non-due
retry that makes no new challenge request. The same run retains first-group hard
exit/recovery and ordinary rollback, expiry and refusal checks. Eight initial
policy tests and twenty-one existing renewal tests pass with Python -O on Linux.
Owned processes are reaped and the challenge thread stops.

The fixture starts with an absent dedicated Certbot account directory. It
registers exactly one fresh owned ACME account using the configured email and
explicit terms acceptance, then issues and deploys the certificate. The ordinary
fixture account is separate. Account/lineage keys remain in
temporary storage and are removed. No public CA is contacted. Automatic creation
of these jobs from authenticated domain claims, public DNS/CA behavior and KVM
traffic through this initial-issuance mode remain incomplete. Existing renewals
retain their prior mode unless explicitly configured. No performance win is claimed.
