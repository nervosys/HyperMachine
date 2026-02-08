# Configuration

HyperMachine can be configured through configuration files, environment variables, or command-line arguments.

## Configuration File

The default configuration file location is:

- **Linux/macOS:** `~/.config/hypermachine/config.toml`
- **Windows:** `%APPDATA%\hypermachine\config.toml`

### Example Configuration

```toml
[general]
log_level = "info"  # trace, debug, info, warn, error
data_dir = "/var/lib/hypermachine"

[server]
host = "127.0.0.1"
port = 8080
api_key = "your-secret-key"  # Or use HM_API_KEY env var
tls_enabled = true
tls_cert = "/etc/hypermachine/cert.pem"
tls_key = "/etc/hypermachine/key.pem"

[vm.defaults]
cpu_cores = 2
memory_mb = 4096
enable_gpu = false
network_mode = "nat"  # nat, bridge, host

[hypervisor]
backend = "auto"  # auto, kvm, whpx, hvf
nested_virtualization = false

[gpu]
enabled = true
passthrough = false
virtual_gpu = true
vulkan_enabled = true

[network]
default_bridge = "hm0"
dns_servers = ["8.8.8.8", "8.8.4.4"]
enable_ipv6 = true

[security]
seccomp_enabled = true
capability_mode = "strict"  # strict, permissive
audit_logging = true
audit_log_path = "/var/log/hypermachine/audit.log"

[crypto]
fips_mode = false
default_cipher = "aes-256-gcm"
key_derivation = "hkdf-sha256"

[mcp]
enabled = true
max_concurrent_operations = 100
operation_timeout_secs = 300
```

## Environment Variables

| Variable         | Description                | Default           |
| ---------------- | -------------------------- | ----------------- |
| `HM_API_KEY`     | API key for authentication | None              |
| `HM_LOG_LEVEL`   | Logging verbosity          | `info`            |
| `HM_DATA_DIR`    | Data storage directory     | Platform-specific |
| `HM_CONFIG_FILE` | Custom config file path    | Default location  |
| `HM_HYPERVISOR`  | Force hypervisor backend   | `auto`            |
| `HM_TLS_CERT`    | TLS certificate path       | None              |
| `HM_TLS_KEY`     | TLS private key path       | None              |

## Command-Line Arguments

All configuration options can be overridden via CLI:

```bash
# Override server port
hm mcp serve --port 9090

# Override log level
hm --log-level debug t2 list

# Use custom config file
hm --config /path/to/config.toml t2 create --name test

# Multiple overrides
hm mcp serve \
  --port 8443 \
  --tls-cert /etc/ssl/cert.pem \
  --tls-key /etc/ssl/key.pem \
  --api-key "production-key"
```

## VM-Specific Configuration

Each VM can have its own configuration in `<data_dir>/vms/<vm-name>/config.toml`:

```toml
[vm]
name = "my-vm"
uuid = "550e8400-e29b-41d4-a716-446655440000"

[hardware]
cpu_cores = 4
memory_mb = 8192
cpu_model = "host"  # host, qemu64, max

[gpu]
enabled = true
passthrough_device = "0000:01:00.0"  # PCI address for passthrough

[storage]
[[storage.disks]]
path = "disk0.qcow2"
format = "qcow2"
size_gb = 100

[[storage.disks]]
path = "/dev/nvme0n1"
format = "raw"
readonly = false

[network]
[[network.interfaces]]
type = "virtio"
mode = "nat"
mac = "52:54:00:12:34:56"

[[network.interfaces]]
type = "virtio"
mode = "bridge"
bridge = "br0"

[boot]
firmware = "uefi"  # bios, uefi
secure_boot = true
boot_order = ["disk", "network"]
```

## Security Profiles

Pre-defined security profiles for different use cases:

```bash
# High security (production)
hm t2 create --name secure-vm --security-profile high

# Development (more permissive)
hm t2 create --name dev-vm --security-profile development

# AI sandbox (isolated with network access)
hm t2 create --name ai-vm --security-profile ai-sandbox
```

### Profile Definitions

**high:**
```toml
[security]
seccomp_enabled = true
capability_mode = "strict"
network_isolation = true
audit_logging = true
```

**development:**
```toml
[security]
seccomp_enabled = false
capability_mode = "permissive"
network_isolation = false
audit_logging = false
```

**ai-sandbox:**
```toml
[security]
seccomp_enabled = true
capability_mode = "strict"
network_isolation = false  # AI needs network
audit_logging = true
resource_limits.cpu_percent = 80
resource_limits.memory_percent = 75
```

## Next Steps

- [Architecture Overview](../architecture/overview.md) - Understand HyperMachine internals
- [Security Guide](../security/overview.md) - Configure security settings
- [AI Integration](../ai/overview.md) - Set up AI agent access
