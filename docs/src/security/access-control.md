# Access Control

Two separate things are described here, and an earlier version of this page ran
them together: the HTTP API's authentication, which is a flat list of keys, and
the agent layer's capabilities, which are per-agent and not configured in the
server's TOML at all.

## API keys

Authentication for the REST and gRPC surface. Keys are accepted or refused;
**there is no per-key permission set and no per-key quota.** Any accepted key
can call anything that is not on the excluded list.

```toml
[middleware]
enable_api_key_auth = true

[middleware.api_key]
keys = ["prod-key-xxxxx", "dev-key-xxxxx"]
excluded_paths = ["/health", "/agentic"]
```

Or by environment, which also turns authentication on when the list is
non-empty:

```bash
export HV2_API_KEYS="prod-key-xxxxx,dev-key-xxxxx"
```

Two keys differ only in the string. If one of them should be able to do less
than the other, that distinction does not exist at this layer, and issuing a
"read-only" key here does not make one.

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
  the flat `keys` list above. An operator who wrote this got no API-key
  authentication at all, because `[middleware.api_key].keys` stayed empty.
- **`[quotas.default]`** — `max_vms`, `max_cpu_cores`, `max_memory_gb`,
  `max_disk_gb`, `max_gpu`. There is a quota mechanism in the agent layer, but
  it is not populated from the server's configuration file.
- **`[rate_limits]`** with `default_rpm`, `vm_create_rpm`, `vm_exec_rpm`. Rate
  limiting is the token bucket above, and it is not per-operation.
