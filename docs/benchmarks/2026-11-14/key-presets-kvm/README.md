# Named API key presets on a real KVM node

**What this checks:** the `preset` field of an [API key policy entry](../../../API_KEY_ROTATION.md#presets)
(`read-only`, `developer`, `machines`, `admin`) through the shipped `hv2-control-plane` and `hv2-sandboxd`
binaries over verified TLS and node mTLS, with an owned Redis, and one real sandbox booted on KVM. It
extends [the scopes and roles run](../key-presets-roles-kvm/README.md); that run's 16 cases are kept and
repeated here unchanged.

Driver: `tools/check-key-presets-roles-kvm.py`. Reports: `run1/report.json`, `run2/report.json`,
`run3/report.json` (every request, the status expected and got, and the first 240 bytes of each response).

## Result: 25 of 25 cases, three runs, all pass (98 requests per run)

Every run's node log has 2 `Kvm` and 0 `Tcg` platform lines. All three runs used the same binaries:

| Input | sha256 |
|---|---|
| `hv2-control-plane` | `39f798d5e743626b7cf906c78b8a2f67fb0b55946637c6ca794e6ffa1e0c5305` |
| `hv2-sandboxd` | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| driver | `e6e7458853a2fabc1a11e262cc471ea66358f218fb2f37c891e9504747bd6104` |

New cases (the other 20 are the earlier scopes-and-roles cases):

| Case | Allowed (status) | Refused (status) |
|---|---|---|
| preset `read-only` | `GET /sandboxes` lists, `GET /cluster/nodes` (200) | sandbox detail, `exec`, `POST /v2/sandboxes`, delete, `/volumes`, `/machines` (403) |
| preset `developer` | list, detail, `exec` returned `keyroles`, `/templates`, create volume (201), `/events/sandboxes` (200) | `/machines`, `POST /sandboxes/{id}/owner` (403) |
| preset `machines` | `/machines`, `/sandboxes` list, `/events/sandboxes` (200) | sandbox detail, create sandbox, `/volumes` (403) |
| preset `admin` (no team, global) | `/cluster/nodes`, list, detail, `/volumes`, `/machines` (200) | nothing: admin reaches all |
| preset `admin` with explicit role `observer` | `GET /sandboxes` (200) | detail, create, `/volumes` (403) |
| start with an unknown preset | | the control plane exits non-zero with `invalid API key policy JSON` |
| SIGHUP reload, unknown preset | | rejected (`API key reload rejected` logged); developer, read-only and machines keys still work, an unknown key is 401 |
| SIGHUP reload, `read-only` with role `operator` | | rejected; the developer key still works |
| SIGHUP reload, the good file again | `API key policies reloaded` logged; developer key works | |

## What this does not show

- **NOT VERIFIED: key creation.** Presets exist only in the policy file an operator writes. There is no API or
  CLI that creates a key from a preset, and none was run.
- **NOT VERIFIED: SSO members.** `sso.rs` members carry their own scopes and role; presets are not accepted
  there and were not run.
- **`read-only` is `inventory` alone.** A first version also carried `events` and `telemetry`, which did
  nothing: its role is `observer`, and the observer role refuses every route except the five inventory GETs
  (`/sandboxes`, `/v2/sandboxes`, `/templates`, `/sandboxes/metrics`, `/cluster/nodes`). The preset was
  narrowed to say only what it grants, and `read-only` given `events` is now rejected. The runs kept here
  are of the narrowed build. Reading events or telemetry without operator rights is not offered.
- **The `admin` preset has no refused request**, because the admin scope permits every route; the lowered
  (`role: observer`) variant is the refusal case.
- Not a performance comparison; the three runs show repeatability of the status codes only, on one WSL2 host.
