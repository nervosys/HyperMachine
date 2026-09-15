# Configuration

The API server, `hv2`, is configured by a TOML file, environment variables, or
command-line flags. The `hm` CLI is a different binary and reads no
configuration file at all — see [The `hm` CLI](#the-hm-cli) below.

## Configuration file

There is no search path and no per-user location. `hv2 serve` reads
**`hv2.toml` in the working directory**, or the file given to `--config`:

```bash
hv2 serve                          # reads ./hv2.toml if it exists
hv2 serve --config /etc/hv2.toml   # reads exactly this
```

A missing `./hv2.toml` is not an error — the server starts on defaults. A
missing `--config` path is.

Write a complete file, with every supported key at its default, with:

```bash
hv2 config init --output hv2.toml
```

That command is the authoritative list of what exists. Check one you already
have with:

```bash
hv2 config check hv2.toml
```

which names every key the build does not read *before* it reports the file
valid. Unknown keys are not an error — the schema is lax so that a file shared
with a newer build still loads — so this is the only thing that will tell you a
setting does nothing.

### The sections that exist

Three, plus their subsections: `[server]`, `[runtime]`, `[middleware]`.

```toml
[server]
host = "0.0.0.0"
rest_port = 8080
grpc_port = 50051
enable_runtime = true
enable_events = true
pre_warm_count = 2
shutdown_timeout_secs = 30

# TLS is on when both paths are set and off when either is missing.
# There is no `tls_enabled` switch.
tls_cert_path = "/etc/hypermachine/cert.pem"
tls_key_path = "/etc/hypermachine/key.pem"

[runtime]
instance_id = ""

[runtime.pool]
min_warm = 2
max_size = 64
default_vcpus = 2
default_memory = 2147483648

[middleware]
enable_api_key_auth = true
enable_rate_limit = true
enable_audit_log = true
enable_security_headers = true

[middleware.api_key]
keys = ["GENERATE_WITH_openssl_rand_base64_32"]
```

`[middleware]` is much larger than this excerpt — CORS, body limits, idempotency,
circuit breaking, response caching and more. `hv2 config init` writes them all.

## Environment variables

Read by `hv2` at startup, after the file and before the CLI flags. Every one is
prefixed `HV2_`:

| Variable                | Sets                              |
| ----------------------- | --------------------------------- |
| `HV2_HOST`              | `server.host`                     |
| `HV2_REST_PORT`         | `server.rest_port`                |
| `HV2_GRPC_PORT`         | `server.grpc_port`                |
| `HV2_PRE_WARM`          | `server.pre_warm_count`           |
| `HV2_ENABLE_RUNTIME`    | `server.enable_runtime`           |
| `HV2_ENABLE_EVENTS`     | `server.enable_events`            |
| `HV2_SHUTDOWN_TIMEOUT`  | `server.shutdown_timeout_secs`    |
| `HV2_INSTANCE_ID`       | `runtime.instance_id`             |
| `HV2_API_KEYS`          | `middleware.api_key.keys`, comma-separated; also turns API-key auth on |
| `HV2_CORS_ORIGINS`      | `middleware.cors.allowed_origins`, comma-separated |
| `HV2_BODY_LIMIT`        | `middleware.body_limit.max_bytes` |

Booleans accept `true` or `1`. A value that does not parse is ignored rather
than rejected.

## The `hm` CLI

`hm` is a separate binary from `hv2` and **reads no configuration file at
all** — there is no `--config` flag and no TOML anywhere in it. It has one
environment variable, `HM_API_KEY`, used by `hm mcp serve` for authentication;
without it that server logs a warning and runs unauthenticated.

Global flags are `--verbose` and the per-command options in `hm <command>
--help`.


## Not implemented

Earlier revisions of this page documented two more mechanisms. Neither exists,
and both are listed here rather than deleted so that anyone who followed them
can find out why nothing happened.

**Per-VM configuration files.** A `config.toml` under
`<data_dir>/vms/<vm-name>/` describing `[vm]`, `[hardware]`, `[storage]`,
`[network]`, `[gpu]` and `[boot]`. Nothing reads a file at that path, or at any
per-VM path. A VM is described by the arguments given when it is created.

**Security profiles.** `hm t2 create --security-profile high|development|
ai-sandbox`. There is no `--security-profile` flag; `hm t2 create --help` lists
what the command does take.

The same applies to the sections a previous version of this page showed in the
main config file — `[general]`, `[vm]`, `[network]`, `[security]`, `[gpu]`,
`[hypervisor]`, `[mcp]` and `[crypto]`. None of them is read. `hv2 config
check` will say so for any file you already have.


## Next Steps

- [Architecture Overview](../architecture/overview.md) - Understand HyperMachine internals
- [Security Guide](../security/overview.md) - Configure security settings
- [AI Integration](../ai/overview.md) - Set up AI agent access
