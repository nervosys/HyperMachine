//! Exclusive native socket bindings for durable port reservations.
//!
//! This primitive does not authorize requests or forward traffic. A gateway must
//! reconcile it with the store and own connection/peer tasks separately.
use crate::ports::{PortAllocation, PortProtocol};
use std::{
    io,
    net::{IpAddr, SocketAddr},
};
use tokio::net::{TcpListener, UdpSocket};

/// Owns the listeners for one exact reservation. Dropping releases both sockets.
/// No address/port reuse option is enabled; competing gateways fail to bind.
pub struct NativePortBinding {
    allocation: PortAllocation,
    address: SocketAddr,
    tcp: Option<TcpListener>,
    udp: Option<UdpSocket>,
}
impl NativePortBinding {
    /// Bind the allocated port on the operator-selected interface.
    /// # Errors
    /// Invalid records or occupied/unavailable addresses fail without retaining
    /// a partially bound socket. This does not remove the durable reservation.
    pub async fn bind(ip: IpAddr, allocation: PortAllocation) -> io::Result<Self> {
        allocation
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port allocation"))?;
        let address = SocketAddr::new(ip, allocation.public_port());
        let tcp = if wants_tcp(allocation.protocol()) {
            Some(TcpListener::bind(address).await?)
        } else {
            None
        };
        let udp = if wants_udp(allocation.protocol()) {
            Some(crate::udp_socket::bind(address).await?)
        } else {
            None
        };
        Ok(Self {
            allocation,
            address,
            tcp,
            udp,
        })
    }
    pub(crate) fn into_parts(self) -> (PortAllocation, Option<TcpListener>, Option<UdpSocket>) {
        (self.allocation, self.tcp, self.udp)
    }
    pub fn allocation(&self) -> &PortAllocation {
        &self.allocation
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn tcp(&self) -> Option<&TcpListener> {
        self.tcp.as_ref()
    }
    pub fn udp(&self) -> Option<&UdpSocket> {
        self.udp.as_ref()
    }

    /// Reconcile a protocol update without changing reservation identity.
    /// New sockets bind before old sockets are released. A failed bind leaves
    /// the old protocol and sockets intact, so the gateway can report failure
    /// and retry. The caller must reconcile any already-committed store update.
    /// # Errors
    /// Identity/owner/address changes or an unavailable new protocol socket.
    pub async fn update(&mut self, allocation: PortAllocation) -> io::Result<()> {
        allocation
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port allocation"))?;
        if allocation.sandbox_id() != self.allocation.sandbox_id()
            || allocation.machine_port() != self.allocation.machine_port()
            || allocation.public_port() != self.allocation.public_port()
            || allocation.owner_id() != self.allocation.owner_id()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "port reservation identity changed",
            ));
        }
        let protocol = allocation.protocol();
        let new_tcp = if wants_tcp(protocol) && self.tcp.is_none() {
            Some(TcpListener::bind(self.address).await?)
        } else {
            None
        };
        let new_udp = if wants_udp(protocol) && self.udp.is_none() {
            Some(crate::udp_socket::bind(self.address).await?)
        } else {
            None
        };
        if let Some(tcp) = new_tcp {
            self.tcp = Some(tcp);
        }
        if let Some(udp) = new_udp {
            self.udp = Some(udp);
        }
        if !wants_tcp(protocol) {
            self.tcp = None;
        }
        if !wants_udp(protocol) {
            self.udp = None;
        }
        self.allocation = allocation;
        Ok(())
    }
}
fn wants_tcp(protocol: PortProtocol) -> bool {
    matches!(protocol, PortProtocol::Tcp | PortProtocol::Both)
}
fn wants_udp(protocol: PortProtocol) -> bool {
    matches!(protocol, PortProtocol::Udp | PortProtocol::Both)
}

/// One gateway's bounded desired-state listener set. A service must separately
/// own relay tasks and cancel them when their reservation changes or disappears.
/// This type deliberately cannot authorize a caller or publish management APIs.
pub struct NativePortRegistry {
    ip: IpAddr,
    limit: usize,
    bindings: std::collections::BTreeMap<u16, NativePortBinding>,
}
/// A valid snapshot can have individual unavailable ports. Those ports remain
/// unbound and will be retried on the next reconciliation.
#[derive(Debug)]
pub struct PortBindFailure {
    pub public_port: u16,
    pub kind: io::ErrorKind,
}
impl NativePortRegistry {
    /// # Errors
    /// Rejects zero or more than 4096 listeners per gateway.
    pub fn new(ip: IpAddr, limit: usize) -> io::Result<Self> {
        if !(1..=4096).contains(&limit) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "gateway port limit must be 1–4096",
            ));
        }
        Ok(Self {
            ip,
            limit,
            bindings: Default::default(),
        })
    }
    pub fn binding(&self, port: u16) -> Option<&NativePortBinding> {
        self.bindings.get(&port)
    }
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
    /// Close all owned listeners. Established relay tasks need separate cleanup.
    pub fn close(&mut self) {
        self.bindings.clear();
    }
    /// Read authoritative reservations. Store failure closes all listeners;
    /// callers may retry after recovery. No timers or background work are started.
    /// # Errors
    /// Snapshot read/validation failed; individual bind failures are returned.
    pub async fn refresh(
        &mut self,
        store: &dyn crate::store::ClusterStore,
    ) -> io::Result<Vec<PortBindFailure>> {
        let desired = match store.port_allocations(None).await {
            Ok(rows) => rows,
            Err(_) => {
                self.close();
                return Err(io::Error::other("port reservation store unavailable"));
            }
        };
        self.reconcile(desired).await
    }
    /// Apply a complete authoritative snapshot. Unchanged bindings survive;
    /// removed, replaced and changing bindings leave the active registry before
    /// any await. A failed/cancelled update closes its old listeners rather than
    /// exposing a stale protocol. Corrupt, duplicate or oversized snapshots close
    /// all bindings. The durable allocation is never modified by this operation.
    /// # Errors
    /// Invalid complete snapshot; callers must not pass partial/incremental lists.
    pub async fn reconcile(
        &mut self,
        desired: Vec<PortAllocation>,
    ) -> io::Result<Vec<PortBindFailure>> {
        let by_port = match validated_snapshot(desired, self.limit) {
            Ok(rows) => rows,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        let mut changed = std::collections::BTreeMap::new();
        self.bindings
            .retain(|port, binding| by_port.get(port) == Some(binding.allocation()));
        // Keeping a socket through a protocol update would preserve a stale
        // protocol on cancellation. Rebind changed reservations after dropping
        // their old listeners; stable allocated port numbers are unaffected.
        for (port, row) in by_port {
            if !self.bindings.contains_key(&port) {
                changed.insert(port, row);
            }
        }
        let mut failures = Vec::new();
        for (port, row) in changed {
            match NativePortBinding::bind(self.ip, row).await {
                Ok(binding) => {
                    self.bindings.insert(port, binding);
                }
                Err(error) => failures.push(PortBindFailure {
                    public_port: port,
                    kind: error.kind(),
                }),
            }
        }
        Ok(failures)
    }
}

pub(crate) fn validated_snapshot(
    desired: Vec<PortAllocation>,
    limit: usize,
) -> io::Result<std::collections::BTreeMap<u16, PortAllocation>> {
    let mut by_port = std::collections::BTreeMap::new();
    let mut destinations = std::collections::HashSet::new();
    let invalid = desired.len() > limit || desired.iter().any(|row| row.validate().is_err());
    if invalid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid port reservation snapshot",
        ));
    }
    for row in desired {
        let destination = (row.sandbox_id().to_owned(), row.machine_port());
        if !destinations.insert(destination) || by_port.insert(row.public_port(), row).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate port reservation snapshot",
            ));
        }
    }
    Ok(by_port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::{timeout, Duration},
    };
    fn row(port: u16, protocol: PortProtocol) -> PortAllocation {
        PortAllocation {
            sandbox_id: "owned-vm".into(),
            machine_port: 5353,
            public_port: port,
            owner_id: "owner-a".into(),
            protocol,
        }
    }
    async fn tcp_exchange(binding: &NativePortBinding) {
        let mut client = TcpStream::connect(binding.address()).await.unwrap();
        let (mut server, _) = binding.tcp().unwrap().accept().await.unwrap();
        client.write_all(b"tcp\0binary").await.unwrap();
        let mut bytes = [0; 10];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"tcp\0binary");
    }
    async fn udp_exchange(binding: &NativePortBinding) {
        let client = crate::udp_socket::bind(SocketAddr::new(binding.address().ip(), 0))
            .await
            .unwrap();
        for payload in [b"".as_slice(), b"udp\0binary", &[7; 65507]] {
            client.send_to(payload, binding.address()).await.unwrap();
            let mut bytes = vec![0; 65535];
            let (n, peer) = timeout(
                Duration::from_secs(2),
                binding.udp().unwrap().recv_from(&mut bytes),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(&bytes[..n], payload);
            binding
                .udp()
                .unwrap()
                .send_to(&bytes[..n], peer)
                .await
                .unwrap();
            let n = timeout(Duration::from_secs(2), client.recv(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&bytes[..n], payload);
        }
    }
    #[tokio::test]
    async fn same_port_protocol_changes_preserve_existing_listener() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut binding =
            NativePortBinding::bind("127.0.0.1".parse().unwrap(), row(port, PortProtocol::Tcp))
                .await
                .unwrap();
        let occupied_udp = crate::udp_socket::bind(binding.address()).await.unwrap();
        assert!(binding.update(row(port, PortProtocol::Both)).await.is_err());
        assert_eq!(binding.allocation().protocol(), PortProtocol::Tcp);
        tcp_exchange(&binding).await;
        drop(occupied_udp);
        binding.update(row(port, PortProtocol::Both)).await.unwrap();
        tcp_exchange(&binding).await;
        udp_exchange(&binding).await;
        assert!(TcpListener::bind(binding.address()).await.is_err());
        assert!(crate::udp_socket::bind(binding.address()).await.is_err());
        let mut foreign = row(port, PortProtocol::Udp);
        foreign.owner_id = "owner-b".into();
        assert!(binding.update(foreign).await.is_err());
        assert_eq!(binding.allocation().protocol(), PortProtocol::Both);
        binding.update(row(port, PortProtocol::Udp)).await.unwrap();
        assert!(binding.tcp().is_none());
        udp_exchange(&binding).await;
        let released_tcp = TcpListener::bind(binding.address()).await.unwrap();
        assert!(binding.update(row(port, PortProtocol::Tcp)).await.is_err());
        assert_eq!(binding.allocation().protocol(), PortProtocol::Udp);
        udp_exchange(&binding).await;
        drop(released_tcp);
        binding.update(row(port, PortProtocol::Tcp)).await.unwrap();
        assert!(binding.udp().is_none());
        tcp_exchange(&binding).await;
        let address = binding.address();
        drop(binding);
        let _tcp = TcpListener::bind(address).await.unwrap();
        let _udp = crate::udp_socket::bind(address).await.unwrap();
    }
    #[tokio::test]
    async fn ipv6_dual_protocol_datagrams_and_exclusive_bind() {
        let probe = TcpListener::bind("[::1]:0").await.unwrap();
        let address = probe.local_addr().unwrap();
        drop(probe);
        let binding =
            NativePortBinding::bind(address.ip(), row(address.port(), PortProtocol::Both))
                .await
                .unwrap();
        assert_eq!(binding.address(), address);
        tcp_exchange(&binding).await;
        udp_exchange(&binding).await;
        assert!(
            NativePortBinding::bind(address.ip(), row(address.port(), PortProtocol::Both))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn failed_dual_bind_releases_partial_tcp_socket() {
        // The UDP port is ephemeral but its TCP twin is not reserved, so a
        // concurrent test binding TCP port 0 can be handed it. A leaked
        // partial socket fails every attempt; that race fails one at random.
        let mut attempts = 0;
        let (_occupied, address, _tcp) = loop {
            let occupied = crate::udp_socket::bind("127.0.0.1:0").await.unwrap();
            let address = occupied.local_addr().unwrap();
            assert!(
                NativePortBinding::bind(address.ip(), row(address.port(), PortProtocol::Both))
                    .await
                    .is_err()
            );
            match TcpListener::bind(address).await {
                Ok(tcp) => break (occupied, address, tcp),
                Err(error) if attempts < 4 => {
                    attempts += 1;
                    eprintln!("TCP {address} unavailable ({error}); retrying");
                }
                Err(error) => panic!("TCP {address} still held after a failed dual bind: {error}"),
            }
        };
        assert!(
            NativePortBinding::bind(address.ip(), row(0, PortProtocol::Tcp))
                .await
                .is_err()
        );
    }
    async fn free_port() -> u16 {
        let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        socket.local_addr().unwrap().port()
    }
    #[tokio::test]
    async fn registry_recovers_same_ports_and_fails_closed_on_update() {
        let port = free_port().await;
        let ip = "127.0.0.1".parse().unwrap();
        let mut registry = NativePortRegistry::new(ip, 2).unwrap();
        assert!(registry
            .reconcile(vec![row(port, PortProtocol::Tcp)])
            .await
            .unwrap()
            .is_empty());
        tcp_exchange(registry.binding(port).unwrap()).await;
        assert!(registry
            .reconcile(vec![row(port, PortProtocol::Tcp)])
            .await
            .unwrap()
            .is_empty());
        let occupied = crate::udp_socket::bind(SocketAddr::new(ip, port))
            .await
            .unwrap();
        let errors = registry
            .reconcile(vec![row(port, PortProtocol::Both)])
            .await
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].public_port, port);
        assert!(registry.is_empty());
        let released = TcpListener::bind(SocketAddr::new(ip, port)).await.unwrap();
        drop(released);
        drop(occupied);
        assert!(registry
            .reconcile(vec![row(port, PortProtocol::Both)])
            .await
            .unwrap()
            .is_empty());
        udp_exchange(registry.binding(port).unwrap()).await;
        drop(registry);
        let mut restarted = NativePortRegistry::new(ip, 2).unwrap();
        assert!(restarted
            .reconcile(vec![row(port, PortProtocol::Both)])
            .await
            .unwrap()
            .is_empty());
        tcp_exchange(restarted.binding(port).unwrap()).await;
        udp_exchange(restarted.binding(port).unwrap()).await;
        restarted.reconcile(vec![]).await.unwrap();
        assert!(restarted.is_empty());
        let _tcp = TcpListener::bind(SocketAddr::new(ip, port)).await.unwrap();
        let _udp = crate::udp_socket::bind(SocketAddr::new(ip, port))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn registry_rejects_ambiguous_and_oversized_snapshots() {
        let port = free_port().await;
        let ip = "127.0.0.1".parse().unwrap();
        assert!(NativePortRegistry::new(ip, 0).is_err());
        assert!(NativePortRegistry::new(ip, 4097).is_err());
        let mut registry = NativePortRegistry::new(ip, 1).unwrap();
        let original = row(port, PortProtocol::Both);
        registry.reconcile(vec![original.clone()]).await.unwrap();
        assert!(registry
            .reconcile(vec![original.clone(), original.clone()])
            .await
            .is_err());
        assert!(registry.is_empty());
        let mut registry = NativePortRegistry::new(ip, 2).unwrap();
        registry.reconcile(vec![original.clone()]).await.unwrap();
        let mut duplicate = original.clone();
        duplicate.owner_id = "other-owner".into();
        assert!(registry
            .reconcile(vec![original.clone(), duplicate])
            .await
            .is_err());
        assert!(registry.is_empty());
        let mut duplicate = original.clone();
        duplicate.public_port = if port == 65535 { port - 1 } else { port + 1 };
        assert!(registry
            .reconcile(vec![original.clone(), duplicate])
            .await
            .is_err());
        registry.reconcile(vec![original]).await.unwrap();
        assert!(registry
            .reconcile(vec![row(0, PortProtocol::Tcp)])
            .await
            .is_err());
        assert!(registry.is_empty());
        let _tcp = TcpListener::bind(SocketAddr::new(ip, port)).await.unwrap();
        let _udp = crate::udp_socket::bind(SocketAddr::new(ip, port))
            .await
            .unwrap();
    }
}
