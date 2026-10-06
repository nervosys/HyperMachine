//! Bounded native UDP ingress over one length-framed tunnel per source peer.
use crate::native_budget::NativeSessionBudget;
use crate::{
    native_tcp::NativeTunnel,
    ports::{PortAllocation, PortProtocol},
};
use std::{collections::HashMap, future::Future, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::mpsc,
    task::{Id, JoinSet},
    time::timeout,
};
pub const MAX_UDP_PAYLOAD: usize = 65_507;
#[async_trait::async_trait]
pub trait NativeUdpConnector: Send + Sync {
    /// Resolve and authenticate the current node; the peer cannot select targets.
    async fn open_udp(&self, allocation: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>>;
}
#[derive(Debug, Clone, Copy)]
pub struct UdpRelayLimits {
    max_peers: usize,
    open_timeout: Duration,
    idle_timeout: Duration,
    io_timeout: Duration,
}
impl UdpRelayLimits {
    /// # Errors
    /// Reject zero/unbounded limits: 1–1024 peers, opening/I/O at most 60 seconds,
    /// and outbound peer inactivity at most one hour. Each peer queues eight frames.
    pub fn new(
        max_peers: usize,
        open_timeout: Duration,
        idle_timeout: Duration,
        io_timeout: Duration,
    ) -> io::Result<Self> {
        if !(1..=1024).contains(&max_peers)
            || open_timeout.is_zero()
            || open_timeout > Duration::from_secs(60)
            || idle_timeout.is_zero()
            || idle_timeout > Duration::from_secs(3600)
            || io_timeout.is_zero()
            || io_timeout > Duration::from_secs(60)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid UDP relay limits",
            ));
        }
        Ok(Self {
            max_peers,
            open_timeout,
            idle_timeout,
            io_timeout,
        })
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UdpRelayStats {
    pub admitted: u64,
    pub refused: u64,
    pub dropped_queue: u64,
    pub oversized: u64,
    pub ended: u64,
    pub failed: u64,
    pub cancelled: u64,
}
fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(payload.len() + 2);
    bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}
fn timed_out(_: tokio::time::error::Elapsed) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "UDP session timeout")
}
async fn session(
    connector: Arc<dyn NativeUdpConnector>,
    allocation: PortAllocation,
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    mut messages: mpsc::Receiver<Vec<u8>>,
    limits: UdpRelayLimits,
) -> io::Result<()> {
    let stream = timeout(limits.open_timeout, connector.open_udp(&allocation))
        .await
        .map_err(timed_out)??;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let outbound = async {
        loop {
            match timeout(limits.idle_timeout, messages.recv()).await {
                Ok(Some(payload)) => timeout(limits.io_timeout, writer.write_all(&payload))
                    .await
                    .map_err(timed_out)??,
                Ok(None) | Err(_) => return Ok::<(), io::Error>(()),
            }
        }
    };
    let inbound = async {
        loop {
            let payload = timeout(limits.idle_timeout + limits.io_timeout, async {
                let size = reader.read_u16().await? as usize;
                if size > MAX_UDP_PAYLOAD {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "oversized UDP frame",
                    ));
                }
                let mut bytes = vec![0; size];
                reader.read_exact(&mut bytes).await?;
                Ok::<_, io::Error>(bytes)
            })
            .await
            .map_err(timed_out)??;
            let sent = timeout(limits.io_timeout, socket.send_to(&payload, peer))
                .await
                .map_err(timed_out)??;
            if sent != payload.len() {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "partial UDP send"));
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), io::Error>(())
    };
    tokio::select! { result = outbound => result, result = inbound => result }
}
/// Serve one native UDP reservation. Peers are full socket addresses, each with
/// an isolated bounded queue/tunnel. Excess peers/frames are dropped, as UDP
/// permits. Complete malformed/oversized frames terminate only their session.
/// Explicit shutdown aborts and joins every peer; dropping aborts owned tasks.
/// # Errors
/// Invalid reservation/socket identity or fatal datagram receive failure.
pub async fn serve_udp(
    socket: UdpSocket,
    allocation: PortAllocation,
    connector: Arc<dyn NativeUdpConnector>,
    limits: UdpRelayLimits,
    shutdown: impl Future<Output = ()>,
) -> io::Result<UdpRelayStats> {
    serve_udp_with_budget(
        socket,
        allocation,
        connector,
        limits,
        NativeSessionBudget::new(limits.max_peers)?,
        shutdown,
    )
    .await
}
/// Serve UDP using an admission budget shared with other native listeners.
/// # Errors
/// As `serve_udp`; exhausted aggregate capacity drops a new peer's datagram.
pub async fn serve_udp_with_budget(
    socket: UdpSocket,
    allocation: PortAllocation,
    connector: Arc<dyn NativeUdpConnector>,
    limits: UdpRelayLimits,
    budget: NativeSessionBudget,
    shutdown: impl Future<Output = ()>,
) -> io::Result<UdpRelayStats> {
    allocation
        .validate()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid UDP reservation"))?;
    if !matches!(
        allocation.protocol(),
        PortProtocol::Udp | PortProtocol::Both
    ) || socket.local_addr()?.port() != allocation.public_port()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "UDP socket does not match reservation",
        ));
    }
    let socket = Arc::new(socket);
    let mut peers: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut tasks = JoinSet::new();
    let mut task_peers: HashMap<Id, SocketAddr> = HashMap::new();
    let mut stats = UdpRelayStats::default();
    let mut receive_error = None;
    let mut buffer = vec![0; MAX_UDP_PAYLOAD + 1];
    tokio::pin!(shutdown);
    loop {
        tokio::select! { biased;
            _ = &mut shutdown => break,
            completed = tasks.join_next_with_id(), if !tasks.is_empty() => {
                if let Some(completed) = completed {
                    let id = match completed {
                        Ok((id, Ok(()))) => { stats.ended += 1; id }
                        Ok((id, Err(_))) => { stats.failed += 1; id }
                        Err(error) => { stats.failed += 1; error.id() }
                    };
                    if let Some(peer) = task_peers.remove(&id) { peers.remove(&peer); }
                }
            }
            received = socket.recv_from(&mut buffer) => {
                let (size, peer) = match received { Ok(value) => value, Err(error) => { receive_error = Some(error); break; } };
                if size > MAX_UDP_PAYLOAD { stats.oversized += 1; continue; }
                if let Some(sender) = peers.get(&peer) {
                    if sender.try_send(frame(&buffer[..size])).is_err() { stats.dropped_queue += 1; }
                    continue;
                }
                if peers.len() >= limits.max_peers { stats.refused += 1; continue; }
                let Some(permit) = budget.try_acquire() else { stats.refused += 1; continue; };
                let (sender, receiver) = mpsc::channel(8);
                sender.try_send(frame(&buffer[..size])).map_err(|_| io::Error::other("new UDP peer queue failed"))?;
                peers.insert(peer, sender); stats.admitted += 1;
                let connector = connector.clone(); let allocation = allocation.clone(); let socket = socket.clone();
                let task = tasks.spawn(async move { let _permit = permit; session(connector, allocation, socket, peer, receiver, limits).await });
                task_peers.insert(task.id(), peer);
            }
        }
    }
    peers.clear();
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => stats.ended += 1,
            Err(error) if error.is_cancelled() => stats.cancelled += 1,
            _ => stats.failed += 1,
        }
    }
    drop(socket);
    if let Some(error) = receive_error {
        return Err(error);
    }
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{oneshot, Mutex, Notify};
    struct Echo {
        tasks: Mutex<JoinSet<()>>,
        mode: AtomicUsize,
        opened: AtomicUsize,
        notify: Notify,
    }
    impl Echo {
        fn new(mode: usize) -> Arc<Self> {
            Arc::new(Self {
                tasks: Mutex::new(JoinSet::new()),
                mode: AtomicUsize::new(mode),
                opened: AtomicUsize::new(0),
                notify: Notify::new(),
            })
        }
        async fn wait(&self, count: usize) {
            timeout(Duration::from_secs(3), async {
                loop {
                    let notified = self.notify.notified();
                    if self.opened.load(Ordering::SeqCst) >= count {
                        break;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap();
        }
        async fn close(&self) {
            let mut tasks = self.tasks.lock().await;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
    }
    #[async_trait::async_trait]
    impl NativeUdpConnector for Echo {
        async fn open_udp(&self, _: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            self.notify.notify_one();
            match self.mode.load(Ordering::SeqCst) {
                1 => return Err(io::Error::other("owned failure")),
                2 => return std::future::pending().await,
                3 => panic!("owned panic"),
                _ => {}
            }
            let mode = self.mode.load(Ordering::SeqCst);
            let (client, mut server) = tokio::io::duplex(131072);
            self.tasks.lock().await.spawn(async move {
                if mode == 4 {
                    let _ = server.write_all(&[255, 255]).await;
                    return;
                }
                if mode == 5 {
                    let _ = server.write_all(&[0]).await;
                    return;
                }
                while let Ok(size) = server.read_u16().await {
                    let mut bytes = vec![0; size as usize];
                    if server.read_exact(&mut bytes).await.is_err()
                        || server.write_all(&frame(&bytes)).await.is_err()
                    {
                        break;
                    }
                }
            });
            Ok(Box::new(client))
        }
    }
    fn row(port: u16) -> PortAllocation {
        PortAllocation {
            sandbox_id: "owned-vm".into(),
            machine_port: 5353,
            public_port: port,
            owner_id: "owner-a".into(),
            protocol: PortProtocol::Both,
        }
    }
    struct OwnedTask<T>(tokio::task::JoinHandle<T>);
    impl<T> Future for OwnedTask<T> {
        type Output = Result<T, tokio::task::JoinError>;
        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            std::pin::Pin::new(&mut self.get_mut().0).poll(cx)
        }
    }
    impl<T> Drop for OwnedTask<T> {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn start(
        echo: Arc<Echo>,
        limits: UdpRelayLimits,
        ipv6: bool,
    ) -> (
        SocketAddr,
        oneshot::Sender<()>,
        OwnedTask<io::Result<UdpRelayStats>>,
    ) {
        let socket = crate::udp_socket::bind(if ipv6 { "[::1]:0" } else { "127.0.0.1:0" })
            .await
            .unwrap();
        let address = socket.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = OwnedTask(tokio::spawn(serve_udp(
            socket,
            row(address.port()),
            echo,
            limits,
            async {
                let _ = stopped.await;
            },
        )));
        (address, stop, task)
    }
    async fn exchange(client: &UdpSocket, address: SocketAddr, payload: &[u8]) {
        client.send_to(payload, address).await.unwrap();
        let mut bytes = vec![0; MAX_UDP_PAYLOAD + 1];
        let (size, source) = timeout(Duration::from_secs(3), client.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source, address);
        assert_eq!(&bytes[..size], payload);
    }
    #[tokio::test]
    async fn exact_peer_datagrams_ipv4_ipv6_and_cap_refusal() {
        for ipv6 in [false, true] {
            let echo = Echo::new(0);
            let limits = UdpRelayLimits::new(
                2,
                Duration::from_secs(1),
                Duration::from_secs(10),
                Duration::from_secs(1),
            )
            .unwrap();
            let (address, stop, task) = start(echo.clone(), limits, ipv6).await;
            let a = crate::udp_socket::bind(SocketAddr::new(address.ip(), 0))
                .await
                .unwrap();
            let b = crate::udp_socket::bind(SocketAddr::new(address.ip(), 0))
                .await
                .unwrap();
            for (left, right) in [
                (Vec::new(), vec![0, 255, 13, 10]),
                (vec![7; MAX_UDP_PAYLOAD], vec![9; MAX_UDP_PAYLOAD]),
            ] {
                tokio::join!(exchange(&a, address, &left), exchange(&b, address, &right));
            }
            let third = crate::udp_socket::bind(SocketAddr::new(address.ip(), 0))
                .await
                .unwrap();
            third.send_to(b"capped", address).await.unwrap();
            assert!(timeout(Duration::from_millis(30), third.recv(&mut [0; 16]))
                .await
                .is_err());
            assert_eq!(echo.opened.load(Ordering::SeqCst), 2);
            stop.send(()).unwrap();
            let stats = task.await.unwrap().unwrap();
            assert_eq!(stats.admitted, 2);
            assert_eq!(stats.refused, 1);
            assert_eq!(stats.cancelled, 2);
            let _released = crate::udp_socket::bind(address).await.unwrap();
            echo.close().await;
        }
    }
    #[tokio::test]
    async fn failed_sessions_release_same_peer_slot() {
        for mode in [1, 2, 3, 4, 5] {
            let echo = Echo::new(mode);
            let limits = UdpRelayLimits::new(
                1,
                Duration::from_millis(30),
                Duration::from_secs(5),
                Duration::from_secs(1),
            )
            .unwrap();
            let (address, stop, task) = start(echo.clone(), limits, false).await;
            let client = crate::udp_socket::bind("127.0.0.1:0").await.unwrap();
            client.send_to(b"failure", address).await.unwrap();
            echo.wait(1).await;
            // Observe no reply across the bounded opening failure, then verify
            // that the same peer can establish a fresh session.
            assert!(
                timeout(Duration::from_millis(60), client.recv(&mut [0; 16]))
                    .await
                    .is_err()
            );
            echo.mode.store(0, Ordering::SeqCst);
            exchange(&client, address, b"recovered\0").await;
            echo.wait(2).await;
            stop.send(()).unwrap();
            let stats = task.await.unwrap().unwrap();
            assert_eq!(stats.admitted, 2);
            assert_eq!(stats.failed, 1);
            assert_eq!(stats.cancelled, 1);
            echo.close().await;
        }
    }
    #[tokio::test]
    async fn idle_sessions_expire_and_owner_cancellation_releases_socket() {
        let echo = Echo::new(0);
        let limits = UdpRelayLimits::new(
            1,
            Duration::from_secs(1),
            Duration::from_millis(30),
            Duration::from_secs(1),
        )
        .unwrap();
        let (address, stop, task) = start(echo.clone(), limits, false).await;
        let client = crate::udp_socket::bind("127.0.0.1:0").await.unwrap();
        exchange(&client, address, b"idle").await;
        timeout(Duration::from_millis(60), std::future::pending::<()>())
            .await
            .unwrap_err();
        exchange(&client, address, b"new").await;
        echo.wait(2).await;
        stop.send(()).unwrap();
        let stats = task.await.unwrap().unwrap();
        assert_eq!(stats.admitted, 2);
        assert!(stats.ended >= 1);
        echo.close().await;
        let echo = Echo::new(0);
        let (address, _stop, task) = start(echo.clone(), limits, false).await;
        exchange(&client, address, b"live").await;
        task.0.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        echo.close().await;
        // Await owned peer teardown before testing exact release.
        let _released = timeout(Duration::from_secs(2), async {
            loop {
                match crate::udp_socket::bind(address).await {
                    Ok(socket) => break socket,
                    Err(_) => tokio::task::yield_now().await,
                }
            }
        })
        .await
        .unwrap();
        assert!(UdpRelayLimits::new(
            0,
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1)
        )
        .is_err());
    }
    #[tokio::test]
    async fn pressure_is_dropped_and_oversized_ipv6_ingress_never_opens_a_peer() {
        let echo = Echo::new(2);
        let limits = UdpRelayLimits::new(
            1,
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(1),
        )
        .unwrap();
        let (address, stop, task) = start(echo.clone(), limits, true).await;
        let client = crate::udp_socket::bind("[::1]:0").await.unwrap();
        client
            .send_to(&vec![0; MAX_UDP_PAYLOAD + 1], address)
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_millis(30), client.recv(&mut [0; 16]))
                .await
                .is_err()
        );
        assert_eq!(echo.opened.load(Ordering::SeqCst), 0);
        client.send_to(b"opening", address).await.unwrap();
        echo.wait(1).await;
        for _ in 0..32 {
            client.send_to(b"queued", address).await.unwrap();
        }
        assert!(
            timeout(Duration::from_millis(100), client.recv(&mut [0; 16]))
                .await
                .is_err()
        );
        stop.send(()).unwrap();
        let stats = task.await.unwrap().unwrap();
        assert_eq!(stats.oversized, 1);
        assert_eq!(stats.admitted, 1);
        assert!(stats.dropped_queue > 0);
        assert_eq!(stats.cancelled, 1);
        echo.close().await;
    }
    #[tokio::test]
    async fn wrong_reservations_refuse_dispatch() {
        let echo = Echo::new(0);
        let limits = UdpRelayLimits::new(
            1,
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        for tcp in [false, true] {
            let socket = crate::udp_socket::bind("127.0.0.1:0").await.unwrap();
            let address = socket.local_addr().unwrap();
            let mut allocation = row(address.port());
            if tcp {
                allocation.protocol = PortProtocol::Tcp;
            } else {
                allocation.public_port = if address.port() == 65535 {
                    address.port() - 1
                } else {
                    address.port() + 1
                };
            }
            assert!(serve_udp(
                socket,
                allocation,
                echo.clone(),
                limits,
                std::future::pending()
            )
            .await
            .is_err());
            let _released = crate::udp_socket::bind(address).await.unwrap();
        }
        assert_eq!(echo.opened.load(Ordering::SeqCst), 0);
    }
}
