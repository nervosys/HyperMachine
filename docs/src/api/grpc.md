# gRPC API

HyperMachine provides a high-performance gRPC API for low-latency operations.

## Service Definition

The service is defined in `crates/hv2-api/proto/vm.proto`, which is the
authority: generate clients from that file rather than from this page.

```protobuf
syntax = "proto3";

package hv2.v1;

service VMService {
  rpc CreateVM(CreateVMRequest) returns (CreateVMResponse);
  rpc StartVM(StartVMRequest) returns (StartVMResponse);
  rpc StopVM(StopVMRequest) returns (StopVMResponse);
  rpc PauseVM(PauseVMRequest) returns (PauseVMResponse);
  rpc ResumeVM(ResumeVMRequest) returns (ResumeVMResponse);
  rpc GetVMStatus(GetVMStatusRequest) returns (GetVMStatusResponse);
  rpc ListVMs(ListVMsRequest) returns (ListVMsResponse);
  rpc ExecuteScript(ExecuteScriptRequest) returns (ExecuteScriptResponse);
  rpc StreamEvents(StreamEventsRequest) returns (stream VMEvent);
}
```

The twenty message definitions are in the same file and are not reproduced
here, because a copy of a schema is a copy that goes stale -- which is what
this section was.

`PauseVM` and `ResumeVM` exist and always fail: nothing in this hypervisor
suspends a running vCPU, the same reason their REST equivalents return 500.

### What an earlier version of this page described

A `HyperMachine` service in package `hypermachine.v1`, with `CreateVm`,
`GetVm`, `DeleteVm`, `ExecCommand`, `ExecStream`, `UploadFile`,
`DownloadFile`, `CreateSnapshot`, `RestoreSnapshot`, `ListSnapshots` and
`ConsoleStream`, plus its own message definitions.

None of it resolves. The package and the service name are both different from
the real ones, and gRPC method names are case-sensitive, so even `CreateVm`
would not reach `CreateVM`. A client generated from that definition could not
call this server at all.

Of the operations it named, `DeleteVM`, file transfer, snapshots and console
streaming are not in `VMService`. Snapshots and the console are reachable over
REST instead -- see `docs/src/api/rest.md`.

## Client Usage

Generated from `crates/hv2-api/proto/vm.proto` by `tonic`, so the client type
is `vm_service_client::VmServiceClient` in package `hv2.v1`.

```rust
use tonic::transport::Channel;
// from tonic::include_proto!("hv2.v1")
use proto::vm_service_client::VmServiceClient;
use proto::{CreateVmRequest, StreamEventsRequest, VmConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_static("http://localhost:50051")
        .connect()
        .await?;
    let mut client = VmServiceClient::new(channel);

    let created = client
        .create_vm(CreateVmRequest {
            config: Some(VmConfig {
                name: "grpc-vm".into(),
                ..Default::default()
            }),
        })
        .await?
        .into_inner();

    // The one streaming method: VM events, not a console.
    let mut events = client
        .stream_events(StreamEventsRequest {
            vm_id: created.vm_id.clone(),
        })
        .await?
        .into_inner();

    while let Some(event) = events.message().await? {
        println!("{event:?}");
    }
    Ok(())
}
```

`tonic` renames as it generates: `VMService` becomes `VmServiceClient`,
`CreateVMRequest` becomes `CreateVmRequest`, and `CreateVM` becomes
`create_vm`. The proto file spells them the other way; both are correct in
their own language, and the wire names are the proto's.

Clients in other languages generate from the same file. Point `protoc` at
`crates/hv2-api/proto/vm.proto` rather than transcribing a definition from
documentation, which is how this page came to describe a service that does not
exist.

## Streaming

`StreamEvents` is the only streaming method. It carries VM lifecycle events
for one VM, as shown above.

An earlier version of this page documented `ConsoleStream` and `ExecStream`.
Neither exists. For console output, poll `GET /api/v1/vms/{id}/console` over
REST; for command output, `ExecuteScript` returns it when the command
finishes, and there is no incremental variant.


## Configuration

The gRPC server takes one setting, its port:

```toml
[server]
grpc_port = 50051
```

`HV2_GRPC_PORT` sets the same thing from the environment.

Nothing else about it is configurable. `grpc::serve` is handed an address and
nothing more, so there is no maximum message size, no keepalive interval, and
**no TLS on the gRPC listener** — an earlier version of this page documented
`[grpc]` and `[grpc.tls]` sections, and neither is read. The `tls_cert_path`
and `tls_key_path` under `[server]` apply to the REST listener.
