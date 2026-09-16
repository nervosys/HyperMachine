# CLI Reference

The `hm` command-line interface for managing HyperMachine.

## Global Options

```bash
hm [OPTIONS] <COMMAND>

Options:
  -v, --verbose  Enable verbose logging
  -h, --help     Print help
  -V, --version  Print version
```

Commands: `t1`, `t2`, `serve`, `completions`, `info`.

`--config`, `--log-level`, `--quiet` and `--json` were documented here and do
not exist. `hm` reads no configuration file at all, and its verbosity is
`--verbose` or `RUST_LOG`.

## Commands

### Type-2 (Hosted) Hypervisor

#### Create VM

```bash
hm t2 create [OPTIONS] --name <NAME>

Options:
  --name <NAME>       VM name (required)
  --cpu <N>           CPU cores [default: 2]
  --memory <SIZE>     Memory (e.g., 4G, 4096M) [default: 4G]
  --disk <SIZE>       Disk size [default: 20G]
  --gpu               Enable GPU
  --network <MODE>    Network mode (nat, bridge, host) [default: nat]
  --image <IMAGE>     Base image

Examples:
  hm t2 create --name dev --cpu 4 --memory 8G
  hm t2 create --name ml --cpu 8 --memory 32G --gpu --disk 100G
```

#### List VMs

```bash
hm t2 list [OPTIONS]

Options:
  --status <STATUS>   Filter by status (running, stopped, paused)
  --format <FORMAT>   Output format (table, json, yaml)

Examples:
  hm t2 list
  hm t2 list --status running
  hm t2 list --json
```

#### Start VM

```bash
hm t2 start <NAME|ID>

Examples:
  hm t2 start my-vm
  hm t2 start vm-550e8400
```

#### Stop VM

```bash
hm t2 stop [OPTIONS] <NAME|ID>

Options:
  --force    Force stop (don't wait for graceful shutdown)

Examples:
  hm t2 stop my-vm
  hm t2 stop --force my-vm
```

#### Delete VM

```bash
hm t2 delete [OPTIONS] <NAME|ID>

Options:
  --force    Don't prompt for confirmation

Examples:
  hm t2 delete my-vm
  hm t2 delete --force my-vm
```

#### Execute a Script

The subcommand is `script`, and it runs an agent script rather than an
arbitrary command line. There is no `hm t2 exec`.

```bash
hm t2 script [OPTIONS] --script <SCRIPT> <NAME>

Options:
  -s, --script <SCRIPT>    Script content or file path
  -t, --timeout <TIMEOUT>  Timeout in seconds [default: 300]

Examples:
  hm t2 script my-vm --script "print('hello')"
  hm t2 script my-vm --script ./provision.rhai
```

#### Status

```bash
hm t2 status <NAME>
```

#### Console and snapshots

Neither has a CLI subcommand. `hm t2 console` and `hm t2 snapshot` were
documented here and do not exist.

Both are reachable over the REST API instead:

```bash
curl http://localhost:8080/api/v1/vms/{id}/console
curl -X POST http://localhost:8080/api/v1/vms/{id}/snapshots
```


### MCP Server

#### Start Server

There is no `hm mcp` subcommand. `hm serve` runs the MCP server, alongside the
REST and gRPC surfaces:

```bash
hm serve [OPTIONS]

Options:
      --grpc-port <GRPC_PORT>  gRPC port [default: 50051]
      --rest-port <REST_PORT>  REST API port [default: 8080]
  -v, --verbose                Enable verbose logging

Examples:
  hm mcp serve --api-key "secret"
  hm mcp serve --port 8443 --tls-cert cert.pem --tls-key key.pem
```

#### List Tools

No CLI subcommand; `hm mcp tools` was documented here and does not exist. The
running server lists them over HTTP, and the API server `hv2` serves the
per-vendor schemas:

```bash
curl http://localhost:8080/mcp/tools              # hm serve
curl http://localhost:8080/agentic/tools/openai   # hv2 serve
curl http://localhost:8080/agentic/tools/anthropic
curl http://localhost:8080/agentic/tools/gemini
```

### System

#### Info

```bash
hm info
```

Version and system information. There is no `hm version` subcommand; `hm
--version` prints the version alone.

`hm doctor` was documented here and does not exist. Nothing checks hypervisor
support, VT-x, IOMMU or GPU passthrough from the command line.

## Environment Variables

| Variable     | Description                                     |
| ------------ | ----------------------------------------------- |
| `HM_API_KEY` | MCP server API key; without it, it runs unauthenticated |
| `RUST_LOG`   | Log filter, e.g. `RUST_LOG=info`                 |

`HM_LOG_LEVEL`, `HM_CONFIG_FILE` and `HM_DATA_DIR` were documented here and are
read by nothing. The API server `hv2` has its own variables, all prefixed
`HV2_` — see `docs/src/getting-started/configuration.md`.

## Completion

Generate shell completions:

The subcommand is `completions`, plural:

```bash
# Bash
hm completions bash > /etc/bash_completion.d/hm

# Zsh
hm completions zsh > ~/.zsh/completions/_hm

# Fish
hm completions fish > ~/.config/fish/completions/hm.fish

# PowerShell
hm completions powershell > $PROFILE.d/hm.ps1
```
