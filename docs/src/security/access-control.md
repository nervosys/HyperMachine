# Access Control

HyperMachine uses capability-based access control for fine-grained permissions.

## API Key Permissions

```toml
# config.toml
[[api_keys]]
key = "prod-key-xxxxx"
name = "Production"
permissions = ["vm.create", "vm.start", "vm.stop", "vm.list"]
quotas = { max_vms = 10, max_memory_gb = 64 }

[[api_keys]]
key = "dev-key-xxxxx"
name = "Development"
permissions = ["*"]  # Full access
quotas = { max_vms = 5, max_memory_gb = 32 }
```

## Permission Categories

| Category | Permissions |
|----------|-------------|
| **VM Lifecycle** | `vm.create`, `vm.delete`, `vm.start`, `vm.stop` |
| **VM Info** | `vm.list`, `vm.get` |
| **Execution** | `vm.exec`, `vm.upload`, `vm.download` |
| **Snapshots** | `snapshot.create`, `snapshot.restore` |
| **GPU** | `gpu.attach`, `gpu.detach` |
| **Admin** | `admin.config`, `admin.users` |

## Resource Quotas

```toml
[quotas.default]
max_vms = 5
max_cpu_cores = 16
max_memory_gb = 32
max_disk_gb = 500
max_gpu = 1
```

## Rate Limiting

```toml
[rate_limits]
default_rpm = 120  # Requests per minute
vm_create_rpm = 10
vm_exec_rpm = 100
```
