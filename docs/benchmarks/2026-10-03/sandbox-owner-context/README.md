# Trusted sandbox creator context

Native owner-only port APIs need trustworthy VM ownership before their first reservation. Existing API scopes authorize the configured team, and an API-key fingerprint identifies a credential rather than a stable creator. This implementation adds an optional operator-provisioned `principal_id` to API key policies, represented by a validated opaque OwnerId (1–128 ASCII letters/digits/dot/hyphen/underscore). Changing credential digests while retaining the principal ID preserves creator identity. Legacy policies and legacy/admin/anonymous creation remain ownerless. Labels must identify principals, never contain API credentials.

Authentication snapshots the current policy and inserts internal creator context after authorizing the request. It removes client `x-hv2-sandbox-owner` headers. V1/V2 creation forwards only the configured principal, over a cluster-authenticated node request, using a sensitive internal header. Configured principals without a nonempty cluster credential fail closed before node dispatch. The node validates one bounded owner header and requires clustered/authenticated configuration; its existing cluster-token middleware checks each request. Public body/metadata owner labels do not set trusted ownership. The owner is stored in the initial SandboxRecord, so registration, response-loss recovery and subsequent lifecycle records carry the same attribution rather than assigning it after a successful response. Malformed owner values fail deserialization. Internal owner IDs are omitted from public listed descriptors.

Forks inherit the source record's owner. They do not inherit its native port allocation. Pause/resume retain owner through record and snapshot persistence. This does not introduce a creator-only sandbox access policy: existing team capability scopes remain in force. Owner-only port API handlers, explicit legacy adoption/ownership transfer, organization identity provisioning and managed persistence guarantees remain incomplete. A shared cluster credential remains an administrative trust boundary.

Verification:

- Cluster library: 85 passed, one explicit owned-Redis test ignored. New checks validate principal parsing, key rotation, legacy record compatibility, owner JSON round trips and malformed record refusal.
- HTTP control integration: 29 passed. New owned listeners check forged client header/body/metadata context, V1/V2 stable identity after policy rotation, revoked-key refusal, observer denial, legacy/unattributed keys and no-cluster-token refusal without dispatch. Fixture tasks are aborted on drop and joined on success.
- Full daemon suite: 47 passed, including authenticated-cluster/duplicate/malformed owner-context refusal.
- Actual Redis/mTLS/KVM owned profile: seventeen checks. Valid mTLS peers with missing/wrong cluster credentials cannot create using forged owner context. Real creation stores the configured principal despite forged public header/body/metadata. Two forks inherit that principal while preserving the source's existing native allocation and having no child allocation. Actual pause/resume retain owner. Native TCP/UDP delivery, protocol updates and deletion cleanup still pass.
- Actual legacy profile: fourteen checks with the same binaries/image/kernel; the stored creator owner is explicitly absent. Both runs leave zero guests and reap all owned processes. Logs are checked for fixture API/cluster credentials before reporting success.

Host binaries are frozen development builds from accepted isolated `/var/tmp/hm-egress-log-mA2CCL`; reports verify their hashes, exact guest/kernel/CLI inputs and checker before/after. 133 permitted source/manifest/test files match root and isolate; source-context.json catalogs them. The accepted isolated core was revalidated; protected modified root core files were not read, built or staged. This is limited accepted-context verification, not whole-root validation. New owner records are proven across live Redis writes and pause/resume, not an owner-specific AOF/server-restart test or managed Redis durability guarantee. Earlier checker iterations are excluded; archived reports match the final checker. No new performance or competitor superiority claim follows.

Reproduce against a dual-service guest image, with a fresh output directory:

```sh
python3 tools/check-udp-cluster-kvm.py --daemon /path/hv2-sandboxd \
  --control-plane /path/hv2-control-plane --cli /path/hm \
  --kernel /path/bzImage --initrd /path/native-fixture.cpio.gz \
  --native-gateway /path/hv2-native-gateway --tls --mtls --owner-context \
  --output /fresh/results
```

Omit `--owner-context` for the legacy profile. Exact reports/logs, source snapshots, host build log/input hashes and source context are covered by manifest.json. No certificate private keys or policy file are archived.
