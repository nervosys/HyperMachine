//! Shared admission budget for native TCP connections and UDP peer sessions.
use std::{io, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
/// Clone one instance into all listeners owned by a gateway. Each admitted TCP
/// connection or UDP peer retains one permit through setup and relay teardown.
#[derive(Clone)]
pub struct NativeSessionBudget {
    capacity: usize,
    permits: Arc<Semaphore>,
}
impl NativeSessionBudget {
    /// # Errors
    /// Rejects zero or more than 65,536 aggregate sessions.
    pub fn new(capacity: usize) -> io::Result<Self> {
        if !(1..=65536).contains(&capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native session budget must be 1–65536",
            ));
        }
        Ok(Self {
            capacity,
            permits: Arc::new(Semaphore::new(capacity)),
        })
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }
    pub(crate) fn try_acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.permits.clone().try_acquire_owned().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        native_tcp::{serve_tcp_with_budget, NativeTcpConnector, NativeTunnel, TcpRelayLimits},
        native_udp::{serve_udp_with_budget, NativeUdpConnector, UdpRelayLimits},
        ports::{PortAllocation, PortProtocol},
    };
    use std::{
        future::Future,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream, UdpSocket},
        sync::{oneshot, Mutex},
        task::JoinSet,
        time::timeout,
    };
    struct Echo {
        tasks: Mutex<JoinSet<()>>,
        opened: AtomicUsize,
    }
    impl Echo {
        async fn open(&self) -> io::Result<Box<dyn NativeTunnel>> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            let (client, mut server) = tokio::io::duplex(8192);
            self.tasks.lock().await.spawn(async move {
                let mut bytes = [0; 8192];
                loop {
                    let Ok(n) = server.read(&mut bytes).await else {
                        break;
                    };
                    if n == 0 || server.write_all(&bytes[..n]).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Box::new(client))
        }
    }
    #[async_trait::async_trait]
    impl NativeTcpConnector for Echo {
        async fn open_tcp(&self, _: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
            self.open().await
        }
    }
    #[async_trait::async_trait]
    impl NativeUdpConnector for Echo {
        async fn open_udp(&self, _: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
            self.open().await
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
    fn row(port: u16) -> PortAllocation {
        PortAllocation {
            sandbox_id: "owned-vm".into(),
            machine_port: 8080,
            public_port: port,
            owner_id: "owner-a".into(),
            protocol: PortProtocol::Both,
        }
    }
    #[tokio::test]
    async fn tcp_and_udp_on_one_port_share_admission_and_return_slots() {
        let budget = NativeSessionBudget::new(1).unwrap();
        let echo = Arc::new(Echo {
            tasks: Mutex::new(JoinSet::new()),
            opened: AtomicUsize::new(0),
        });
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = tcp.local_addr().unwrap();
        let udp = UdpSocket::bind(address).await.unwrap();
        let tcp_limits =
            TcpRelayLimits::new(2, Duration::from_secs(1), Duration::from_secs(10)).unwrap();
        let udp_limits = UdpRelayLimits::new(
            2,
            Duration::from_secs(1),
            Duration::from_secs(10),
            Duration::from_secs(1),
        )
        .unwrap();
        let (stop_tcp, stopped_tcp) = oneshot::channel();
        let (stop_udp, stopped_udp) = oneshot::channel();
        let tcp_task = OwnedTask(tokio::spawn(serve_tcp_with_budget(
            tcp,
            row(address.port()),
            echo.clone(),
            tcp_limits,
            budget.clone(),
            async {
                let _ = stopped_tcp.await;
            },
        )));
        let udp_task = OwnedTask(tokio::spawn(serve_udp_with_budget(
            udp,
            row(address.port()),
            echo.clone(),
            udp_limits,
            budget.clone(),
            async {
                let _ = stopped_udp.await;
            },
        )));
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(b"tcp").await.unwrap();
        let mut bytes = [0; 3];
        timeout(Duration::from_secs(3), client.read_exact(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes, b"tcp");
        assert_eq!(budget.available(), 0);
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        peer.send_to(b"capped", address).await.unwrap();
        assert!(timeout(Duration::from_millis(30), peer.recv(&mut [0; 16]))
            .await
            .is_err());
        assert_eq!(echo.opened.load(Ordering::SeqCst), 1);
        stop_tcp.send(()).unwrap();
        let tcp_stats = tcp_task.await.unwrap().unwrap();
        assert_eq!(tcp_stats.cancelled, 1);
        assert_eq!(budget.available(), 1);
        peer.send_to(b"udp\0", address).await.unwrap();
        let mut bytes = [0; 16];
        let n = timeout(Duration::from_secs(3), peer.recv(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..n], b"udp\0");
        assert_eq!(budget.available(), 0);
        assert_eq!(echo.opened.load(Ordering::SeqCst), 2);
        let other_tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let other_address = other_tcp.local_addr().unwrap();
        let (stop_other, stopped_other) = oneshot::channel();
        let other = OwnedTask(tokio::spawn(serve_tcp_with_budget(
            other_tcp,
            row(other_address.port()),
            echo.clone(),
            tcp_limits,
            budget.clone(),
            async {
                let _ = stopped_other.await;
            },
        )));
        let mut refused = TcpStream::connect(other_address).await.unwrap();
        let result = timeout(Duration::from_secs(3), refused.read(&mut [0; 1]))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0)) || result.is_err());
        assert_eq!(echo.opened.load(Ordering::SeqCst), 2);
        stop_other.send(()).unwrap();
        let stats = other.await.unwrap().unwrap();
        assert_eq!(stats.refused, 1);
        assert_eq!(stats.admitted, 0);
        stop_udp.send(()).unwrap();
        let stats = udp_task.await.unwrap().unwrap();
        assert_eq!(stats.refused, 1);
        assert_eq!(stats.cancelled, 1);
        assert_eq!(budget.available(), budget.capacity());
        let mut tasks = echo.tasks.lock().await;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        let _tcp = TcpListener::bind(address).await.unwrap();
        let _udp = UdpSocket::bind(address).await.unwrap();
        assert!(NativeSessionBudget::new(0).is_err());
        assert!(NativeSessionBudget::new(65537).is_err());
    }
}
