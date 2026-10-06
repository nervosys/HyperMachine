//! Bounded loopback UDP forwarding with one upstream session per local peer.
use super::*;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::mpsc, task::{Id, JoinError, JoinSet}};
const MAX_PAYLOAD: usize = 65_507;

struct PeerSession {
    task: Id,
    sender: mpsc::Sender<Vec<u8>>,
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(payload.len() + 2);
    bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

pub(super) async fn run(api: Api, id: String, port: u16, listen: SocketAddr, max_peers: u32) -> Result<()> {
    if !listen.ip().is_loopback() { bail!("UDP listener must use a loopback address"); }
    // Validate the identifier before publishing a listener.
    api.url(&["sandboxes", &id])?;
    let socket = Arc::new(tokio::net::UdpSocket::bind(listen).await?);
    println!("{}", json!({"listen": socket.local_addr()?.to_string(), "protocol":"udp", "sandboxID":id, "port":port}));
    let mut peers: HashMap<SocketAddr, PeerSession> = HashMap::new();
    let mut sessions = JoinSet::new();
    let mut buffer = vec![0; MAX_PAYLOAD + 1];
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(completed) = completed { release_peer(&mut peers, completed); }
            }
            received = socket.recv_from(&mut buffer) => {
                let (size, peer) = received?;
                if size > MAX_PAYLOAD { continue; }
                if let Some(session) = peers.get(&peer) {
                    // UDP can drop under pressure; never grow an unbounded queue.
                    let _ = session.sender.try_send(frame(&buffer[..size]));
                    continue;
                }
                if peers.len() >= max_peers as usize { continue; }
                let (sender, receiver) = mpsc::channel(8);
                sender.try_send(frame(&buffer[..size]))?;
                let api = api.clone(); let id = id.clone(); let socket = socket.clone();
                let task = sessions.spawn(async move {
                    if let Err(error) = session(api, id, port, socket, peer, receiver).await {
                        eprintln!("UDP peer session closed: {error}");
                    }
                    peer
                });
                peers.insert(peer, PeerSession { task: task.id(), sender });
            }
        }
    }
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
    Ok(())
}

// Successful joins return their peer directly. Only a failed join scans the
// bounded peer map for its task ID, keeping separate task bookkeeping unnecessary.
fn release_peer(peers: &mut HashMap<SocketAddr, PeerSession>,
    completed: std::result::Result<SocketAddr, JoinError>) {
    let peer = match completed {
        Ok(peer) => Some(peer),
        Err(error) => peers.iter().find_map(|(peer, session)|
            (session.task == error.id()).then_some(*peer)),
    };
    if let Some(peer) = peer { peers.remove(&peer); }
}

async fn session(api: Api, id: String, port: u16, socket: Arc<tokio::net::UdpSocket>, peer: SocketAddr,
    mut messages: mpsc::Receiver<Vec<u8>>) -> Result<()> {
    let stream = api.port_tunnel(&id, port, true).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let outbound = async {
        while let Some(payload) = tokio::time::timeout(Duration::from_secs(30), messages.recv()).await? {
            tokio::time::timeout(Duration::from_secs(5), async {
                writer.write_all(&payload).await
            }).await??;
        }
        Ok::<(), anyhow::Error>(())
    };
    let inbound = async {
        loop {
            let payload = tokio::time::timeout(Duration::from_secs(35), async {
                let size = reader.read_u16().await? as usize;
                if size > MAX_PAYLOAD { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "oversized UDP frame")); }
                let mut payload = vec![0; size];
                reader.read_exact(&mut payload).await?;
                Ok::<_, std::io::Error>(payload)
            }).await??;
            tokio::time::timeout(Duration::from_secs(5), socket.send_to(&payload, peer)).await??;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    };
    // Either direction ending drops the complete upstream transport.
    tokio::select! { result = outbound => result, result = inbound => result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_tasks_release_peer_capacity() {
        let mut peers = HashMap::new();
        let mut tasks = JoinSet::new();
        let normal_peer = SocketAddr::from(([127, 0, 0, 1], 30002));
        let panicked = tasks.spawn(async { panic!("owned session failure") });
        let cancelled = tasks.spawn(std::future::pending::<SocketAddr>());
        let completed = tasks.spawn(async move { normal_peer });
        for (index, task) in [panicked, cancelled.clone(), completed].into_iter().enumerate() {
            let address = SocketAddr::from(([127, 0, 0, 1], 30000 + index as u16));
            let (sender, _receiver) = mpsc::channel(8);
            peers.insert(address, PeerSession { task: task.id(), sender });
        }
        cancelled.abort();
        let mut panics = 0;
        let mut cancellations = 0;
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = &result {
                panics += usize::from(error.is_panic());
                cancellations += usize::from(error.is_cancelled());
            }
            let before = peers.len();
            release_peer(&mut peers, result);
            assert_eq!(peers.len(), before - 1, "each completion removes only its own peer");
        }
        assert_eq!((panics, cancellations), (1, 1));
        assert!(peers.is_empty(), "failed sessions must not consume peer capacity");
    }
}
