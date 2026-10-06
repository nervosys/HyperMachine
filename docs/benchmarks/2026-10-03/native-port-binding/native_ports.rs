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
            Some(UdpSocket::bind(address).await?)
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
            Some(UdpSocket::bind(self.address).await?)
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
        let client = UdpSocket::bind(SocketAddr::new(binding.address().ip(), 0))
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
        let occupied_udp = UdpSocket::bind(binding.address()).await.unwrap();
        assert!(binding.update(row(port, PortProtocol::Both)).await.is_err());
        assert_eq!(binding.allocation().protocol(), PortProtocol::Tcp);
        tcp_exchange(&binding).await;
        drop(occupied_udp);
        binding.update(row(port, PortProtocol::Both)).await.unwrap();
        tcp_exchange(&binding).await;
        udp_exchange(&binding).await;
        assert!(TcpListener::bind(binding.address()).await.is_err());
        assert!(UdpSocket::bind(binding.address()).await.is_err());
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
        let _udp = UdpSocket::bind(address).await.unwrap();
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
        let occupied = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap();
        assert!(
            NativePortBinding::bind(address.ip(), row(address.port(), PortProtocol::Both))
                .await
                .is_err()
        );
        let _tcp = TcpListener::bind(address).await.unwrap();
        assert!(
            NativePortBinding::bind(address.ip(), row(0, PortProtocol::Tcp))
                .await
                .is_err()
        );
    }
}
