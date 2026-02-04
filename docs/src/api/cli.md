# CLI Reference

The `hm` command-line interface for managing HyperMachine.

## Global Options

```bash
hm [OPTIONS] <COMMAND>

Options:
  --config <FILE>     Config file path
  --log-level <LEVEL> Log level (trace, debug, info, warn, error)
  --quiet             Suppress output
  --json              Output as JSON
  -h, --help          Print help
  -V, --version       Print version
```

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

#### Execute Command

```bash
hm t2 exec [OPTIONS] <NAME|ID> -- <COMMAND>

Options:
  --timeout <SECS>    Command timeout [default: 60]
  --user <USER>       Run as user

Examples:
  hm t2 exec my-vm -- uname -a
  hm t2 exec my-vm -- python -c "print('hello')"
  hm t2 exec --user root my-vm -- apt update
```

#### Console

```bash
hm t2 console <NAME|ID>

# Attach to VM console (Ctrl+] to detach)
```

#### Snapshots

```bash
# Create snapshot
hm t2 snapshot create <VM> --name <NAME>

# List snapshots
hm t2 snapshot list <VM>

# Restore snapshot
hm t2 snapshot restore <VM> <SNAPSHOT>

# Delete snapshot
hm t2 snapshot delete <VM> <SNAPSHOT>

Examples:
  hm t2 snapshot create my-vm --name before-update
  hm t2 snapshot list my-vm
  hm t2 snapshot restore my-vm before-update
```

### MCP Server

#### Start Server

```bash
hm mcp serve [OPTIONS]

Options:
  --port <PORT>       Listen port [default: 8080]
  --host <HOST>       Listen address [default: 127.0.0.1]
  --api-key <KEY>     API key (or use HM_API_KEY env var)
  --tls-cert <FILE>   TLS certificate
  --tls-key <FILE>    TLS private key

Examples:
  hm mcp serve --api-key "secret"
  hm mcp serve --port 8443 --tls-cert cert.pem --tls-key key.pem
```

#### List Tools

```bash
hm mcp tools [OPTIONS]

Options:
  --format <FORMAT>   Format (mcp, openai, anthropic, gemini)

Examples:
  hm mcp tools
  hm mcp tools --format openai
```

### System

#### Doctor

Check system requirements:

```bash
hm doctor

# Output:
# ✓ Hypervisor support: KVM
# ✓ CPU virtualization: Intel VT-x
# ✓ IOMMU support: Intel VT-d
# ✓ GPU passthrough: Available
```

#### Version

```bash
hm version

# hypermachine 0.1.0
# Built: 2025-01-15
# Rust: 1.75.0
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `HM_API_KEY` | MCP server API key |
| `HM_LOG_LEVEL` | Log level |
| `HM_CONFIG_FILE` | Config file path |
| `HM_DATA_DIR` | Data directory |

## Completion

Generate shell completions:

```bash
# Bash
hm completion bash > /etc/bash_completion.d/hm

# Zsh
hm completion zsh > ~/.zsh/completions/_hm

# Fish
hm completion fish > ~/.config/fish/completions/hm.fish

# PowerShell
hm completion powershell > $PROFILE.d/hm.ps1
```
