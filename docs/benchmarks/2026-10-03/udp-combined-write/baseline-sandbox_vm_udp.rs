//! Bounded loopback UDP forwarding with one upstream session per local peer.
use super::*;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::mpsc, task::JoinSet};
const MAX_PAYLOAD: usize = 65_507;

pub(super) async fn run(api: Api, id: String, port: u16, listen: SocketAddr, max_peers: u32) -> Result<()> {
    if !listen.ip().is_loopback() { bail!("UDP listener must use a loopback address"); }
    // Validate the identifier before publishing a listener.
    api.url(&["sandboxes", &id])?;
    let socket = Arc::new(tokio::net::UdpSocket::bind(listen).await?);
    println!("{}", json!({"listen": socket.local_addr()?.to_string(), "protocol":"udp", "sandboxID":id, "port":port}));
    let mut peers: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut sessions = JoinSet::new();
    let mut buffer = vec![0; MAX_PAYLOAD + 1];
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(Ok(peer)) = completed { peers.remove(&peer); }
            }
            received = socket.recv_from(&mut buffer) => {
                let (size, peer) = received?;
                if size > MAX_PAYLOAD { continue; }
                if let Some(sender) = peers.get(&peer) {
                    // UDP can drop under pressure; never grow an unbounded queue.
                    let _ = sender.try_send(buffer[..size].to_vec());
                    continue;
                }
                if peers.len() >= max_peers as usize { continue; }
                let (sender, receiver) = mpsc::channel(8);
                sender.try_send(buffer[..size].to_vec())?;
                peers.insert(peer, sender);
                let api = api.clone(); let id = id.clone(); let socket = socket.clone();
                sessions.spawn(async move {
                    if let Err(error) = session(api, id, port, socket, peer, receiver).await {
                        eprintln!("UDP peer session closed: {error}");
                    }
                    peer
                });
            }
        }
    }
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
    Ok(())
}

async fn session(api: Api, id: String, port: u16, socket: Arc<tokio::net::UdpSocket>, peer: SocketAddr,
    mut messages: mpsc::Receiver<Vec<u8>>) -> Result<()> {
    let stream = api.port_tunnel(&id, port, true).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let outbound = async {
        while let Some(payload) = tokio::time::timeout(Duration::from_secs(30), messages.recv()).await? {
            tokio::time::timeout(Duration::from_secs(5), async {
                writer.write_all(&(payload.len() as u16).to_be_bytes()).await?;
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
