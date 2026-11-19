# Access Control

Two separate things are described here, and an earlier version of this page ran
them together: the HTTP API's authentication, which is a list of keys that each
carry a role and optionally a tenant, and the agent layer's capabilities, which
are per-agent and not configured in the server's TOML at all.

## API keys

Authentication for the REST and gRPC surface. There are two kinds of key:

- `keys` entries are **unscoped administrators**: every route, no tenant.
- `scoped_keys` entries have a **role** and, optionally, a **tenant**.

```toml
[middleware]
enable_api_key_auth = true

[middleware.api_key]
keys = ["prod-key-xxxxx"]
scoped_keys = [
  { key = "ops-key-xxxxx", role = "operator", tenant = "acme" },
  { key = "audit-key-xxxxx", role = "viewer" },
]
excluded_paths = ["/health", "/agentic"]
```

Or by environment, which also turns authentication on:

```bash
export HV2_API_KEYS="prod-key-xxxxx"
export HV2_API_SCOPED_KEYS="operator:acme:ops-key-xxxxx,viewer::audit-key-xxxxx"
```

(`HV2_API_SCOPED_KEYS` entries are `role:tenant:key`; an empty tenant means
none.)

| Role       | Reads (GET, HEAD, OPTIONS) | Writes                                                  |
| ---------- | -------------------------- | ------------------------------------------------------- |
| `viewer`   | everywhere                 | nowhere (`403 ROLE_DENIED`); every gRPC call is refused |
| `operator` | everywhere                 | everywhere except the admin-only routes                 |
| `admin`    | everywhere                 | everywhere                                              |

The admin-only write routes are `/api/v1/images` (the image allowlist),
`/api/v1/runtime` (the fleet) and `/api/v1/gpu-fabric`, apart from capacity
reservations. The check is by method and route prefix, not per resource: it is
coarse, and an operator can still do anything that is a write outside those
prefixes (start and stop VMs, run guest commands, register webhooks).

**Tenants.** A key with a `tenant` *is* that tenant to the routes that scope by
tenant (the agent runtime and GPU capacity reservations): the `X-Tenant-Id`
header is set from the key, and a request naming a different tenant is refused
with `403 TENANT_MISMATCH`. A key with no tenant, including every `keys` entry,
leaves the header to the caller, who can then name any tenant, so use those
only for operators who may act as any. With **no API key at all**, the header
is whatever the caller sends: run the server that way only behind a proxy that
sets it from its own authentication.

**Where the server listens.** With no API key the server binds loopback only
(`127.0.0.1`, the default) and **refuses to start** on any other address
unless `server.allow_unauthenticated = true` (`hv2 serve --allow-unauthenticated`),
which says that every caller who can reach the port is an administrator.

**Not wired.** `hv2-api` also contains a graph-based permission middleware
(`permission_middleware.rs`) that no router installs. It is not used: its route
table maps only some routes to a permission and lets the rest through, and
nothing maps a key to a principal. The roles above are what gates the API.

## Rate limiting

A token bucket over the whole surface, not a per-operation budget:

```toml
[middleware]
enable_rate_limit = true

[middleware.rate_limit]
capacity = 100        # burst size
refill_rate = 10.0    # tokens per second
excluded_paths = ["/health"]
```

`vm.create` cannot be given a different allowance from `vm.list`; the bucket
does not know which endpoint spent the token.

## Agent capabilities

Names of the form `vm.create`, `vm.exec`, `snapshot.restore` are **MCP tool
names**, and the agent layer does gate them per agent — capabilities together
with VM ownership are what decide whether an agent's call is permitted. That
machinery lives in `hv2-agent` and is granted programmatically when an agent is
created. It is not read from the server's configuration file, and it does not
scope an HTTP API key.

| Category         | Tools                                           |
| ---------------- | ----------------------------------------------- |
| **VM Lifecycle** | `vm.create`, `vm.delete`, `vm.start`, `vm.stop` |
| **VM Info**      | `vm.list`, `vm.get`                             |
| **Execution**    | `vm.exec`, `guest.exec`                         |
| **Snapshots**    | `snapshot.create`, `snapshot.restore`           |
| **GPU**          | `gpu.attach`, `gpu.detach`, `gpu.list`          |

Registered in `hv2-agent`'s MCP server (41 tools) and `hm-cli`'s ontology
(8). `vm.upload` and `vm.download` were in this table and are in neither: no
file transfer to or from a guest exists, by any name.

## Not implemented

Configuration this page used to document. None of it is read; a file
containing it parses, validates, and does nothing. `hv2 config check <file>`
names every such key.

- **`[[api_keys]]`** with `permissions` and `quotas` per key. The real model is
  the `keys` and `scoped_keys` lists above (a role and a tenant, no per-key
  permission list or quota). An operator who wrote this got no API-key
  authentication at all, because `[middleware.api_key].keys` stayed empty.
- **`[quotas.default]`** — `max_vms`, `max_cpu_cores`, `max_memory_gb`,
  `max_disk_gb`, `max_gpu`. There is a quota mechanism in the agent layer, but
  it is not populated from the server's configuration file.
- **`[rate_limits]`** with `default_rpm`, `vm_create_rpm`, `vm_exec_rpm`. Rate
  limiting is the token bucket above, and it is not per-operation.
