# Python SDK

The HyperMachine Python SDK provides a high-level interface for managing virtual machines.

## Installation

```bash
pip install hypermachine
```

## Quick Start

```python
from hypermachine import HyperMachine

# Connect to HyperMachine
hm = HyperMachine("http://localhost:8080", api_key="your-api-key")

# Create a VM
vm = hm.create_vm(
    name="my-sandbox",
    cpu=4,
    memory="8G",
    gpu=True
)

# Start the VM
vm.start()

# Execute a command
result = vm.exec("echo 'Hello, World!'")
print(result.stdout)  # Hello, World!

# Stop and delete
vm.stop()
vm.delete()
```

## HyperMachine Client

### Connection

```python
from hypermachine import HyperMachine

# Basic connection
hm = HyperMachine("http://localhost:8080", api_key="your-key")

# With TLS
hm = HyperMachine(
    "https://localhost:8443",
    api_key="your-key",
    verify_ssl=True,
    ca_cert="/path/to/ca.pem"
)

# From environment
import os
hm = HyperMachine(
    os.environ["HM_URL"],
    api_key=os.environ["HM_API_KEY"]
)
```

### VM Management

```python
# Create VM with all options
vm = hm.create_vm(
    name="ml-workstation",
    cpu=8,
    memory="32G",
    disk="100G",
    gpu=True,
    gpu_type="passthrough",  # or "vgpu"
    network="nat",  # or "bridge"
    image="ubuntu-22.04"
)

# List VMs
vms = hm.list_vms()
for vm in vms:
    print(f"{vm.name}: {vm.status}")

# Get specific VM
vm = hm.get_vm("my-sandbox")

# Find VM by name
vm = hm.find_vm(name="my-sandbox")
```

## VM Object

### Lifecycle

```python
# Start
vm.start()
vm.wait_until_running(timeout=60)

# Stop (graceful)
vm.stop()

# Stop (force)
vm.stop(force=True)

# Restart
vm.restart()

# Pause/Resume
vm.pause()
vm.resume()

# Delete
vm.delete()
```

### Status

```python
# Check status
print(vm.status)  # "running", "stopped", "paused"

# Get full info
info = vm.info()
print(f"CPU: {info.cpu_cores}")
print(f"Memory: {info.memory_mb}MB")
print(f"GPU: {info.gpu_enabled}")

# Check if running
if vm.is_running():
    print("VM is running")
```

### Command Execution

```python
# Simple command
result = vm.exec("uname -a")
print(result.stdout)
print(result.stderr)
print(result.exit_code)

# With timeout
result = vm.exec("long-running-script.sh", timeout=300)

# As specific user
result = vm.exec("whoami", user="root")

# With environment variables
result = vm.exec("echo $MY_VAR", env={"MY_VAR": "hello"})

# Working directory
result = vm.exec("ls", cwd="/home/user/project")
```

### Script Execution

```python
# Run a script
script = """
#!/bin/bash
echo "Starting setup..."
apt-get update
apt-get install -y python3 python3-pip
pip install numpy pandas
echo "Setup complete!"
"""

result = vm.exec_script(script, interpreter="/bin/bash")

# Python script
python_code = """
import sys
print(f"Python version: {sys.version}")
print("Hello from VM!")
"""

result = vm.exec_script(python_code, interpreter="python3")
```

### File Operations

```python
# Upload file
vm.upload("/home/user/data.json", '{"key": "value"}')

# Upload from local file
vm.upload_file("/home/user/model.pkl", "local_model.pkl")

# Download file
content = vm.download("/home/user/output.txt")

# Download to local file
vm.download_file("/home/user/results.csv", "local_results.csv")

# Check if file exists
if vm.file_exists("/home/user/data.json"):
    print("File exists")

# List directory
files = vm.list_dir("/home/user")
for f in files:
    print(f"{f.name}: {f.size} bytes")
```

### Snapshots

```python
# Create snapshot
snapshot = vm.create_snapshot("before-experiment")

# List snapshots
snapshots = vm.list_snapshots()
for s in snapshots:
    print(f"{s.name}: {s.created_at}")

# Restore snapshot
vm.restore_snapshot(snapshot.id)

# Delete snapshot
vm.delete_snapshot(snapshot.id)
```

### GPU Operations

```python
# Attach GPU
vm.attach_gpu(device="0000:01:00.0", mode="passthrough")

# List GPUs
gpus = vm.list_gpus()
for gpu in gpus:
    print(f"{gpu.name}: {gpu.memory_mb}MB")

# Detach GPU
vm.detach_gpu()
```

### Console Access

```python
# Get console output
output = vm.console_output(lines=100)
print(output)

# Interactive console (returns WebSocket URL)
ws_url = vm.console_websocket()
```

## Context Manager

```python
# Automatic cleanup
with hm.create_vm("temp-sandbox", cpu=2, memory="4G") as vm:
    vm.start()
    result = vm.exec("python -c 'print(1+1)'")
    print(result.stdout)  # 2
# VM is automatically stopped and deleted
```

## Async Support

```python
import asyncio
from hypermachine import AsyncHyperMachine

async def main():
    hm = AsyncHyperMachine("http://localhost:8080", api_key="your-key")
    
    # Create VMs in parallel
    vms = await asyncio.gather(
        hm.create_vm("vm-1", cpu=2, memory="4G"),
        hm.create_vm("vm-2", cpu=2, memory="4G"),
        hm.create_vm("vm-3", cpu=2, memory="4G"),
    )
    
    # Start all VMs
    await asyncio.gather(*[vm.start() for vm in vms])
    
    # Run commands in parallel
    results = await asyncio.gather(*[
        vm.exec("hostname") for vm in vms
    ])
    
    for vm, result in zip(vms, results):
        print(f"{vm.name}: {result.stdout}")

asyncio.run(main())
```

## Error Handling

```python
from hypermachine import (
    HyperMachineError,
    VMNotFoundError,
    VMAlreadyRunningError,
    QuotaExceededError,
)

try:
    vm = hm.get_vm("nonexistent")
except VMNotFoundError:
    print("VM not found")

try:
    vm.start()
except VMAlreadyRunningError:
    print("VM is already running")

try:
    hm.create_vm("test", cpu=1000, memory="1TB")
except QuotaExceededError as e:
    print(f"Quota exceeded: {e.message}")
```

## Configuration

```python
from hypermachine import HyperMachine, Config

config = Config(
    timeout=60,
    retry_count=3,
    retry_delay=1.0,
    verify_ssl=True,
)

hm = HyperMachine(
    "https://localhost:8443",
    api_key="your-key",
    config=config
)
```

## Next Steps

- [MCP Server](./mcp-server.md) - Direct API access
- [Tool Formats](./tool-formats.md) - LLM integration
- [GUI Automation](./gui-automation.md) - Desktop control
