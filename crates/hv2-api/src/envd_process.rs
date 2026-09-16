//! A real implementation of envd's `process.Process` gRPC service, against
//! the actual proto copied from `e2b-dev/runtime`'s
//! `packages/envd/spec/process/process.proto` (see `proto/process.proto`).
//!
//! See `docs/CUBESANDBOX_PARITY_ROADMAP.md`, Phase 1, for what this is and
//! is not: real wire messages and a real `Start` RPC against
//! `AgentVM::exec_in_guest`, but `exec_in_guest` runs to completion and
//! returns its whole output at once, so `Start` emits `StartEvent` /
//! `DataEvent` / `EndEvent` in a burst rather than truly live-streaming
//! output. `Connect`, `Update`, `StreamInput`, `SendInput`, `SendSignal`,
//! and `CloseStdin` are `unimplemented` -- each needs a way to reach a
//! process that is still running, which `exec_in_guest` (one call, blocks
//! until exit) does not provide.
//!
//! `List` is real, backed by an actual table of in-flight `Start` calls
//! (keyed by a synthetic id this struct assigns, *not* a real guest PID --
//! `exec_in_guest` doesn't report one, so `StartEvent::pid` and `List`'s
//! `ProcessInfo::pid` both carry this synthetic id instead, honestly
//! smaller in scope than the field's name suggests). A `Start` call
//! registers itself before the guest command runs and removes itself when
//! it ends, so `List` reflects genuinely in-flight work, not a fabricated
//! empty table.
//!
//! Pulled out of the `envd_process` example and into the crate itself so
//! both that example (one VM, one daemon, matching envd's real per-sandbox
//! shape) and `e2b_compat`'s per-sandbox gRPC listener (see
//! `serve_for_sandbox`) share one implementation rather than diverging.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use hv2_agent::AgentVM;

pub mod process_proto {
    // As in `envd_filesystem`: generated code, lints nobody can act on.
    #![allow(clippy::large_enum_variant, clippy::doc_overindented_list_items)]

    tonic::include_proto!("process");
}

use process_proto::process_event::{DataEvent, EndEvent, Event as ProcessEventKind, StartEvent};
use process_proto::process_server::Process;
pub use process_proto::process_server::ProcessServer;
use process_proto::{
    CloseStdinRequest, CloseStdinResponse, ConnectRequest, ConnectResponse, ListRequest,
    ListResponse, ProcessEvent, ProcessInfo, SendInputRequest, SendInputResponse,
    SendSignalRequest, SendSignalResponse, StartRequest, StartResponse, StreamInputRequest,
    StreamInputResponse, UpdateRequest, UpdateResponse,
};

/// One VM's `process.Process` service -- envd for exactly the sandbox this
/// was built for, not a multi-sandbox multiplexer.
pub struct EnvdProcess {
    vm: Arc<AgentVM>,
    /// Synthetic pid -> what `Start` was asked to run, for every call
    /// currently between its `StartEvent` and its `EndEvent`. See this
    /// module's doc comment for why the id isn't a real guest PID.
    running: Arc<Mutex<HashMap<u32, ProcessInfo>>>,
    next_pid: AtomicU32,
}

impl EnvdProcess {
    pub fn new(vm: Arc<AgentVM>) -> Self {
        Self {
            vm,
            running: Arc::new(Mutex::new(HashMap::new())),
            next_pid: AtomicU32::new(1),
        }
    }
}

type EventStream = Pin<Box<dyn Stream<Item = Result<StartResponse, Status>> + Send>>;

#[tonic::async_trait]
impl Process for EnvdProcess {
    type ConnectStream = Pin<Box<dyn Stream<Item = Result<ConnectResponse, Status>> + Send>>;
    type StartStream = EventStream;

    async fn list(&self, _request: Request<ListRequest>) -> Result<Response<ListResponse>, Status> {
        let processes = self.running.lock().values().cloned().collect();
        Ok(Response::new(ListResponse { processes }))
    }

    async fn connect(
        &self,
        _request: Request<ConnectRequest>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        Err(Status::unimplemented(
            "Connect (reattaching to a running process) needs live process tracking, which \
             exec_in_guest does not provide yet",
        ))
    }

    async fn start(
        &self,
        request: Request<StartRequest>,
    ) -> Result<Response<Self::StartStream>, Status> {
        let req = request.into_inner();
        let config = req
            .process
            .ok_or_else(|| Status::invalid_argument("process config is required"))?;

        let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);
        self.running.lock().insert(
            pid,
            ProcessInfo {
                config: Some(config.clone()),
                pid,
                tag: req.tag.clone(),
            },
        );

        let vm = Arc::clone(&self.vm);
        let running = Arc::clone(&self.running);
        let cmd = config.cmd;
        let args = config.args;

        let (tx, rx) = tokio::sync::mpsc::channel(4);

        tokio::spawn(async move {
            // Removes this call's entry on every exit path -- the match
            // below has two arms (Ok/Err) that both end the process, and
            // both need this, so it runs once here via a guard rather than
            // being duplicated (and risking one arm forgetting it).
            struct Deregister {
                running: Arc<Mutex<HashMap<u32, ProcessInfo>>>,
                pid: u32,
            }
            impl Drop for Deregister {
                fn drop(&mut self) {
                    self.running.lock().remove(&self.pid);
                }
            }
            let _deregister = Deregister {
                running: Arc::clone(&running),
                pid,
            };

            let _ = tx
                .send(Ok(StartResponse {
                    event: Some(ProcessEvent {
                        event: Some(ProcessEventKind::Start(StartEvent { pid })),
                    }),
                }))
                .await;

            let result = vm.exec_in_guest(&cmd, &args, Duration::from_secs(30)).await;
            match result {
                Ok(exec) => {
                    if !exec.stdout.is_empty() {
                        let _ = tx
                            .send(Ok(StartResponse {
                                event: Some(ProcessEvent {
                                    event: Some(ProcessEventKind::Data(DataEvent {
                                        output: Some(
                                            process_proto::process_event::data_event::Output::Stdout(
                                                exec.stdout.into_bytes(),
                                            ),
                                        ),
                                    })),
                                }),
                            }))
                            .await;
                    }
                    if !exec.stderr.is_empty() {
                        let _ = tx
                            .send(Ok(StartResponse {
                                event: Some(ProcessEvent {
                                    event: Some(ProcessEventKind::Data(DataEvent {
                                        output: Some(
                                            process_proto::process_event::data_event::Output::Stderr(
                                                exec.stderr.into_bytes(),
                                            ),
                                        ),
                                    })),
                                }),
                            }))
                            .await;
                    }
                    let _ = tx
                        .send(Ok(StartResponse {
                            event: Some(ProcessEvent {
                                event: Some(ProcessEventKind::End(EndEvent {
                                    exit_code: exec.exit_code.unwrap_or(-1),
                                    exited: !exec.timed_out,
                                    status: if exec.timed_out {
                                        "timed_out".to_string()
                                    } else {
                                        "exited".to_string()
                                    },
                                    error: None,
                                })),
                            }),
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(Ok(StartResponse {
                            event: Some(ProcessEvent {
                                event: Some(ProcessEventKind::End(EndEvent {
                                    exit_code: -1,
                                    exited: false,
                                    status: "error".to_string(),
                                    error: Some(e.to_string()),
                                })),
                            }),
                        }))
                        .await;
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }

    async fn update(
        &self,
        _request: Request<UpdateRequest>,
    ) -> Result<Response<UpdateResponse>, Status> {
        Err(Status::unimplemented("PTY resize is not implemented"))
    }

    async fn stream_input(
        &self,
        _request: Request<tonic::Streaming<StreamInputRequest>>,
    ) -> Result<Response<StreamInputResponse>, Status> {
        Err(Status::unimplemented(
            "stdin streaming needs a live process to write to, which exec_in_guest does not \
             expose",
        ))
    }

    async fn send_input(
        &self,
        _request: Request<SendInputRequest>,
    ) -> Result<Response<SendInputResponse>, Status> {
        Err(Status::unimplemented("stdin is not implemented"))
    }

    async fn send_signal(
        &self,
        _request: Request<SendSignalRequest>,
    ) -> Result<Response<SendSignalResponse>, Status> {
        Err(Status::unimplemented(
            "signalling a specific process needs live process tracking, which exec_in_guest \
             does not provide",
        ))
    }

    async fn close_stdin(
        &self,
        _request: Request<CloseStdinRequest>,
    ) -> Result<Response<CloseStdinResponse>, Status> {
        Err(Status::unimplemented("stdin is not implemented"))
    }
}

/// Serve `process.Process` **and** `filesystem.Filesystem` for `vm` on
/// `addr`, until `shutdown` fires -- both on the same port, the way real
/// envd does. One call to this is one sandbox's whole envd, matching its
/// real per-sandbox-daemon shape -- see this module's own doc comment.
pub async fn serve_for_sandbox(
    vm: Arc<AgentVM>,
    addr: std::net::SocketAddr,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), tonic::transport::Error> {
    let process = EnvdProcess::new(Arc::clone(&vm));
    let filesystem = crate::envd_filesystem::EnvdFilesystem::new(vm);
    tonic::transport::Server::builder()
        .add_service(ProcessServer::new(process))
        .add_service(crate::envd_filesystem::FilesystemServer::new(filesystem))
        .serve_with_shutdown(addr, async {
            let _ = shutdown.await;
        })
        .await
}
