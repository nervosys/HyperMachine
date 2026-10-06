# Scheduled certificate renewal

`tools/renew-tls-certificates.py` is a Linux operator worker for configured Certbot
lineages and HyperMachine certificate bundles. Its built-in scheduler checks due
jobs, runs Certbot, and verifies deployment through new TLS connections. It
reconciles deployment even when Certbot reports that renewal is not needed, so a
previously issued certificate awaiting activation can recover on a later run.
It creates no domain bindings. Jobs may explicitly authorize initial issuance
and named certificate-group deployment as described below.

First complete the [bundle setup](CUSTOM_DOMAINS.md#deploy-certificates-from-certbot).
Existing-lineage jobs require a certificate; initial-issuance jobs require an
existing default bundle certificate for pinned rollback.
Install the worker and `deploy-tls-certificate.py` together. Run as the manifest
owner with Python 3, OpenSSL and Certbot. Keep configuration files owned by that
user and not writable by others. Create a private journal directory with mode
`0700`; the worker creates a `0600` journal and exclusive sibling lock.

An example configuration for one existing lineage:

```json
{
  "certbot": "/opt/certbot/bin/certbot",
  "config_dir": "/etc/letsencrypt",
  "work_dir": "/var/lib/letsencrypt",
  "logs_dir": "/var/log/letsencrypt",
  "interval_seconds": 43200,
  "retry_seconds": 300,
  "command_timeout_seconds": 180,
  "jobs": [
    {
      "id": "app",
      "cert_name": "app",
      "lineage": "/etc/letsencrypt/live/app",
      "manifest": "/etc/hypermachine/tls-bundle.json",
      "generations": "/etc/hypermachine/certificate-generations",
      "control_plane": "/opt/hypermachine/bin/hv2-control-plane",
      "domains": ["app.example.com"],
      "port": 443
    }
  ]
}
```

```sh
python3 /opt/hypermachine/tools/renew-tls-certificates.py \
  --config /etc/hypermachine/renewals.json \
  --state /var/lib/hypermachine/tls-renewal/state.json --watch
```

Without `--watch`, the command performs one due cycle and exits nonzero if an
attempt fails. `--force-renewal` explicitly forces issuance for that single cycle;
it is refused with `--watch`. Normal scheduling delegates the certificate's actual
renewal eligibility to Certbot, including its configured authenticator. Keep the
HTTP-01 webroot service or DNS authenticator available independently of the worker.
The worker disables directory hooks and overrides the saved deploy hook with an
empty value; its own activation step replaces that hook. Stored per-lineage pre/post
hooks and authenticator hooks remain Certbot configuration. Use a dedicated
operator configuration directory if your other certificates depend on directory
hooks. Certbot writes its ordinary diagnostic logs to `logs_dir`.

The default check interval is twelve hours and failed attempts retry after five
minutes. Jobs run serially, with separate success/failure results. Up to 64 jobs
are supported; each targets exactly one existing hostname group, with up to 128
exact names. Missed intervals coalesce into one current check. A backward wall-clock
change triggers a fresh check instead of waiting for an old future deadline.
The journal is limited to 1 MiB and configuration to 1 MiB. Subprocesses have a
bounded timeout, and stopping the worker terminates an in-flight command's owned
process group. The scheduler releases its journal lock on exit.

The worker finds exactly one process owned by its user whose executable inode and
absolute `--tls-bundle-file` argument match the job. This discovers a restarted
control plane with a new PID. If several instances share that manifest, configure
an owned, bounded regular `pid_file` containing the selected PID. The deploy hook
still pins and checks the process before signaling it. Optional `connect_address`
selects a TLS endpoint IP, defaulting to loopback; hostname validation still uses
the configured names. Per-job `ca_file` supplies private TLS trust. Global `server`
can override the ACME directory with an HTTPS URL without credentials, and
`acme_ca_file` supplies private ACME transport trust. Neither disables verification.

A durable `renewing` record precedes issuance. A `deploying` record precedes
activation, and success is committed only after a verified TLS receipt. If issuance
succeeds but activation fails, the pending record survives and the next retry
activates the issued lineage without requesting another certificate. Restart during
an uncertain issuance rechecks Certbot. Restart during deployment resumes activation.
Changing or removing a pending job requires reconciliation; completed jobs with
changed configuration receive an immediate new check. Configuration changes to a
running worker take effect after restart.

The deploy hook also keeps a private, checksummed `MANIFEST.deploy.pending` journal
before publication. It contains the prior/proposed manifests and public leaf hashes,
not private keys. On retry, only those two recorded manifests and leaf identities
may be reconciled. This handles termination after publication or after activation
before acknowledgment. Corrupt records, altered certificate generations and
unrelated manifest edits are refused without overwriting them. The journal is
limited to 3 MiB and is removed after verified completion or rollback. All manifest
writers must honor `MANIFEST.deploy.lock`; keep its directory owned and not writable
by others. Do not remove referenced certificate generations during recovery.

The [owned worker evidence](benchmarks/2026-10-03/tls-renewal-worker/README.md) records
21 scheduling/input/process tests, 21 live issuer/worker checks, and 14 real KVM
checks. It exercises continuous scheduling, overlapping-worker refusal, a restarted
control plane, hard termination at both deployment boundaries, and real HTTP-01
renewal while an existing 16 MiB guest download completes. Public CA operation,
automatic issuance after a domain claim, fleet-wide orchestration and generation
garbage collection remain outside this verified worker scope. No performance win
is inferred from its functional checks.

## Initial issuance for configured jobs

Add this optional field to a job to authorize first issuance:

```json
"initial_issuance": {
  "webroot": "/var/lib/hypermachine/acme-webroot",
  "email": "operator@example.com",
  "agree_tos": true
}
```

The operator must configure the HTTP-01 service and DNS to serve this webroot.
Its directory must be owned by the worker user and not writable by others.
The email and explicit terms acceptance authorize Certbot account operations.
The worker issues only when both the lineage and saved renewal configuration
are absent; broken symlinks or existing state go through ordinary renewal.
After issuance it can provision a non-overlapping named bundle group, retaining
the default certificate. Existing jobs gain no initial-issuance authority.

[Owned initial-issuance evidence](benchmarks/2026-10-03/initial-tls-worker/README.md)
verifies a missing lineage using an existing owned ACME account and live TLS.
Public DNS/CA operation and automatic job discovery from domain claims remain
unverified. Interrupted issuance may
require another CA order when Certbot has not persisted any usable lineage;
interrupted activation follows the existing deployment-retry journal.

The [fresh-account worker check](benchmarks/2026-10-03/fresh-account-tls-worker/README.md)
passes sixteen owned ACME/live TLS checks. A separate, initially absent Certbot
account directory gains exactly one registration before initial issuance and
verified named-group activation; a non-due retry skips issuance. Fresh account
registration is now verified against the owned CA. Public DNS/CA operation,
domain-claim job discovery remain incomplete.

[Initial issuance through KVM guest traffic](benchmarks/2026-10-03/initial-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. A fresh account
and lineage issue a certificate and provision its named group while an existing
16 MiB guest download completes. Routing, reload refusal, restart and guest
lifecycle checks pass with clean teardown. Configured initial issuance is KVM
verified; automatic domain-claim job discovery and public DNS/CA operation remain
incomplete. Failed fixture runs are retained and excluded.

## Domain claim discovery component

`tools/domain-certificate-jobs.py` provides the pure planning component for a
future discovery worker. Given authenticated control-plane binding records,
explicit operator hostname suffixes and an initial-issuance job template, it
produces deterministic, single-hostname jobs with hashed certificate identities.
It refuses duplicate/conflicting claims, unsafe binding records, capacity overflow
and missing initial-issuance authority. Suffix matching respects DNS label boundaries.
Six policy tests pass under Python -O on Linux.

This component performs no API polling or certificate issuance. Callers must
provide authoritative authenticated records and handle ownership changes,
revalidation, pending-job reconciliation and certificate retirement before it
can support automatic domain-claim provisioning. The control plane currently
exposes per-sandbox bindings; the discovery adapter remains incomplete.

The discovery component now includes `HTTPSReader` for certificate-verified,
API-key-authenticated control-plane reads. It refuses redirects and origin
credentials, restricts request paths, disables inherited proxies, rejects
duplicate JSON keys and limits response bodies to 1 MiB. Twelve tests pass,
including real owned HTTPS with trusted-CA success, untrusted-CA refusal and
wrong-key refusal. Its five-second socket timeout is per operation; system DNS
and slow responses still require a whole-cycle deadline in a supervisor before
unattended deployment. Worker integration remains incomplete.

`bounded_inventory` now supervises the entire collection in an owned Linux
child, including DNS resolution. It enforces a configurable collection timeout,
limits the returned inventory to 1 MiB, redacts child failure details and reaps
the child. Termination can add up to approximately three seconds of cleanup
after the collection deadline. Fourteen tests pass, including an actually stalled
child, verified timeout cleanup, successful collection and error redaction.
Use this supervised entry point rather than calling `HTTPSReader` directly in
an unattended worker. Ownership revalidation and worker integration remain open.

## Guard discovered jobs against ownership changes

The planner can attach `domain_claim` metadata when given a trusted HTTPS
control-plane origin. The record contains the hostname, sandbox ID and guest
port, plus an optional absolute private CA path. The worker reads its API key
from `HV2_DOMAIN_DISCOVERY_API_KEY`; credentials are not stored in job JSON.
It collects bounded authenticated inventory before Certbot and again before
TLS activation, refusing a missing claim, different owner or changed port.
Existing manually configured jobs without claim metadata retain their prior mode.

Sixteen discovery tests, 21 renewal tests and eight initial-issuance tests pass
under Python -O. Stale claims prevent issuer and activation commands. These
checks are separate observations, not an atomic transaction with binding changes.
The automatic discovery loop, pending-job ownership reconciliation, retirement
and real control-plane-to-guard integration verification remain incomplete.

The discovery planner now reconciles a proposed worker configuration against
the durable journal. It preserves operator jobs, admits eligible claimed domains
and marks disappeared completed jobs for renewal-schedule retirement. A pending
job cannot disappear or change its fingerprint, including owner or guest-port
changes; the planner refuses the replacement without modifying the journal.
Eighteen discovery tests pass under Python -O. This is a read-only reconciliation
plan: it does not publish configuration, remove certificates or run an automatic
discovery loop. A verified certificate-retirement transaction and integration
with the worker lock and persistence boundary remain incomplete.

## Run discovery in the worker

Pass `--discovery-config /etc/hypermachine/domain-discovery.json` alongside the
normal `--config` and `--state` arguments. This optional owned JSON file contains
`origin`, `allowed_suffixes` and `template`, plus optional `ca_file`. The origin
must be a trusted HTTPS control-plane API. The template supplies the ordinary
job's manifest, generation directory, control-plane binary, TLS endpoint and
explicit `initial_issuance` settings; identity, domains and lineage are generated.
Set `HV2_DOMAIN_DISCOVERY_API_KEY` in the worker environment, not in JSON.

With discovery enabled, the base worker configuration may use an empty `jobs`
list. Each cycle discovers and reconciles under the existing journal lock,
then executes eligible jobs through the normal durable issuance/deployment path.
A changed or disappeared pending job refuses the cycle without losing its state.
Completed missing claims are removed from renewal scheduling; certificates are
retained until a separate retirement transaction is implemented. Failed discovery
exits without issuing certificates. In watch mode, restart supervision is needed
for that failure. Existing static jobs and mode without discovery are retained.

The actual CLI passes an owned HTTPS empty-inventory test and creates an idle
journal without persisting the API credential. Eighteen discovery tests, 21 renewal
and eight initial-issuance tests pass. Nonempty automatic discovery through a
real control plane into ACME/KVM issuance, retirement and fleet behavior remain
unverified; the earlier KVM evidence used explicitly configured jobs.

[Automatic claim discovery through ACME and KVM](benchmarks/2026-10-03/discovery-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. An empty job list
is populated from authenticated, certificate-verified API inventory under operator
suffix policy; guarded initial issuance provisions the named TLS group while an
existing 16 MiB guest download completes. This supersedes earlier statements
that domain-claim job discovery was unverified. Certificate retirement, real
ownership-change interruption, public DNS/CA operation and fleet scale remain
incomplete. No managed-product performance win is established.

[Real pending-job unbind verification](benchmarks/2026-10-03/discovery-unbind-kvm/README.md)
passes seventeen outer ACME and sixteen KVM checks. An actual issued-but-not-yet
activated job survives a real unbind: discovery refuses without changing its
journal, manifest or certificate. Restoring the proof-authorized binding activates
the retained certificate. Live owner replacement and port-change interruptions,
certificate retirement and public DNS/CA operation remain incomplete.

[Live pending-job ownership and port changes](benchmarks/2026-10-03/discovery-ownership-kvm/README.md)
pass seventeen outer ACME and seventeen KVM checks. Real unbind, replacement
owner and guest-port changes refuse a pending job without altering its journal,
manifest or issued certificate; restoring the original binding recovers deployment.
Replacement guests are cleaned up. This supersedes earlier statements that live
owner/port interruption checks were unverified. Certificate retirement, public
DNS/CA behavior and fleet operation remain incomplete.

[Explicit named TLS group retirement](benchmarks/2026-10-03/tls-retirement/README.md)
now passes fifteen owned ACME/live TLS checks plus eight provisioning and
21 renewal tests. `deploy-tls-certificate.py --retire-group` removes an exact
entry, preserves the existing default and immutable files, verifies its pinned
fallback leaf, retries idempotently and permits reprovisioning. The existing
manifest/lineage/generation/process arguments remain required; lineage contents
are unused in retirement mode. Provision and retirement flags are exclusive.
Automatic completed-job retirement, no-default bundles and retirement-specific
crash/KVM verification remain incomplete. This operation sends no application
data during fallback pin checks and does not revoke CA certificates or remove
bindings, old files or established connections.

[Retirement publication crash recovery](benchmarks/2026-10-03/tls-retirement-crash/README.md)
passes sixteen owned ACME/live TLS checks. A hard exit after retirement manifest
publication leaves the previous live leaf and a durable journal; retry reconciles
the manifest, verifies fallback activation and clears the journal without changing
immutable-file inventory. Automatic retirement, additional crash boundaries,
no-default bundles and retirement-specific KVM traffic remain incomplete.

Guarded jobs now retain a `managed_job` identity in the private durable journal
before commands execute. It contains the validated deployment parameters and
claim identity, copied independently of the active configuration. Existing journal
records without this optional field remain readable. The stored job is validated
with the strict job schema on restart. Twenty discovery/journal tests, 21 renewal
and eight initial-issuance tests pass. This supplies the deployment identity needed
for future retirement of completed jobs; automatic retirement and migration of
legacy completed records lacking that identity remain incomplete.

Retirement eligibility now requires an absent hostname claim, an unchanged worker
and deployment target, the original claim authority, and a stored verified leaf
receipt. Discovery refuses to discard a completed managed job until certificate
retirement is verified, retaining its journal unchanged. Legacy managed records
without a deployment identity require operator reconciliation. These checks do
not execute automatic retirement; that transaction remains incomplete. The 21
discovery tests include unchanged-journal verification on this refusal; all 21
renewal and eight initial-issuance tests also pass under Python optimization.

The explicit removal hook accepts `--expected-retirement-leaf SHA256` to bind
retirement to the previously verified certificate receipt. An existing named
group must match that pin, including the original manifest in a pending recovery
journal; a later operator certificate replacement is refused before mutation.
Already removed groups remain idempotent. The pin is accepted only in retirement
mode. [Receipt-bound retirement evidence](benchmarks/2026-10-03/tls-retirement-receipt/README.md)
records 17 owned ACME/live TLS checks and nine unit tests, including wrong-receipt
refusal and receipt-bound publication-crash recovery. The automatic worker has
not yet been wired to execute these retirement transactions.

[Recovery receipt isolation](benchmarks/2026-10-03/tls-retirement-recovery-pin/README.md)
extends the owned fixture to 18 passing live checks. After retirement publication
and hard exit, a wrong receipt preserves exact manifest/journal bytes, the old
served leaf and immutable-file inventory. Correct-pin recovery then verifies the
fallback and clears the journal. The deployment hook binary source is unchanged.
Automatic retirement wiring awaits explicit approval after automatic review
rejected that service-changing action; the existing worker remains unchanged.
