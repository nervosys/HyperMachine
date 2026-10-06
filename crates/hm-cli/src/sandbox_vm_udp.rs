//! Bounded loopback UDP forwarding with one upstream session per local peer.
use super::*;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::{Id, JoinError, JoinSet},
};
const MAX_PAYLOAD: usize = 65_507;

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(payload.len() + 2);
    bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

pub(super) async fn run(
    api: Api,
    id: String,
    port: u16,
    listen: SocketAddr,
    max_peers: u32,
    ipv6: bool,
) -> Result<()> {
    if !listen.ip().is_loopback() {
        bail!("UDP listener must use a loopback address");
    }
    // Validate the identifier before publishing a listener.
    api.url(&["sandboxes", &id])?;
    let socket = Arc::new(tokio::net::UdpSocket::bind(listen).await?);
    println!(
        "{}",
        json!({"listen": socket.local_addr()?.to_string(), "protocol":"udp", "sandboxID":id, "port":port})
    );
    let mut peers: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut sessions = JoinSet::new();
    let mut task_peers = HashMap::new();
    let mut buffer = vec![0; MAX_PAYLOAD + 1];
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            completed = sessions.join_next_with_id(), if !sessions.is_empty() => {
                if let Some(completed) = completed { release_peer(&mut peers, &mut task_peers, completed); }
            }
            received = socket.recv_from(&mut buffer) => {
                let (size, peer) = received?;
                if size > MAX_PAYLOAD { continue; }
                if let Some(sender) = peers.get(&peer) {
                    // UDP can drop under pressure; never grow an unbounded queue.
                    let _ = sender.try_send(frame(&buffer[..size]));
                    continue;
                }
                if peers.len() >= max_peers as usize { continue; }
                let (sender, receiver) = mpsc::channel(8);
                sender.try_send(frame(&buffer[..size]))?;
                peers.insert(peer, sender);
                let api = api.clone(); let id = id.clone(); let socket = socket.clone();
                let task = sessions.spawn(async move {
                    if let Err(error) = session(api, id, port, socket, peer, receiver, ipv6).await {
                        eprintln!("UDP peer session closed: {error}");
                    }
                });
                task_peers.insert(task.id(), peer);
            }
        }
    }
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
    Ok(())
}

// Task identity remains available when a session panics or is cancelled.
// Removing only successful results would permanently consume its peer slot.
fn release_peer(
    peers: &mut HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>,
    task_peers: &mut HashMap<Id, SocketAddr>,
    completed: std::result::Result<(Id, ()), JoinError>,
) {
    let task = match completed {
        Ok((task, ())) => task,
        Err(error) => error.id(),
    };
    if let Some(peer) = task_peers.remove(&task) {
        peers.remove(&peer);
    }
}

async fn session(
    api: Api,
    id: String,
    port: u16,
    socket: Arc<tokio::net::UdpSocket>,
    peer: SocketAddr,
    mut messages: mpsc::Receiver<Vec<u8>>,
    ipv6: bool,
) -> Result<()> {
    let stream = api.port_tunnel(&id, port, true, ipv6).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let outbound = async {
        while let Some(payload) =
            tokio::time::timeout(Duration::from_secs(30), messages.recv()).await?
        {
            tokio::time::timeout(Duration::from_secs(5), async {
                writer.write_all(&payload).await
            })
            .await??;
        }
        Ok::<(), anyhow::Error>(())
    };
    let inbound = async {
        loop {
            let payload = tokio::time::timeout(Duration::from_secs(35), async {
                let size = reader.read_u16().await? as usize;
                if size > MAX_PAYLOAD {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "oversized UDP frame",
                    ));
                }
                let mut payload = vec![0; size];
                reader.read_exact(&mut payload).await?;
                Ok::<_, std::io::Error>(payload)
            })
            .await??;
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
        let mut task_peers = HashMap::new();
        let mut tasks = JoinSet::new();
        let panicked = tasks.spawn(async { panic!("owned session failure") });
        let cancelled = tasks.spawn(std::future::pending::<()>());
        let completed = tasks.spawn(async {});
        for (index, task) in [panicked, cancelled.clone(), completed]
            .into_iter()
            .enumerate()
        {
            let address = SocketAddr::from(([127, 0, 0, 1], 30000 + index as u16));
            let (sender, _receiver) = mpsc::channel(8);
            peers.insert(address, sender);
            task_peers.insert(task.id(), address);
        }
        cancelled.abort();
        let mut panics = 0;
        let mut cancellations = 0;
        while let Some(result) = tasks.join_next_with_id().await {
            if let Err(error) = &result {
                panics += usize::from(error.is_panic());
                cancellations += usize::from(error.is_cancelled());
            }
            release_peer(&mut peers, &mut task_peers, result);
        }
        assert_eq!((panics, cancellations), (1, 1));
        assert!(
            peers.is_empty(),
            "failed sessions must not consume peer capacity"
        );
        assert!(task_peers.is_empty(), "task identities must not leak");
    }
}
