# Getting Started with AetherVM

Welcome to AetherVM! This guide will help you get started with the high-performance VM framework designed for AI agents.

## Installation

### From Source

```bash
git clone https://github.com/nervosys/aethervm
cd aethervm
cargo build --release
cargo install --path crates/aethervm-cli
```

### Using Cargo

```bash
cargo install aethervm-cli
```

## Quick Start

### 1. Create Your First VM

```bash
aethervm create \
  --name my-first-vm \
  --cpu 2 \
  --memory 4 \
  --network \
  --gpu
```

### 2. Start the VM

```bash
aethervm start my-first-vm
```

### 3. Check Status

```bash
aethervm status my-first-vm
```

### 4. Stop the VM

```bash
aethervm stop my-first-vm
```

## Using the API

### Start API Servers

```bash
aethervm serve --grpc-port 50051 --rest-port 8080
```

### REST API Examples

```bash
# Create a VM
curl -X POST http://localhost:8080/api/v1/vms \
  -H "Content-Type: application/json" \
  -d '{
    "name": "api-vm",
    "vcpu_count": 4,
    "memory_gb": 8,
    "enable_gpu": false
  }'

# List VMs
curl http://localhost:8080/api/v1/vms

# Get VM status
curl http://localhost:8080/api/v1/vms/{vm_id}

# Start VM
curl -X POST http://localhost:8080/api/v1/vms/{vm_id}/start
```

## AI Agent Integration

### Basic Script Execution

```bash
aethervm script my-vm --script "vm_state"
```

### From File

```bash
echo 'print("VM: " + vm_name); vm_state' > script.rhai
aethervm script my-vm --script script.rhai
```

### Programmatic Usage

```rust
use aethervm_agent::AgentVM;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create VM
    let vm = AgentVM::builder()
        .name("agent-vm")
        .cpu_cores(4)
        .memory_gb(8)
        .enable_gpu(true)
        .with_tracing()
        .build()
        .await?;
    
    // Start VM
    vm.start().await?;
    
    // Execute agent script
    let result = vm.execute_agent_script(r#"
        if vm_state == "Running" {
            print("VM is healthy");
            true
        } else {
            print("VM needs attention");
            false
        }
    "#).await?;
    
    println!("Result: {}", result);
    
    // Stop VM
    vm.stop().await?;
    
    Ok(())
}
```

## Configuration

Create a `config.toml`:

```toml
[vm]
name = "my-vm"
vcpus = 4
memory_mb = 8192

[network]
enabled = true
interface = "tap0"

[gpu]
enabled = false

[agent]
enabled = true
script_timeout_seconds = 300

[observability]
tracing = true
metrics = true
```

Load configuration:

```rust
use aethervm_core::Config;

let config = Config::from_file("config.toml")?;
```

## Advanced Features

### GPU Passthrough

```bash
aethervm create \
  --name gpu-vm \
  --gpu-passthrough \
  --gpu-device 0000:01:00.0
```

### Network Configuration

```bash
aethervm create \
  --name net-vm \
  --network \
  --ip 192.168.100.10 \
  --interface tap0
```

### Custom Script Timeout

```bash
aethervm script my-vm \
  --script "long_running_task()" \
  --timeout 600
```

## Monitoring and Observability

### Enable Tracing

```rust
let vm = AgentVM::builder()
    .with_tracing()
    .build()
    .await?;
```

### Export Metrics

Set OpenTelemetry endpoint:

```toml
[observability]
tracing = true
metrics = true
otlp_endpoint = "http://localhost:4317"
```

### View Logs

```bash
export RUST_LOG=aethervm=debug
aethervm start my-vm
```

## Examples

AetherVM includes several examples:

```bash
# Basic VM management
cargo run --example basic

# AI agent scripting
cargo run --example agent_script
```

## Troubleshooting

### VM Won't Start

- Check virtualization is enabled in BIOS
- Verify KVM/WHPX is available
- Check memory availability
- Review logs: `RUST_LOG=debug aethervm start my-vm`

### Script Timeout

- Increase timeout: `--timeout 600`
- Optimize script logic
- Check resource limits

### GPU Not Available

- Verify GPU drivers installed
- Check IOMMU configuration
- Review GPU passthrough setup

## Next Steps

- Read the [Architecture Guide](docs/architecture.md)
- Explore the [AI Agent API](docs/agent-api.md)
- Review [GPU Virtualization](docs/gpu.md)
- Check out [examples](examples/)

## Getting Help

- GitHub Issues: Report bugs and request features
- Discussions: Ask questions and share ideas
- Documentation: Comprehensive API docs

## Resources

- [GitHub Repository](https://github.com/nervosys/aethervm)
- [API Documentation](https://docs.rs/aethervm)
- [Examples](examples/)
- [Contributing Guide](CONTRIBUTING.md)

Happy virtualizing! 🚀
