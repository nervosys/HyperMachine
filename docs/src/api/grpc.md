# gRPC API

HyperMachine provides a high-performance gRPC API for low-latency operations.

## Service Definition

```protobuf
syntax = "proto3";

package hypermachine.v1;

service HyperMachine {
  // VM lifecycle
  rpc CreateVm(CreateVmRequest) returns (Vm);
  rpc GetVm(GetVmRequest) returns (Vm);
  rpc ListVms(ListVmsRequest) returns (ListVmsResponse);
  rpc DeleteVm(DeleteVmRequest) returns (Empty);
  rpc StartVm(StartVmRequest) returns (Empty);
  rpc StopVm(StopVmRequest) returns (Empty);
  
  // Execution
  rpc ExecCommand(ExecCommandRequest) returns (ExecCommandResponse);
  rpc ExecStream(ExecStreamRequest) returns (stream ExecStreamResponse);
  
  // Files
  rpc UploadFile(UploadFileRequest) returns (Empty);
  rpc DownloadFile(DownloadFileRequest) returns (DownloadFileResponse);
  
  // Snapshots
  rpc CreateSnapshot(CreateSnapshotRequest) returns (Snapshot);
  rpc RestoreSnapshot(RestoreSnapshotRequest) returns (Empty);
  rpc ListSnapshots(ListSnapshotsRequest) returns (ListSnapshotsResponse);
  
  // Console streaming
  rpc ConsoleStream(ConsoleStreamRequest) returns (stream ConsoleStreamResponse);
}

message Vm {
  string id = 1;
  string name = 2;
  VmStatus status = 3;
  int32 cpu_cores = 4;
  int64 memory_mb = 5;
  bool gpu_enabled = 6;
  google.protobuf.Timestamp created_at = 7;
}

enum VmStatus {
  VM_STATUS_UNSPECIFIED = 0;
  VM_STATUS_CREATED = 1;
  VM_STATUS_RUNNING = 2;
  VM_STATUS_STOPPED = 3;
  VM_STATUS_PAUSED = 4;
}

message CreateVmRequest {
  string name = 1;
  int32 cpu_cores = 2;
  int64 memory_mb = 3;
  int64 disk_gb = 4;
  bool enable_gpu = 5;
  string network_mode = 6;
  string image = 7;
}

message ExecCommandRequest {
  string vm_id = 1;
  string command = 2;
  int32 timeout_secs = 3;
  map<string, string> environment = 4;
  string working_dir = 5;
}

message ExecCommandResponse {
  int32 exit_code = 1;
  bytes stdout = 2;
  bytes stderr = 3;
  int64 duration_ms = 4;
}
```

## Client Usage

### Rust

```rust
use hypermachine::grpc::HyperMachineClient;
use tonic::transport::Channel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_static("http://localhost:50051")
        .connect()
        .await?;
    
    let mut client = HyperMachineClient::new(channel);
    
    // Create VM
    let request = tonic::Request::new(CreateVmRequest {
        name: "grpc-vm".into(),
        cpu_cores: 4,
        memory_mb: 8192,
        ..Default::default()
    });
    
    let response = client.create_vm(request).await?;
    println!("Created VM: {:?}", response.into_inner());
    
    Ok(())
}
```

### Python

```python
import grpc
from hypermachine_pb2 import CreateVmRequest
from hypermachine_pb2_grpc import HyperMachineStub

channel = grpc.insecure_channel('localhost:50051')
stub = HyperMachineStub(channel)

# Create VM
request = CreateVmRequest(
    name="grpc-vm",
    cpu_cores=4,
    memory_mb=8192
)

response = stub.CreateVm(request)
print(f"Created VM: {response.id}")
```

### Go

```go
package main

import (
    "context"
    pb "github.com/nervosys/hypermachine/proto"
    "google.golang.org/grpc"
)

func main() {
    conn, _ := grpc.Dial("localhost:50051", grpc.WithInsecure())
    defer conn.Close()
    
    client := pb.NewHyperMachineClient(conn)
    
    resp, _ := client.CreateVm(context.Background(), &pb.CreateVmRequest{
        Name:     "grpc-vm",
        CpuCores: 4,
        MemoryMb: 8192,
    })
    
    fmt.Printf("Created VM: %s\n", resp.Id)
}
```

## Streaming

### Console Stream

```rust
let request = tonic::Request::new(ConsoleStreamRequest {
    vm_id: vm.id.clone(),
});

let mut stream = client.console_stream(request).await?.into_inner();

while let Some(response) = stream.message().await? {
    print!("{}", String::from_utf8_lossy(&response.data));
}
```

### Exec Stream

For long-running commands with real-time output:

```rust
let request = tonic::Request::new(ExecStreamRequest {
    vm_id: vm.id.clone(),
    command: "tail -f /var/log/syslog".into(),
});

let mut stream = client.exec_stream(request).await?.into_inner();

while let Some(response) = stream.message().await? {
    match response.output_type {
        OutputType::Stdout => print!("{}", String::from_utf8_lossy(&response.data)),
        OutputType::Stderr => eprint!("{}", String::from_utf8_lossy(&response.data)),
    }
}
```

## Configuration

```toml
[grpc]
enabled = true
port = 50051
max_message_size_mb = 16
keepalive_secs = 60

[grpc.tls]
enabled = true
cert = "/etc/hypermachine/grpc-cert.pem"
key = "/etc/hypermachine/grpc-key.pem"
```
