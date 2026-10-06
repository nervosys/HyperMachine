//! Bounded raw TCP forwarding. The connector must authenticate the node tunnel;
//! no target address or credential is accepted from an ingress connection.
use crate::native_budget::NativeSessionBudget;
use crate::ports::{PortAllocation, PortProtocol};
use std::{future::Future, io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    task::JoinSet,
    time::timeout,
};

pub trait NativeTunnel: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> NativeTunnel for T {}
/// Trusted gateway adapter. Implementations must resolve the current sandbox
/// node, enforce lifecycle state and authenticate their upstream connection.
#[async_trait::async_trait]
pub trait NativeTcpConnector: Send + Sync {
    async fn open_tcp(&self, allocation: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>>;
}
#[derive(Debug, Clone, Copy)]
pub struct TcpRelayLimits {
    max_connections: usize,
    open_timeout: Duration,
    max_connection_age: Duration,
}
impl TcpRelayLimits {
    /// # Errors
    /// Rejects unbounded/zero limits: 1–1024 connections, open timeout at most
    /// 60 seconds and connection lifetime at most 24 hours. Lifetime is an
    /// absolute bound, not an idle timeout; deployments must choose accordingly.
    pub fn new(
        max_connections: usize,
        open_timeout: Duration,
        max_connection_age: Duration,
    ) -> io::Result<Self> {
        if !(1..=1024).contains(&max_connections)
            || open_timeout.is_zero()
            || open_timeout > Duration::from_secs(60)
            || max_connection_age.is_zero()
            || max_connection_age > Duration::from_secs(86400)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid TCP relay limits",
            ));
        }
        Ok(Self {
            max_connections,
            open_timeout,
            max_connection_age,
        })
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TcpRelayStats {
    pub admitted: u64,
    pub refused: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
}
/// Serve an already-bound reservation until explicit shutdown. Each ingress
/// stream gets one isolated authenticated upstream tunnel. The bounded task set
/// owns every relay; failure, panic and cancellation release connection slots.
/// Dropping this future drops the listener and aborts its owned task set.
/// # Errors
/// Invalid reservation/listener identity or fatal listener accept failure.
pub async fn serve_tcp(
    listener: TcpListener,
    allocation: PortAllocation,
    connector: Arc<dyn NativeTcpConnector>,
    limits: TcpRelayLimits,
    shutdown: impl Future<Output = ()>,
) -> io::Result<TcpRelayStats> {
    serve_tcp_with_budget(
        listener,
        allocation,
        connector,
        limits,
        NativeSessionBudget::new(limits.max_connections)?,
        shutdown,
    )
    .await
}
/// Serve TCP using an admission budget shared with other native listeners.
/// # Errors
/// As `serve_tcp`; exhausted aggregate capacity refuses the connection.
pub async fn serve_tcp_with_budget(
    listener: TcpListener,
    allocation: PortAllocation,
    connector: Arc<dyn NativeTcpConnector>,
    limits: TcpRelayLimits,
    budget: NativeSessionBudget,
    shutdown: impl Future<Output = ()>,
) -> io::Result<TcpRelayStats> {
    allocation
        .validate()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid TCP reservation"))?;
    if !matches!(
        allocation.protocol(),
        PortProtocol::Tcp | PortProtocol::Both
    ) || listener.local_addr()?.port() != allocation.public_port()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TCP listener does not match reservation",
        ));
    }
    tokio::pin!(shutdown);
    let mut tasks = JoinSet::new();
    let mut stats = TcpRelayStats::default();
    let mut accept_error = None;
    loop {
        tokio::select! { biased;
            _ = &mut shutdown => break,
            result = tasks.join_next(), if !tasks.is_empty() => {
                match result {
                    Some(Ok(true)) => stats.completed += 1,
                    Some(_) => stats.failed += 1,
                    None => {}
                }
            }
            accepted = listener.accept() => {
                let (mut ingress, _) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => { accept_error = Some(error); break; }
                };
                if tasks.len() >= limits.max_connections {
                    stats.refused += 1; drop(ingress); continue;
                }
                let Some(permit) = budget.try_acquire() else { stats.refused += 1; drop(ingress); continue; };
                stats.admitted += 1;
                let connector = connector.clone();
                let allocation = allocation.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let Ok(Ok(mut upstream)) = timeout(limits.open_timeout, connector.open_tcp(&allocation)).await else { return false; };
                    matches!(timeout(limits.max_connection_age,
                        tokio::io::copy_bidirectional(&mut ingress, &mut upstream)).await, Ok(Ok(_)))
                });
            }
        }
    }
    drop(listener);
    tasks.abort_all();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(true) => stats.completed += 1,
            Err(error) if error.is_cancelled() => stats.cancelled += 1,
            _ => stats.failed += 1,
        }
    }
    if let Some(error) = accept_error {
        return Err(error);
    }
    Ok(stats)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        sync::{oneshot, Mutex, Notify},
    };
    struct Echo {
        tasks: Mutex<JoinSet<()>>,
        opened: AtomicUsize,
        notify: Notify,
        mode: AtomicUsize,
    }
    impl Echo {
        fn new(mode: usize) -> Arc<Self> {
            Arc::new(Self {
                tasks: Mutex::new(JoinSet::new()),
                opened: AtomicUsize::new(0),
                notify: Notify::new(),
                mode: AtomicUsize::new(mode),
            })
        }
        async fn wait(&self, count: usize) {
            timeout(Duration::from_secs(5), async {
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
    impl NativeTcpConnector for Echo {
        async fn open_tcp(&self, allocation: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
            assert_eq!(allocation.sandbox_id(), "owned-vm");
            assert_eq!(allocation.machine_port(), 8080);
            self.opened.fetch_add(1, Ordering::SeqCst);
            self.notify.notify_one();
            match self.mode.load(Ordering::SeqCst) {
                1 => return Err(io::Error::other("owned fixture failure")),
                2 => return std::future::pending().await,
                3 => panic!("owned fixture panic"),
                _ => {}
            }
            let (client, mut server) = tokio::io::duplex(8192);
            self.tasks.lock().await.spawn(async move {
                let mut bytes = [0; 8192];
                while let Ok(n) = server.read(&mut bytes).await {
                    if n == 0 {
                        break;
                    }
                    if server.write_all(&bytes[..n]).await.is_err() {
                        break;
                    }
                }
                let _ = server.shutdown().await;
            });
            Ok(Box::new(client))
        }
    }
    fn row(port: u16) -> PortAllocation {
        PortAllocation {
            sandbox_id: "owned-vm".into(),
            machine_port: 8080,
            public_port: port,
            owner_id: "owner-a".into(),
            protocol: PortProtocol::Tcp,
        }
    }
    async fn start(
        echo: Arc<Echo>,
        limits: TcpRelayLimits,
    ) -> (
        std::net::SocketAddr,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<io::Result<TcpRelayStats>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(serve_tcp(
            listener,
            row(address.port()),
            echo,
            limits,
            async {
                let _ = stopped.await;
            },
        ));
        (address, stop, task)
    }
    async fn closed(client: &mut TcpStream) {
        let mut byte = [0];
        let result = timeout(Duration::from_secs(3), client.read(&mut byte))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0)) || result.is_err());
    }
    #[tokio::test]
    async fn exact_binary_half_close_and_shutdown_cleanup() {
        let echo = Echo::new(0);
        let limits =
            TcpRelayLimits::new(2, Duration::from_secs(1), Duration::from_secs(10)).unwrap();
        let (address, stop, task) = start(echo.clone(), limits).await;
        let client = TcpStream::connect(address).await.unwrap();
        let (mut reader, mut writer) = client.into_split();
        let payload: Vec<u8> = (0..1_048_576).map(|n| (n % 251) as u8).collect();
        let (sent, received) = tokio::join!(
            async {
                writer.write_all(&payload).await.unwrap();
                writer.shutdown().await
            },
            async {
                let mut result = Vec::new();
                reader.read_to_end(&mut result).await.unwrap();
                result
            }
        );
        sent.unwrap();
        assert_eq!(received, payload);
        let mut live = TcpStream::connect(address).await.unwrap();
        echo.wait(2).await;
        live.write_all(b"live\0").await.unwrap();
        let mut bytes = [0; 5];
        live.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"live\0");
        stop.send(()).unwrap();
        let stats = task.await.unwrap().unwrap();
        assert_eq!(stats.admitted, 2);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.cancelled, 1);
        closed(&mut live).await;
        let _released = TcpListener::bind(address).await.unwrap();
        echo.close().await;
    }
    #[tokio::test]
    async fn limits_and_upstream_failures_release_slots() {
        for mode in [1, 2, 3] {
            let echo = Echo::new(mode);
            let limits =
                TcpRelayLimits::new(1, Duration::from_millis(50), Duration::from_secs(1)).unwrap();
            let (address, stop, task) = start(echo.clone(), limits).await;
            let mut client = TcpStream::connect(address).await.unwrap();
            echo.wait(1).await;
            closed(&mut client).await;
            echo.mode.store(0, Ordering::SeqCst);
            let mut recovered = TcpStream::connect(address).await.unwrap();
            echo.wait(2).await;
            recovered.write_all(b"ok").await.unwrap();
            let mut bytes = [0; 2];
            recovered.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"ok");
            let mut refused = TcpStream::connect(address).await.unwrap();
            closed(&mut refused).await;
            assert_eq!(echo.opened.load(Ordering::SeqCst), 2);
            stop.send(()).unwrap();
            let stats = task.await.unwrap().unwrap();
            assert_eq!(stats.admitted, 2);
            assert_eq!(stats.failed, 1);
            assert_eq!(stats.refused, 1);
            assert_eq!(stats.cancelled, 1);
            closed(&mut recovered).await;
            echo.close().await;
        }
    }
    #[tokio::test]
    async fn absolute_age_closes_idle_connections() {
        let echo = Echo::new(0);
        let limits =
            TcpRelayLimits::new(1, Duration::from_secs(1), Duration::from_millis(50)).unwrap();
        let (address, stop, task) = start(echo.clone(), limits).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        echo.wait(1).await;
        closed(&mut client).await;
        stop.send(()).unwrap();
        let stats = task.await.unwrap().unwrap();
        assert_eq!(stats.failed, 1);
        echo.close().await;
        assert!(TcpRelayLimits::new(0, Duration::from_secs(1), Duration::from_secs(1)).is_err());
        assert!(TcpRelayLimits::new(1025, Duration::from_secs(1), Duration::from_secs(1)).is_err());
        assert!(TcpRelayLimits::new(1, Duration::ZERO, Duration::from_secs(1)).is_err());
    }
    #[tokio::test]
    async fn cancelling_owner_closes_live_stream_and_listener() {
        let echo = Echo::new(0);
        let limits =
            TcpRelayLimits::new(1, Duration::from_secs(1), Duration::from_secs(10)).unwrap();
        let (address, _stop, task) = start(echo.clone(), limits).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        echo.wait(1).await;
        client.write_all(b"ready").await.unwrap();
        let mut bytes = [0; 5];
        client.read_exact(&mut bytes).await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        closed(&mut client).await;
        let _released = TcpListener::bind(address).await.unwrap();
        echo.close().await;
    }
    #[tokio::test]
    async fn mismatched_and_udp_only_reservations_never_dispatch() {
        let echo = Echo::new(0);
        let limits =
            TcpRelayLimits::new(1, Duration::from_secs(1), Duration::from_secs(1)).unwrap();
        for udp in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let mut allocation = row(address.port());
            if udp {
                allocation.protocol = PortProtocol::Udp;
            } else {
                allocation.public_port = if address.port() == 65535 {
                    address.port() - 1
                } else {
                    address.port() + 1
                };
            }
            assert!(serve_tcp(
                listener,
                allocation,
                echo.clone(),
                limits,
                std::future::pending()
            )
            .await
            .is_err());
            let _released = TcpListener::bind(address).await.unwrap();
        }
        assert_eq!(echo.opened.load(Ordering::SeqCst), 0);
    }
}
