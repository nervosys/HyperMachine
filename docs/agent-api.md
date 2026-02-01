# AI Agent API Reference

## Overview

The HyperMachine Agent API provides a safe, scriptable interface for AI agents to control virtual machines. All operations are subject to capability checks and resource limits.

## Quick Start

```rust
use hv2_agent::AgentVM;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create VM with AI capabilities
    let vm = AgentVM::builder()
        .name("my-agent-vm")
        .cpu_cores(4)
        .memory_gb(8)
        .enable_gpu(true)
        .with_tracing()
        .build()
        .await?;
    
    // Start the VM
    vm.start().await?;
    
    // Execute agent script
    let result = vm.execute_agent_script(r#"
        // Script code here
        vm_state
    "#).await?;
    
    println!("Result: {:?}", result);
    Ok(())
}
```

## Scripting Language

HyperMachine uses [Rhai](https://rhai.rs/) as its scripting language - a simple, safe, embedded scripting language for Rust.

### Basic Syntax

```javascript
// Variables
let x = 10;
let name = "test-vm";

// Functions
fn calculate(a, b) {
    return a + b;
}

// Control flow
if x > 5 {
    print("x is large");
} else {
    print("x is small");
}

// Loops
for i in 0..10 {
    print(i);
}
```

## VM API Functions

### State Management

#### `vm_state`
Returns the current VM state as a string.

```javascript
let state = vm_state;
print(state);  // "Running"
```

#### `vm_name`
Returns the VM name.

```javascript
print(vm_name);  // "my-vm"
```

#### `vcpu_count`
Returns the number of vCPUs.

```javascript
print(vcpu_count);  // 4
```

#### `memory_size`
Returns total memory size in bytes.

```javascript
let mem_gb = memory_size / (1024 * 1024 * 1024);
print(`Memory: ${mem_gb} GB`);
```

## Script Examples

### Monitor CPU Usage

```javascript
// Check CPU usage and scale if needed
let cpu_usage = get_cpu_usage();

if cpu_usage > 0.8 {
    print("High CPU usage detected: " + cpu_usage);
    
    // Scale up vCPUs if available
    let current_vcpus = vcpu_count;
    if current_vcpus < 16 {
        scale_vcpu(current_vcpus + 2);
        print("Scaled vCPUs to " + (current_vcpus + 2));
    }
}
```

### Automated Health Check

```javascript
// Periodic health check
fn health_check() {
    let state = vm_state;
    
    if state == "Error" {
        print("VM in error state, attempting restart");
        vm_restart();
        return false;
    }
    
    let mem_usage = get_memory_usage();
    if mem_usage > 0.9 {
        print("High memory pressure: " + mem_usage);
        return false;
    }
    
    return true;
}

health_check()
```

### Conditional Execution

```javascript
// Execute command based on conditions
if vm_state == "Running" {
    let uptime = get_uptime();
    
    if uptime > 3600 * 24 {  // 24 hours
        print("VM uptime exceeds 24 hours");
        
        // Check if maintenance window
        let hour = get_current_hour();
        if hour >= 2 && hour <= 4 {
            print("In maintenance window, performing restart");
            vm_restart();
        }
    }
}
```

## Capability System

Scripts are subject to capability checks. Agents must have the required capabilities to perform operations.

### Available Capabilities

| Capability      | Description                   |
| --------------- | ----------------------------- |
| `VmRead`        | Read VM state and metrics     |
| `VmControl`     | Start, stop, pause, resume VM |
| `VmModify`      | Change VM configuration       |
| `MemoryAccess`  | Read/write guest memory       |
| `Network`       | Network operations            |
| `Gpu`           | GPU operations                |
| `DeviceControl` | Manage virtual devices        |
| `GuestExec`     | Execute commands in guest     |
| `Snapshot`      | Create/restore snapshots      |
| `Metrics`       | Access metrics and monitoring |

### Setting Capabilities

```rust
use hv2_agent::{AgentVM, CapabilitySet, Capability};

let mut caps = CapabilitySet::new();
caps.add(Capability::VmRead);
caps.add(Capability::VmControl);
caps.add(Capability::Metrics);

// Use custom capability set when creating script engine
```

## Resource Limits

All scripts are subject to resource limits to prevent abuse:

- **Timeout**: 300 seconds (configurable)
- **Memory**: 512 MB (configurable)
- **CPU Time**: 300 seconds (configurable)
- **Max Operations**: 100,000 operations

### Configuring Limits

```rust
use std::time::Duration;

let vm = AgentVM::builder()
    .name("limited-vm")
    .script_timeout(Duration::from_secs(60))
    .build()
    .await?;
```

## Error Handling

Scripts should handle errors gracefully:

```javascript
try {
    let result = risky_operation();
    print("Success: " + result);
} catch (error) {
    print("Error occurred: " + error);
}
```

## Best Practices

1. **Always check state** before operations
2. **Use timeouts** for long-running operations
3. **Handle errors** explicitly
4. **Log important actions** for debugging
5. **Test scripts** in isolated environments first
6. **Minimize resource usage** in loops
7. **Use capabilities** conservatively

## Advanced: WASM Plugins

For more complex logic, use WebAssembly plugins:

```rust
// Load WASM plugin
let plugin = vm.load_wasm_plugin("path/to/plugin.wasm").await?;

// Call plugin function
let result = plugin.call("process_data", &data).await?;
```

## Security Considerations

- Scripts run in sandboxed environment
- No access to host filesystem by default
- Network access controlled by capabilities
- All operations are audited and logged
- Resource limits prevent DoS attacks

## Metrics and Observability

Scripts can access VM metrics:

```javascript
let metrics = get_all_metrics();
print("CPU: " + metrics.cpu_usage);
print("Memory: " + metrics.memory_usage);
print("Network TX: " + metrics.network_tx_bytes);
print("Network RX: " + metrics.network_rx_bytes);
```

### VMMetrics Structure

The `VMMetrics` struct provides comprehensive VM state information:

| Field | Type | Description |
|-------|------|-------------|
| `state` | `VMState` | Current VM state (Created, Running, Paused, Stopped) |
| `vcpu_count` | `u32` | Number of virtual CPUs |
| `memory_size` | `u64` | Total memory size in bytes |
| `uptime_seconds` | `u64` | VM uptime since last start |
| `cpu_usage_percent` | `Option<f64>` | CPU utilization (0-100) across all vCPUs |
| `memory_used_bytes` | `Option<u64>` | Memory used (requires virtio-balloon) |

```rust
let metrics = vm.get_metrics().await?;
println!("State: {:?}", metrics.state);
println!("vCPUs: {}", metrics.vcpu_count);
println!("Memory: {} GB", metrics.memory_size / (1024 * 1024 * 1024));
println!("Uptime: {} seconds", metrics.uptime_seconds);
if let Some(cpu) = metrics.cpu_usage_percent {
    println!("CPU Usage: {:.1}%", cpu);
}
```

### CPU Usage Tracking

CPU usage is calculated from vCPU run time statistics:
- Aggregates run time across all vCPUs
- Reports percentage relative to total available CPU time
- Returns `None` if VM is not running or just started

### Memory Usage Tracking

Memory usage tracking requires guest OS cooperation:
- Currently returns `None` (infrastructure in place)
- Full implementation requires virtio-balloon driver
- Guest OS reports actual memory pressure to hypervisor

## Integration Examples

### With LangChain

```python
from langchain import LLMChain
from hv2_agent import AgentVM

vm = AgentVM.create(name="langchain-vm")

# AI generates script
chain = LLMChain(...)
script = chain.run("Scale VM if CPU > 80%")

# Execute generated script
result = vm.execute_script(script)
```

### With AutoGPT

```python
from autogpt import Agent
from hv2_agent import AgentVM

class VMManager(Agent):
    def __init__(self):
        self.vm = AgentVM.create(name="autogpt-vm")
    
    def manage_resources(self):
        script = self.generate_script()
        return self.vm.execute_script(script)
```

## API Reference

For complete API documentation, see the [rustdoc](https://docs.rs/hv2-agent).
