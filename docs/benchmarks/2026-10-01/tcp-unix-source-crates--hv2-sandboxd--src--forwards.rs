//! A sandbox's own ports, reached through the proxy: E2B's
//! `sandbox.get_host(port)` -- a web server, a dev server, the Code
//! Interpreter's Jupyter -- at `{port}-{sandboxID}.{domain}`.
//!
//! The first request for a port makes a loopback listener for it, which
//! the proxy is routed to like envd's. Each connection it accepts is
//! carried into the guest over a vsock connection of its own: the agent
//! connects to the port there and copies bytes both ways
//! ([`hv2_guest_agent::Operation::Forward`]). No network interface is
//! involved, so a sandbox without one serves its ports all the same, and
//! nothing reaches a guest port unless a request through the proxy -- with
//! the proxy's own checks -- asked for it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use hv2_agent::{AgentVM, VsockStream};

use super::{AppState, Arc};

/// How long the guest has to answer that its port is open.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);

/// Open the guest port before accepting an API upgrade. Holding the lifecycle
/// lock through registration prevents pause/delete from missing this stream.
pub(crate) async fn tcp_tunnel(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path((sandbox, port)): axum::extract::Path<(String, u16)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::http::StatusCode;
    if let Err(message) = hv2_api::tcp_tunnel::validate(&request) {
        return super::api_error(StatusCode::BAD_REQUEST, message);
    }
    if port == 0 {
        return super::api_error(StatusCode::BAD_REQUEST, "guest port must be nonzero");
    }
    // Unknown names must not allocate entries in the lifecycle lock table.
    if !state.sandboxes.lock().contains_key(&sandbox) {
        return super::api_error(StatusCode::NOT_FOUND, "sandbox is not running on this node");
    }
    let lock = super::transition_lock(&state, &sandbox);
    let _held = lock.lock().await;
    let vm = state.sandboxes.lock().get(&sandbox).map(|live| {
        (
            Arc::clone(&live.vm),
            super::ActivityGuard::enter(&live.activity),
        )
    });
    let Some((vm, activity)) = vm else {
        return super::api_error(StatusCode::NOT_FOUND, "sandbox is not running on this node");
    };
    let (stream, early) = match vm.forward_port(port, FORWARD_TIMEOUT).await {
        Ok(opened) => opened,
        Err(error) => {
            tracing::debug!(%sandbox, port, %error, "guest TCP port unavailable");
            return super::api_error(StatusCode::BAD_GATEWAY, "guest TCP port unavailable");
        }
    };
    // Reuse the blocking vsock relay behind an anonymous socket pair on Unix.
    // Other hosts use a temporary loopback TCP pair. No address is published.
    #[cfg(unix)]
    let pair = tokio::net::UnixStream::pair().and_then(|(client, server)| {
        let server = server.into_std()?;
        server.set_nonblocking(false)?;
        Ok((client, server))
    });
    #[cfg(not(unix))]
    let pair = async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let client = tokio::net::TcpStream::connect(listener.local_addr()?).await?;
        let (server, _) = listener.accept().await?;
        let server = server.into_std()?;
        server.set_nonblocking(false)?;
        server.set_nodelay(true)?;
        Ok::<_, std::io::Error>((client, server))
    }
    .await;
    let (client, server) = match pair {
        Ok(pair) => pair,
        Err(error) => {
            stream.close();
            return super::api_error(StatusCode::INTERNAL_SERVER_ERROR, error);
        }
    };
    state
        .forwards
        .open
        .lock()
        .entry(sandbox.clone())
        .or_default()
        .push(stream.clone());
    let relay_state = Arc::clone(&state);
    tokio::spawn(async move {
        let _active = activity;
        carry_socket(&relay_state, &sandbox, stream, early, server).await;
    });
    hv2_api::tcp_tunnel::accept(request, client)
}

#[derive(Default)]
pub(crate) struct Forwards {
    listeners: parking_lot::Mutex<HashMap<(String, u16), (SocketAddr, tokio::task::AbortHandle)>>,
    /// Connections carried now, by sandbox, to close when it stops.
    open: parking_lot::Mutex<HashMap<String, Vec<VsockStream>>>,
}

/// The listener for `port` of `sandbox`: made now if there is none, and
/// the proxy routed to it. `None` if the sandbox is not running here.
pub(crate) async fn listen(state: &Arc<AppState>, sandbox: &str, port: u16) -> Option<SocketAddr> {
    let key = (sandbox.to_string(), port);
    if let Some((addr, _)) = state.forwards.listeners.lock().get(&key) {
        return Some(*addr);
    }
    let vm = state
        .sandboxes
        .lock()
        .get(sandbox)
        .map(|live| Arc::clone(&live.vm))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    let addr = listener.local_addr().ok()?;
    let accepting = {
        let state = Arc::clone(state);
        let sandbox = sandbox.to_string();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let vm = Arc::clone(&vm);
                let state = Arc::clone(&state);
                let sandbox = sandbox.clone();
                tokio::spawn(async move { carry(&state, &sandbox, &vm, port, tcp).await });
            }
        })
    };
    let mut listeners = state.forwards.listeners.lock();
    // Two first requests at once: the first listener made stays.
    if let Some((existing, _)) = listeners.get(&key) {
        accepting.abort();
        return Some(*existing);
    }
    listeners.insert(key, (addr, accepting.abort_handle()));
    drop(listeners);
    state.routes.insert(sandbox, port, addr);
    Some(addr)
}

/// One connection: into the guest over vsock, and back.
async fn carry(
    state: &AppState,
    sandbox: &str,
    vm: &AgentVM,
    port: u16,
    tcp: tokio::net::TcpStream,
) {
    let lock = super::transition_lock(state, sandbox);
    let held = lock.lock().await;
    let current = state
        .sandboxes
        .lock()
        .get(sandbox)
        .is_some_and(|live| std::ptr::eq(live.vm.as_ref(), vm));
    if !current {
        return;
    }
    let (stream, early) = match vm.forward_port(port, FORWARD_TIMEOUT).await {
        Ok(opened) => opened,
        Err(e) => {
            tracing::debug!("{sandbox}: port {port}: {e}");
            return; // the proxy answers 502 as the connection closes
        }
    };
    state
        .forwards
        .open
        .lock()
        .entry(sandbox.to_string())
        .or_default()
        .push(stream.clone());
    drop(held);
    carry_opened(state, sandbox, stream, early, tcp).await;
}

async fn carry_opened(
    state: &AppState,
    sandbox: &str,
    stream: VsockStream,
    early: Vec<u8>,
    tcp: tokio::net::TcpStream,
) {
    let Ok(tcp) = tcp.into_std() else {
        stream.close();
        return;
    };
    if tcp.set_nonblocking(false).is_err() {
        stream.close();
        return;
    }
    let _ = tcp.set_nodelay(true);
    carry_socket(state, sandbox, stream, early, tcp).await;
}

async fn carry_socket<S: RelaySocket>(
    state: &AppState,
    sandbox: &str,
    stream: VsockStream,
    early: Vec<u8>,
    tcp: S,
) {
    let done = tokio::task::spawn_blocking(move || splice(&stream, tcp, &early));
    let _ = done.await;
    // Forget streams that have closed; each is closed once both sides are.
    if let Some(open) = state.forwards.open.lock().get_mut(sandbox) {
        open.retain(VsockStream::is_open);
    }
}

/// Bytes both ways between the proxy's connection and the guest's, until
/// either side is done.
fn splice<S: RelaySocket>(stream: &VsockStream, tcp: S, early: &[u8]) {
    let Ok(mut to_proxy) = tcp.clone_socket() else {
        return;
    };
    let mut from_proxy = tcp;
    if !early.is_empty() && to_proxy.write_all(early).is_err() {
        return;
    }
    let upstream = {
        let stream = stream.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match from_proxy.read(&mut buf) {
                    Ok(0) => {
                        let _ = stream.shutdown_write();
                        return;
                    }
                    Err(_) => break,
                    Ok(n) => {
                        if stream.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            // An error aborts the relay; orderly EOF only ends its direction.
            stream.close();
        })
    };
    let clean_eof = loop {
        match stream.read() {
            Ok(data) if !data.is_empty() => {
                if to_proxy.write_all(&data).is_err() {
                    break false;
                }
            }
            Ok(_) => break true,
            Err(_) => break false,
        }
    };
    if clean_eof && stream.is_open() {
        let _ = to_proxy.shutdown(std::net::Shutdown::Write);
    } else {
        let _ = to_proxy.shutdown(std::net::Shutdown::Both);
        stream.close();
    }
    let _ = upstream.join();
    let _ = to_proxy.shutdown(std::net::Shutdown::Both);
    stream.close();
}

trait RelaySocket: std::io::Read + std::io::Write + Send + Sized + 'static {
    fn clone_socket(&self) -> std::io::Result<Self>;
    fn shutdown(&self, direction: std::net::Shutdown) -> std::io::Result<()>;
}

impl RelaySocket for std::net::TcpStream {
    fn clone_socket(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
    fn shutdown(&self, direction: std::net::Shutdown) -> std::io::Result<()> {
        std::net::TcpStream::shutdown(self, direction)
    }
}

#[cfg(unix)]
impl RelaySocket for std::os::unix::net::UnixStream {
    fn clone_socket(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
    fn shutdown(&self, direction: std::net::Shutdown) -> std::io::Result<()> {
        std::os::unix::net::UnixStream::shutdown(self, direction)
    }
}

/// A sandbox stopped or paused: its listeners and every connection close.
pub(crate) fn stop(state: &AppState, sandbox: &str) {
    state
        .forwards
        .listeners
        .lock()
        .retain(|(id, _), (_, accepting)| {
            if id == sandbox {
                accepting.abort();
                false
            } else {
                true
            }
        });
    if let Some(open) = state.forwards.open.lock().remove(sandbox) {
        for stream in open {
            stream.close();
        }
    }
}
