//! gRPC server implementation

use crate::{ApiError, Result};
use hv2_agent::AgentVM;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

// Include generated proto code
pub mod proto {
    tonic::include_proto!("hv2.v1");
}

use proto::vm_service_server::{VmService, VmServiceServer};
use proto::*;

/// gRPC service implementation
pub struct VMServiceImpl {
    vms: Arc<RwLock<HashMap<String, Arc<AgentVM>>>>,
}

impl VMServiceImpl {
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn into_service(self) -> VmServiceServer<Self> {
        VmServiceServer::new(self)
    }
}

impl Default for VMServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl VmService for VMServiceImpl {
    type StreamEventsStream =
        tokio_stream::wrappers::ReceiverStream<std::result::Result<VmEvent, Status>>;

    async fn create_vm(
        &self,
        request: Request<CreateVmRequest>,
    ) -> std::result::Result<Response<CreateVmResponse>, Status> {
        let req = request.into_inner();
        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("config required"))?;

        let vm = AgentVM::builder()
            .name(&config.name)
            .cpu_cores(config.vcpu_count)
            .memory_gb(config.memory_size / (1024 * 1024 * 1024))
            .enable_gpu(config.enable_gpu)
            .enable_networking(config.enable_networking)
            .build()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let vm_id = uuid::Uuid::new_v4().to_string();
        self.vms.write().await.insert(vm_id.clone(), Arc::new(vm));

        Ok(Response::new(CreateVmResponse { vm_id }))
    }

    async fn start_vm(
        &self,
        request: Request<StartVmRequest>,
    ) -> std::result::Result<Response<StartVmResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        vm.start()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(StartVmResponse { success: true }))
    }

    async fn stop_vm(
        &self,
        request: Request<StopVmRequest>,
    ) -> std::result::Result<Response<StopVmResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        vm.stop()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(StopVmResponse { success: true }))
    }

    async fn pause_vm(
        &self,
        request: Request<PauseVmRequest>,
    ) -> std::result::Result<Response<PauseVmResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        vm.pause()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(PauseVmResponse { success: true }))
    }

    async fn resume_vm(
        &self,
        request: Request<ResumeVmRequest>,
    ) -> std::result::Result<Response<ResumeVmResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        vm.resume()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ResumeVmResponse { success: true }))
    }

    async fn get_vm_status(
        &self,
        request: Request<GetVmStatusRequest>,
    ) -> std::result::Result<Response<GetVmStatusResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        let metrics = vm
            .get_metrics()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let state = match metrics.state {
            hv2_core::VMState::Created => VmState::Created as i32,
            hv2_core::VMState::Running => VmState::Running as i32,
            hv2_core::VMState::Paused => VmState::Paused as i32,
            hv2_core::VMState::Stopped => VmState::Stopped as i32,
            hv2_core::VMState::Error => VmState::Error as i32,
        };

        Ok(Response::new(GetVmStatusResponse {
            vm_id: req.vm_id,
            state,
            vcpu_count: metrics.vcpu_count,
            memory_size: metrics.memory_size,
        }))
    }

    async fn list_v_ms(
        &self,
        _request: Request<ListVMsRequest>,
    ) -> std::result::Result<Response<ListVMsResponse>, Status> {
        let vms = self.vms.read().await;

        let vm_list: Vec<VmInfo> = vms
            .iter()
            .map(|(id, vm)| {
                let state = match vm.state() {
                    hv2_core::VMState::Created => VmState::Created as i32,
                    hv2_core::VMState::Running => VmState::Running as i32,
                    hv2_core::VMState::Paused => VmState::Paused as i32,
                    hv2_core::VMState::Stopped => VmState::Stopped as i32,
                    hv2_core::VMState::Error => VmState::Error as i32,
                };

                VmInfo {
                    vm_id: id.clone(),
                    name: vm.vm().config().name.clone(),
                    state,
                }
            })
            .collect();

        Ok(Response::new(ListVMsResponse { vms: vm_list }))
    }

    async fn execute_script(
        &self,
        request: Request<ExecuteScriptRequest>,
    ) -> std::result::Result<Response<ExecuteScriptResponse>, Status> {
        let req = request.into_inner();

        let vm = self.vms.read().await.get(&req.vm_id).ok_or_else(|| Status::not_found("VM not found"))?.clone();

        let result = vm
            .execute_agent_script(&req.script)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ExecuteScriptResponse {
            result: result.to_string(),
            logs: vec![],
        }))
    }

    async fn stream_events(
        &self,
        _request: Request<StreamEventsRequest>,
    ) -> std::result::Result<Response<Self::StreamEventsStream>, Status> {
        // TODO: Implement event streaming
        Err(Status::unimplemented("Event streaming not yet implemented"))
    }
}

/// Start gRPC server
pub async fn serve(addr: impl Into<std::net::SocketAddr>) -> Result<()> {
    let addr = addr.into();
    let service = VMServiceImpl::new().into_service();

    tracing::info!("Starting gRPC server on {}", addr);

    tonic::transport::Server::builder()
        .add_service(service)
        .serve(addr)
        .await
        .map_err(|e| ApiError::Transport(e.to_string()))?;

    Ok(())
}
