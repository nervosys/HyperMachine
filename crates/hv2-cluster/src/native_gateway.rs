//! Owned TCP/UDP gateway supervision from authoritative cluster reservations.
use crate::{
    native_budget::NativeSessionBudget,
    native_ports::{validated_snapshot, NativePortBinding, PortBindFailure},
    native_tcp::{serve_tcp_with_budget, NativeTcpConnector, TcpRelayLimits},
    native_udp::{serve_udp_with_budget, NativeUdpConnector, UdpRelayLimits},
    ports::PortAllocation,
    store::ClusterStore,
};
use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    io,
    net::IpAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::watch,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
struct Entry {
    row: PortAllocation,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Entry {
    async fn stop(mut self) {
        self.stop.send_replace(true);
        if timeout(Duration::from_secs(5), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}
async fn stopped(mut state: watch::Receiver<bool>) {
    while !*state.borrow_and_update() {
        if state.changed().await.is_err() {
            break;
        }
    }
}
/// Each instance owns its listeners and relay tasks. Cloning the same admission
/// budget into every instance enforces one aggregate session cap across them.
/// This type starts no tasks until explicitly refreshed/reconciled or run.
pub struct NativeGateway {
    store: Arc<dyn ClusterStore>,
    ip: IpAddr,
    max_ports: usize,
    tcp: Arc<dyn NativeTcpConnector>,
    udp: Arc<dyn NativeUdpConnector>,
    budget: NativeSessionBudget,
    tcp_limits: TcpRelayLimits,
    udp_limits: UdpRelayLimits,
    entries: BTreeMap<u16, Entry>,
}
impl NativeGateway {
    /// # Errors
    /// Rejects gateway port limits outside 1–4096.
    pub fn new(
        store: Arc<dyn ClusterStore>,
        ip: IpAddr,
        max_ports: usize,
        tcp: Arc<dyn NativeTcpConnector>,
        udp: Arc<dyn NativeUdpConnector>,
        budget: NativeSessionBudget,
        tcp_limits: TcpRelayLimits,
        udp_limits: UdpRelayLimits,
    ) -> io::Result<Self> {
        if !(1..=4096).contains(&max_ports) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid gateway port limit",
            ));
        }
        Ok(Self {
            store,
            ip,
            max_ports,
            tcp,
            udp,
            budget,
            tcp_limits,
            udp_limits,
            entries: BTreeMap::new(),
        })
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Signal all listeners before awaiting any relay shutdown.
    pub async fn close(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        for entry in entries.values() {
            entry.stop.send_replace(true);
        }
        for (_, entry) in entries {
            entry.stop().await;
        }
    }
    async fn snapshot(&self) -> io::Result<Vec<PortAllocation>> {
        let rows = self
            .store
            .port_allocations(None)
            .await
            .map_err(|_| io::Error::other("port store unavailable"))?;
        // Validate all rows before lifecycle filtering; an invalid hidden row
        // must not turn a corrupted complete snapshot into a plausible subset.
        validated_snapshot(rows.clone(), self.max_ports)?;
        let records = self
            .store
            .sandboxes()
            .await
            .map_err(|_| io::Error::other("sandbox store unavailable"))?;
        let nodes = self
            .store
            .nodes()
            .await
            .map_err(|_| io::Error::other("node store unavailable"))?;
        let live: HashSet<_> = nodes.into_iter().map(|node| node.id).collect();
        let active: HashSet<_> = records
            .into_iter()
            .filter(|record| !record.paused && live.contains(&record.node_id))
            .map(|record| record.sandbox_id)
            .collect();
        Ok(rows
            .into_iter()
            .filter(|row| active.contains(row.sandbox_id()))
            .collect())
    }
    /// Refresh a complete snapshot, bounded to five seconds. Store errors,
    /// invalid snapshots and timeouts close all listeners. Paused/missing VMs
    /// and unavailable nodes lose listeners while their reservation may persist.
    /// # Errors
    /// Store/validation failure; occupied ports are returned for retry.
    pub async fn refresh(&mut self) -> io::Result<Vec<PortBindFailure>> {
        match timeout(Duration::from_secs(5), self.snapshot()).await {
            Ok(Ok(rows)) => self.reconcile(rows).await,
            result => {
                self.close().await;
                match result {
                    Ok(Err(error)) => Err(error),
                    _ => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "gateway snapshot timed out",
                    )),
                }
            }
        }
    }
    /// Apply a complete authoritative snapshot. Changed/removed reservations
    /// stop their active sessions before rebinding; identical healthy entries
    /// remain live. Invalid snapshots stop everything. Durable state is untouched.
    /// # Errors
    /// Invalid snapshot; per-port binding failures are returned for retry.
    pub async fn reconcile(
        &mut self,
        rows: Vec<PortAllocation>,
    ) -> io::Result<Vec<PortBindFailure>> {
        let desired = match validated_snapshot(rows, self.max_ports) {
            Ok(rows) => rows,
            Err(error) => {
                self.close().await;
                return Err(error);
            }
        };
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(port, entry)| {
                desired.get(port) != Some(&entry.row) || entry.task.is_finished()
            })
            .map(|(port, _)| *port)
            .collect();
        let old: Vec<_> = removed
            .into_iter()
            .filter_map(|port| self.entries.remove(&port))
            .collect();
        for entry in &old {
            entry.stop.send_replace(true);
        }
        for entry in old {
            entry.stop().await;
        }
        let mut failures = Vec::new();
        for (port, row) in desired {
            if self.entries.contains_key(&port) {
                continue;
            }
            let binding = match NativePortBinding::bind(self.ip, row.clone()).await {
                Ok(binding) => binding,
                Err(error) => {
                    failures.push(PortBindFailure {
                        public_port: port,
                        kind: error.kind(),
                    });
                    continue;
                }
            };
            let (allocation, tcp, udp) = binding.into_parts();
            let (stop, state) = watch::channel(false);
            let tcp_connector = self.tcp.clone();
            let udp_connector = self.udp.clone();
            let budget = self.budget.clone();
            let tcp_limits = self.tcp_limits;
            let udp_limits = self.udp_limits;
            let internal_stop = stop.clone();
            let task = tokio::spawn(async move {
                let mut children = JoinSet::new();
                if let Some(listener) = tcp {
                    let allocation = allocation.clone();
                    let budget = budget.clone();
                    let state = state.clone();
                    children.spawn(async move {
                        let _ = serve_tcp_with_budget(
                            listener,
                            allocation,
                            tcp_connector,
                            tcp_limits,
                            budget,
                            stopped(state),
                        )
                        .await;
                    });
                }
                if let Some(socket) = udp {
                    children.spawn(async move {
                        let _ = serve_udp_with_budget(
                            socket,
                            allocation,
                            udp_connector,
                            udp_limits,
                            budget,
                            stopped(state),
                        )
                        .await;
                    });
                }
                // Any listener exit stops its sibling; both perform explicit
                // peer cleanup. Dropping this supervisor aborts owned children.
                if children.join_next().await.is_some() {
                    internal_stop.send_replace(true);
                }
                while children.join_next().await.is_some() {}
            });
            self.entries.insert(port, Entry { row, stop, task });
        }
        Ok(failures)
    }
    /// Poll authoritative state and retry unavailable ports until shutdown.
    /// Errors close listeners and are retried; no sensitive details are logged.
    /// # Errors
    /// Invalid interval (20 ms–60 seconds).
    pub async fn run(
        &mut self,
        interval: Duration,
        shutdown: impl Future<Output = ()>,
    ) -> io::Result<()> {
        if interval < Duration::from_millis(20) || interval > Duration::from_secs(60) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid gateway polling interval",
            ));
        }
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tokio::pin!(shutdown);
        loop {
            tokio::select! { biased; _ = &mut shutdown => { self.close().await; return Ok(()); }, _ = ticks.tick() => {} }
            let result = tokio::select! { biased; _ = &mut shutdown => None, result = self.refresh() => Some(result) };
            match result {
                None => {
                    self.close().await;
                    return Ok(());
                }
                Some(Err(_)) => {
                    tracing::warn!("native gateway snapshot unavailable; listeners closed");
                }
                Some(Ok(failed)) if !failed.is_empty() => {
                    tracing::debug!(count = failed.len(), "native gateway ports unavailable");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        native_tcp::NativeTunnel,
        ports::{PortClaim, PortProtocol, PublicPortRange},
        store::MemoryStore,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream, UdpSocket},
        sync::{oneshot, Mutex},
    };
    struct Echo {
        tasks: Mutex<JoinSet<()>>,
    }
    impl Echo {
        async fn open(&self) -> io::Result<Box<dyn NativeTunnel>> {
            let (client, mut server) = tokio::io::duplex(131072);
            self.tasks.lock().await.spawn(async move {
                let mut bytes = [0; 8192];
                while let Ok(n) = server.read(&mut bytes).await {
                    if n == 0 || server.write_all(&bytes[..n]).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Box::new(client))
        }
        async fn close(&self) {
            let mut tasks = self.tasks.lock().await;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
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
    struct OwnedTask<T>(JoinHandle<T>);
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
    fn gateway(
        store: Arc<MemoryStore>,
        echo: Arc<Echo>,
        budget: NativeSessionBudget,
    ) -> NativeGateway {
        NativeGateway::new(
            store,
            "127.0.0.1".parse().unwrap(),
            2,
            echo.clone(),
            echo,
            budget,
            TcpRelayLimits::new(2, Duration::from_secs(1), Duration::from_secs(10)).unwrap(),
            UdpRelayLimits::new(
                2,
                Duration::from_secs(1),
                Duration::from_secs(10),
                Duration::from_secs(1),
            )
            .unwrap(),
        )
        .unwrap()
    }
    async fn setup() -> (Arc<MemoryStore>, std::net::SocketAddr, PortAllocation) {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = probe.local_addr().unwrap();
        drop(probe);
        let store = Arc::new(MemoryStore::new());
        store
            .put_node(
                &crate::store::tests::node("node", 0, 1),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        store
            .put_sandbox(&crate::store::tests::sandbox("owned-vm", "node"))
            .await
            .unwrap();
        let PortClaim::Allocated(row) = store
            .claim_port(
                "owned-vm",
                8080,
                "owner-a",
                PortProtocol::Both,
                PublicPortRange::new(address.port(), address.port()).unwrap(),
            )
            .await
            .unwrap()
        else {
            panic!("allocation failed");
        };
        (store, address, row)
    }
    async fn tcp_echo(client: &mut TcpStream) {
        client.write_all(b"tcp\0").await.unwrap();
        let mut bytes = [0; 4];
        timeout(Duration::from_secs(3), client.read_exact(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes, b"tcp\0");
    }
    async fn udp_echo(client: &UdpSocket, address: std::net::SocketAddr) {
        client.send_to(b"udp\0", address).await.unwrap();
        let mut bytes = [0; 16];
        let n = timeout(Duration::from_secs(3), client.recv(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..n], b"udp\0");
    }
    async fn closed(client: &mut TcpStream) {
        let result = timeout(Duration::from_secs(3), client.read(&mut [0; 1]))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0)) || result.is_err());
    }
    #[tokio::test]
    async fn gateway_owns_dual_relays_and_reconciles_live_lifecycle() {
        let (store, address, _) = setup().await;
        let echo = Arc::new(Echo {
            tasks: Mutex::new(JoinSet::new()),
        });
        let budget = NativeSessionBudget::new(2).unwrap();
        let mut worker = gateway(store.clone(), echo.clone(), budget.clone());
        assert!(worker.refresh().await.unwrap().is_empty());
        let mut client = TcpStream::connect(address).await.unwrap();
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        tcp_echo(&mut client).await;
        udp_echo(&peer, address).await;
        assert_eq!(budget.available(), 0);
        worker.refresh().await.unwrap();
        tcp_echo(&mut client).await;
        assert_eq!(budget.available(), 0);
        store
            .claim_port(
                "owned-vm",
                8080,
                "owner-a",
                PortProtocol::Udp,
                PublicPortRange::new(address.port(), address.port()).unwrap(),
            )
            .await
            .unwrap();
        assert!(worker.refresh().await.unwrap().is_empty());
        closed(&mut client).await;
        udp_echo(&peer, address).await;
        let occupied = TcpListener::bind(address).await.unwrap();
        store
            .claim_port(
                "owned-vm",
                8080,
                "owner-a",
                PortProtocol::Both,
                PublicPortRange::new(address.port(), address.port()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(worker.refresh().await.unwrap().len(), 1);
        assert!(worker.is_empty());
        assert_eq!(budget.available(), 2);
        let released_udp = UdpSocket::bind(address).await.unwrap();
        drop(released_udp);
        drop(occupied);
        assert!(worker.refresh().await.unwrap().is_empty());
        let mut client = TcpStream::connect(address).await.unwrap();
        tcp_echo(&mut client).await;
        udp_echo(&peer, address).await;
        let mut record = store.sandbox("owned-vm").await.unwrap().unwrap();
        record.paused = true;
        store.put_sandbox(&record).await.unwrap();
        assert!(worker.refresh().await.unwrap().is_empty());
        assert!(worker.is_empty());
        closed(&mut client).await;
        assert_eq!(budget.available(), 2);
        record.paused = false;
        store.put_sandbox(&record).await.unwrap();
        worker.refresh().await.unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        tcp_echo(&mut client).await;
        store.remove_node("node").await.unwrap();
        worker.refresh().await.unwrap();
        closed(&mut client).await;
        assert!(worker.is_empty());
        store
            .put_node(
                &crate::store::tests::node("node", 0, 1),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        worker.refresh().await.unwrap();
        let rows = store.port_allocations(None).await.unwrap();
        assert!(worker
            .reconcile(vec![rows[0].clone(), rows[0].clone()])
            .await
            .is_err());
        assert!(worker.is_empty());
        worker.refresh().await.unwrap();
        worker.close().await;
        assert_eq!(budget.available(), 2);
        let mut restarted = gateway(store.clone(), echo.clone(), budget.clone());
        restarted.refresh().await.unwrap();
        udp_echo(&peer, address).await;
        store.delete_sandbox("owned-vm").await.unwrap();
        restarted.refresh().await.unwrap();
        assert!(restarted.is_empty());
        assert_eq!(budget.available(), 2);
        let _tcp = TcpListener::bind(address).await.unwrap();
        let _udp = UdpSocket::bind(address).await.unwrap();
        echo.close().await;
    }
    #[tokio::test]
    async fn polling_worker_observes_deletion_and_stops_owned_tasks() {
        let (store, address, _) = setup().await;
        let echo = Arc::new(Echo {
            tasks: Mutex::new(JoinSet::new()),
        });
        let budget = NativeSessionBudget::new(2).unwrap();
        let mut worker = gateway(store.clone(), echo.clone(), budget.clone());
        let (stop, stopped) = oneshot::channel();
        let task = OwnedTask(tokio::spawn(async move {
            worker
                .run(Duration::from_millis(20), async {
                    let _ = stopped.await;
                })
                .await?;
            Ok::<_, io::Error>(worker)
        }));
        let mut client = timeout(Duration::from_secs(3), async {
            loop {
                match TcpStream::connect(address).await {
                    Ok(client) => break client,
                    Err(_) => tokio::task::yield_now().await,
                }
            }
        })
        .await
        .unwrap();
        tcp_echo(&mut client).await;
        store.delete_sandbox("owned-vm").await.unwrap();
        closed(&mut client).await;
        stop.send(()).unwrap();
        let worker = task.await.unwrap().unwrap();
        assert!(worker.is_empty());
        assert_eq!(budget.available(), 2);
        let _tcp = TcpListener::bind(address).await.unwrap();
        let _udp = UdpSocket::bind(address).await.unwrap();
        echo.close().await;
    }
}
