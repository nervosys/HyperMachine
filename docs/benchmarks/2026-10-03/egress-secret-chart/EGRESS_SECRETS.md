# Operator-held egress secrets

The Linux sandbox daemon can replace opaque placeholders in outbound HTTPS
without storing the secret value in the guest. Policies select exact sandbox
IDs and exact authenticated upstream hostnames. Network access still requires
the sandbox's existing egress policy. Forks receive new IDs and do not inherit
the parent's operator scope.

Create a version-1 policy file. This example uses a synthetic placeholder and
value; generate real placeholders with `hms_` followed by 64 lowercase hex
characters, and preserve them when rotating their values.

```json
{
  "version": 1,
  "sandboxes": [
    {
      "sandbox_id": "sbx-example",
      "bindings": [
        {
          "placeholder": "hms_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "value": "example-secret-value",
          "hosts": ["api.example.com"]
        }
      ]
    }
  ]
}
```

Start the daemon with `--network --egress-secrets-file /absolute/path/policy.json`.
The immediate directory must be owned by the daemon's effective UID with mode
`0700`; the file must have the same owner, mode `0600`, one link, and be regular
and not a symlink. Keep the ancestor directories trusted. The policy is bounded
to 1 MiB and 64 sandbox scopes. These files are operator configuration and are
not guest API fields or snapshot contents.

The gateway verifies upstream TLS before sending a rewritten request. It uses
public Web PKI roots by default. For an internal endpoint, explicitly select
additional roots with `--egress-upstream-ca /absolute/path/roots.pem`; that file
uses the same private-file contract and accepts at most 1 MiB and 128 PEM
certificates. Added roots apply to intercepted connections on the entire node.
The guest interception CA is not automatically trusted upstream. Restart the
daemon to change upstream roots.

Substitution covers ordinary headers, Basic authorization, query components,
JSON string values, form components and raw bodies, including binary raw input.
Routing and hop-by-hop headers are excluded. Compressed bodies and trailers are
refused on this path; collected request bodies and rewritten outputs are bounded
to 1 MiB. JSON keys and numeric text are preserved. The guest must trust the
interception CA installed by the daemon; TLS verification stays enabled.

On a directly managed daemon, replace the policy with a new owned private file
and send SIGHUP. Valid reloads preserve existing placeholders and update retained
stores; removed IDs are revoked. Invalid policies leave active stores intact.
Reloading an added scope affects new intercepted connections. Requests already
in progress may finish using their current policy. Re-adding a removed ID creates
a new store, while handles retained from the removed store remain revoked.

## Sandbox Helm chart

Create existing Kubernetes Secrets in the release namespace from your private
policy and, optionally, upstream root files. The chart takes Secret names and
keys; secret values do not belong in Helm values.

```yaml
node:
  network: true
  egressSecrets:
    secretName: sandbox-egress-policy
    key: policy.json
  egressUpstreamCa:
    secretName: sandbox-upstream-roots
    key: roots.pem
```

Either setting can be used alone. Both require networking. An init container
using the node image copies only the selected projected keys into regular private
files in an in-memory volume. The daemon mounts that volume read-only; it does
not mount the projected Secret inputs. This avoids passing Kubernetes projection
symlinks to the private-file loader.

The copy happens once at pod startup. Restart node pods after changing either
Secret; SIGHUP does not copy updated Kubernetes projections into this mount.
Configure every node that may run or resume an ID with its intended exact-ID
scope. This is node-local operator configuration, not a managed organization
secret service. Node restart behavior still follows the chart's existing shared
snapshot-store configuration.

[Owned KVM evidence](benchmarks/2026-10-03/secret-binary-kvm/README.md) covers
body formats, upstream identity refusal, rotation, fork exclusion, pause/resume
and observed overlapping lifecycle calls. Chart tests render configuration and
execute its copy script locally; an actual Kubernetes rollout remains unverified.
