//! A sandbox's router: the far end of its network link, in userspace.
//!
//! [`Bridge`](crate::bridge::Bridge) carries a guest's frames to a
//! [`HostLink`]. A TAP device is one such link, and it needs `CAP_NET_ADMIN`
//! and a host configured to route and filter for it. This is the other: a TCP/IP
//! stack ([`smoltcp`]) that answers as the guest's gateway and DNS server, ends
//! every TCP connection the guest opens, and opens the corresponding one from
//! the host -- if, and only if, the sandbox's [`NetworkPolicy`] allows it.
//!
//! ```text
//!   guest 10.0.2.15 ── virtio-net ── Bridge ── Gateway (10.0.2.2, DNS 10.0.2.3)
//!                                                 │ per connection: decide, then
//!                                                 └─ TcpStream from the host
//! ```
//!
//! Nothing here needs privilege, and nothing leaves the host except through a
//! socket this code opened after asking the policy.
//!
//! # How a connection is decided
//!
//! At the guest's SYN, from the address alone where that is enough:
//!
//! - **Refused** (a reserved address, a `denyOut` address with no name rules
//!   that could override it, or the default): no socket is created, and the
//!   stack answers the SYN with a RST. The guest sees "connection refused" at
//!   once, which is the honest answer and costs a hostile guest nothing to
//!   learn -- it could learn the same by trying.
//! - **Allowed** by address, or because this gateway's own DNS answered an
//!   allowed name with that address: the host dials first, with the guest's
//!   SYN held unanswered (`pause_synack`). A refused dial becomes a RST, so the
//!   guest sees the upstream's own answer rather than a connection that opens
//!   and immediately dies.
//! - **Needs a name** (the address is refused but name rules exist): the
//!   handshake completes, and the first bytes the guest sends are read for a
//!   TLS SNI or HTTP `Host`. The name must be allowed *and* must resolve, from
//!   the host, to the address the guest connected to -- otherwise an allowed
//!   name aimed at any address would be an allowed connection to anywhere.
//!
//! When a TLS ClientHello does arrive on a connection allowed by a DNS answer,
//! its SNI must also be allowed; a CDN address shared by an allowed and a
//! refused name does not let the refused one through.
//!
//! # Resolving is a policy decision
//!
//! A DNS query is itself egress: `secret-data.attacker.example` carries its
//! payload in the question, to whoever runs that zone, whether or not any
//! connection follows. So a sandbox whose policy would refuse what a name
//! resolves to is not given the resolution either -- the query is answered
//! `REFUSED` unless the name is allowed, or the policy lets unnamed traffic
//! out anyway.
//!
//! # What it does not do
//!
//! - **Ordinary UDP other than DNS**, and **ICMP**: dropped. Private IPv4
//!   UDP uses the operator hook and bounded framed sessions. A guest `ping` gets no
//!   answer, rather than a fabricated one from the gateway.
//! - **IPv6**: the guest is configured with IPv4 only.
//! - **Inbound**: nothing from outside can open a connection to a guest
//!   through this. (`hv2-api`'s sandbox proxy is the inbound path, over vsock.)

pub mod dns;
pub mod mitm;
pub mod private_udp;
pub mod sniff;
pub mod socks;

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use parking_lot::{Mutex, RwLock};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{
    EthernetAddress, EthernetFrame, EthernetProtocol, HardwareAddress, IpAddress, IpCidr,
    IpProtocol, Ipv4Packet, TcpPacket, UdpPacket,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::mpsc;

use crate::bridge::HostLink;
use crate::network_policy::{AddressVerdict, NetworkPolicy, Verdict};
use sniff::Sniffed;

/// Addresses and limits.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// The MAC the gateway answers ARP with.
    pub mac: [u8; 6],
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
    pub guest: Ipv4Addr,
    pub prefix: u8,
    /// How long a name-first connection may take to name itself.
    pub name_timeout: Duration,
    /// How long to wait for a guest to speak first, when the answer matters,
    /// before treating the protocol as one where the server speaks first.
    pub sniff_wait: Duration,
    pub connect_timeout: Duration,
    /// Open TCP connections, at most. A SYN beyond this is refused, because
    /// every connection holds two socket buffers of host memory.
    pub max_connections: usize,
    pub dns_ttl: u32,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            mac: [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02],
            gateway: Ipv4Addr::new(10, 0, 2, 2),
            dns: Ipv4Addr::new(10, 0, 2, 3),
            guest: Ipv4Addr::new(10, 0, 2, 15),
            prefix: 24,
            name_timeout: Duration::from_secs(5),
            sniff_wait: Duration::from_millis(300),
            connect_timeout: Duration::from_secs(10),
            max_connections: 256,
            dns_ttl: 30,
        }
    }
}

impl GatewayConfig {
    /// The kernel command-line argument that configures a Linux guest for
    /// this gateway (`CONFIG_IP_PNP`), with no userspace involved. The
    /// nameserver lands in `/proc/net/pnp`, which is in `resolv.conf` format.
    #[must_use]
    pub fn kernel_ip_arg(&self) -> String {
        let mask = u32::MAX
            .checked_shl(32 - u32::from(self.prefix))
            .unwrap_or(0);
        format!(
            "ip={}::{}:{}::eth0:off:{}",
            self.guest,
            self.gateway,
            Ipv4Addr::from(mask),
            self.dns
        )
    }
}

/// Resolves names from the host.
#[async_trait::async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>>;
}

/// The host's own resolver.
pub struct SystemResolver;

#[async_trait::async_trait]
impl Resolver for SystemResolver {
    async fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        Ok(tokio::net::lookup_host((name, 0))
            .await?
            .map(|a| a.ip())
            .collect())
    }
}

/// A byte stream to somewhere outside.
pub trait Upstream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Upstream for T {}

/// Opens connections from the host.
#[async_trait::async_trait]
pub trait Dialer: Send + Sync {
    async fn dial(&self, destination: SocketAddr) -> io::Result<Box<dyn Upstream>>;
}

/// Operator-provided private routing for one fixed guest source. Classifiers
/// must claim the entire reserved namespace/address pool, including unknown
/// names and unallocated addresses, and stay stable for the gateway lifetime.
/// Resolution and dialing must authorize current membership independently.
/// Private refusals never fall back to Internet DNS, SOCKS or header injection.
#[async_trait::async_trait]
pub trait PrivateNetwork: Send + Sync {
    fn owns_name(&self, name: &str) -> bool;
    fn owns_address(&self, address: IpAddr) -> bool;
    async fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>>;
    async fn dial(&self, destination: SocketAddr) -> io::Result<Box<dyn Upstream>>;
    /// Generation-bound framed UDP transport; unsupported hooks refuse rather
    /// than falling back to an ordinary host UDP socket.
    async fn dial_udp(&self, _destination: SocketAddr) -> io::Result<Box<dyn Upstream>> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Plain TCP from the host.
pub struct SystemDialer;

#[async_trait::async_trait]
impl Dialer for SystemDialer {
    async fn dial(&self, destination: SocketAddr) -> io::Result<Box<dyn Upstream>> {
        let stream = tokio::net::TcpStream::connect(destination).await?;
        stream.set_nodelay(true)?;
        Ok(Box::new(stream))
    }
}

/// One thing the gateway decided, for the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub at: SystemTime,
    /// `tcp`, `dns`, `https-intercept` or `http-inject`.
    pub kind: &'static str,
    /// For DNS, the port is 53 and the address the gateway's own.
    pub destination: SocketAddr,
    pub name: Option<String>,
    pub verdict: Verdict,
    pub reason: String,
}

/// Counters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GatewayStats {
    pub connections_allowed: u64,
    pub connections_refused: u64,
    pub dns_answered: u64,
    pub dns_refused: u64,
    pub intercepted: u64,
}

/// How many decisions are kept.
const DECISION_LOG: usize = 512;

/// DNS answers remembered, address to the names that produced it.
const RESOLVED_LIMIT: usize = 8192;

struct Shared {
    config: GatewayConfig,
    policy: RwLock<NetworkPolicy>,
    resolver: Arc<dyn Resolver>,
    dialer: Arc<dyn Dialer>,
    private_network: Option<Arc<dyn PrivateNetwork>>,
    intercept: Option<Intercept>,
    egress_proxy: RwLock<Option<socks::Socks5Proxy>>,
    /// Workload tokens for placeholders in injected headers.
    tokens: RwLock<Option<mitm::TokenSource>>,
    secrets: RwLock<Option<Arc<crate::secret_substitution::Store>>>,
    resolved: Mutex<HashMap<IpAddr, BTreeSet<String>>>,
    log: Mutex<VecDeque<Decision>>,
    stats: Mutex<GatewayStats>,
}

#[derive(Clone)]
struct Intercept {
    authority: Arc<mitm::Authority>,
    server: Arc<rustls::ServerConfig>,
    client: Arc<rustls::ClientConfig>,
}

impl Shared {
    fn record(
        &self,
        kind: &'static str,
        destination: SocketAddr,
        name: Option<String>,
        verdict: Verdict,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        tracing::info!(
            target: "hv2_net::egress::audit",
            kind,
            %destination,
            name = name.as_deref().unwrap_or(""),
            verdict = ?verdict,
            reason = reason.as_str(),
            "egress decision"
        );
        {
            let mut stats = self.stats.lock();
            match (kind, verdict) {
                ("dns", Verdict::Allow) => stats.dns_answered += 1,
                ("dns", Verdict::Deny) => stats.dns_refused += 1,
                ("https-intercept" | "http-inject", _) => stats.intercepted += 1,
                (_, Verdict::Allow) => stats.connections_allowed += 1,
                (_, Verdict::Deny) => stats.connections_refused += 1,
            }
        }
        let mut log = self.log.lock();
        if log.len() == DECISION_LOG {
            log.pop_front();
        }
        log.push_back(Decision {
            at: SystemTime::now(),
            kind,
            destination,
            name,
            verdict,
            reason,
        });
    }

    fn names_for(&self, address: IpAddr) -> Vec<String> {
        self.resolved
            .lock()
            .get(&address)
            .map(|n| n.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn remember(&self, name: &str, addresses: &[IpAddr]) {
        let mut resolved = self.resolved.lock();
        if resolved.len() + addresses.len() > RESOLVED_LIMIT {
            resolved.clear();
        }
        for address in addresses {
            resolved
                .entry(*address)
                .or_default()
                .insert(name.to_string());
        }
    }
}

/// What a gateway's owner can do once the gateway itself has been handed to a
/// bridge: change the policy, and read what was decided.
#[derive(Clone)]
pub struct GatewayHandle(Arc<Shared>);

impl GatewayHandle {
    /// Replace the policy. Connections already open are left open; every
    /// connection and query after this is decided by the new one.
    pub fn set_policy(&self, policy: NetworkPolicy) {
        *self.0.policy.write() = policy;
    }

    /// Tunnel every allowed connection through `proxy` from now on, or stop
    /// tunnelling with `None`.
    pub fn set_egress_proxy(&self, proxy: Option<socks::Socks5Proxy>) {
        *self.0.egress_proxy.write() = proxy;
    }

    #[must_use]
    pub fn policy(&self) -> NetworkPolicy {
        self.0.policy.read().clone()
    }

    /// What allowed connections are tunnelled through, if anything.
    #[must_use]
    pub fn egress_proxy(&self) -> Option<socks::Socks5Proxy> {
        self.0.egress_proxy.read().clone()
    }

    /// Where workload tokens come from, for `${e2b.identity.tokens.NAME}`
    /// in an injected header. None: placeholders are sent as written.
    pub fn set_token_source(&self, tokens: Option<mitm::TokenSource>) {
        *self.0.tokens.write() = tokens;
    }

    /// Configure operator-owned host-bound secrets. TLS interception must
    /// already be enabled. Bindings never grant network access, and plaintext
    /// connections never substitute them. Rotate through the retained store;
    /// replacing this handle applies to newly intercepted connections.
    pub fn set_secret_store(
        &self,
        secrets: Option<Arc<crate::secret_substitution::Store>>,
    ) -> io::Result<()> {
        if secrets.is_some() && self.0.intercept.is_none() {
            return Err(io::Error::other(
                "host-bound secrets require TLS interception",
            ));
        }
        *self.0.secrets.write() = secrets;
        Ok(())
    }

    /// The most recent decisions, oldest first.
    #[must_use]
    pub fn decisions(&self) -> Vec<Decision> {
        self.0.log.lock().iter().cloned().collect()
    }

    #[must_use]
    pub fn stats(&self) -> GatewayStats {
        *self.0.stats.lock()
    }

    /// The CA a guest must trust for header injection, if interception is on.
    #[must_use]
    pub fn ca_pem(&self) -> Option<String> {
        self.0
            .intercept
            .as_ref()
            .map(|i| i.authority.ca_pem().to_string())
    }

    #[must_use]
    pub fn config(&self) -> &GatewayConfig {
        &self.0.config
    }
}

/// Builder for a [`Gateway`].
pub struct GatewayBuilder {
    config: GatewayConfig,
    policy: NetworkPolicy,
    resolver: Arc<dyn Resolver>,
    dialer: Arc<dyn Dialer>,
    private_network: Option<Arc<dyn PrivateNetwork>>,
    authority: Option<Arc<mitm::Authority>>,
    extra_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
}

impl GatewayBuilder {
    #[must_use]
    pub fn config(mut self, config: GatewayConfig) -> Self {
        self.config = config;
        self
    }
    #[must_use]
    pub fn resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }
    /// Install trusted private routing independently of Internet egress policy.
    #[must_use]
    pub fn private_network(mut self, private: Arc<dyn PrivateNetwork>) -> Self {
        self.private_network = Some(private);
        self
    }
    #[must_use]
    pub fn dialer(mut self, dialer: Arc<dyn Dialer>) -> Self {
        self.dialer = dialer;
        self
    }
    /// Enable header injection, signing leaves with `authority`. Without this,
    /// transform rules are refused at connection time rather than silently
    /// skipped -- a request the caller asked to carry a credential should
    /// not leave without it.
    #[must_use]
    pub fn intercept_with(mut self, authority: Arc<mitm::Authority>) -> Self {
        self.authority = Some(authority);
        self
    }
    /// Trust `root` for upstream servers, in addition to the web PKI.
    #[must_use]
    pub fn upstream_root(mut self, root: rustls::pki_types::CertificateDer<'static>) -> Self {
        self.extra_roots.push(root);
        self
    }

    /// Start the gateway. Must be called inside a tokio runtime.
    ///
    /// # Errors
    ///
    /// The TLS configuration could not be built.
    pub fn build(self) -> io::Result<Gateway> {
        let intercept = match self.authority {
            Some(authority) => Some(Intercept {
                server: authority.server_config()?,
                client: mitm::upstream_config(&self.extra_roots)?,
                authority,
            }),
            None => None,
        };
        let shared = Arc::new(Shared {
            config: self.config,
            policy: RwLock::new(self.policy),
            resolver: self.resolver,
            dialer: self.dialer,
            private_network: self.private_network,
            intercept,
            egress_proxy: RwLock::new(None),
            tokens: RwLock::new(None),
            secrets: RwLock::new(None),
            resolved: Mutex::new(HashMap::new()),
            log: Mutex::new(VecDeque::new()),
            stats: Mutex::new(GatewayStats::default()),
        });

        let (to_stack, frames_in) = mpsc::unbounded_channel();
        let (frames_out, from_stack) = mpsc::unbounded_channel();
        let ready = Arc::new(tokio::sync::Notify::new());
        let task = tokio::spawn(Stack::new(Arc::clone(&shared)).run(
            frames_in,
            frames_out,
            Arc::clone(&ready),
        ));
        Ok(Gateway {
            shared,
            to_stack,
            from_stack: Mutex::new(from_stack),
            ready,
            task,
        })
    }
}

/// The userspace router. Hand it to a [`Bridge`](crate::bridge::Bridge) as
/// its link; keep a [`GatewayHandle`] to change the policy afterwards.
pub struct Gateway {
    shared: Arc<Shared>,
    to_stack: mpsc::UnboundedSender<Vec<u8>>,
    from_stack: Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    /// Signalled when the stack has frames for the guest.
    ready: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Gateway {
    #[must_use]
    pub fn builder(policy: NetworkPolicy) -> GatewayBuilder {
        GatewayBuilder {
            config: GatewayConfig::default(),
            policy,
            resolver: Arc::new(SystemResolver),
            dialer: Arc::new(SystemDialer),
            private_network: None,
            authority: None,
            extra_roots: Vec::new(),
        }
    }

    #[must_use]
    pub fn handle(&self) -> GatewayHandle {
        GatewayHandle(Arc::clone(&self.shared))
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[async_trait::async_trait]
impl HostLink for Gateway {
    async fn send(&self, frame: &[u8]) -> crate::Result<usize> {
        self.to_stack
            .send(frame.to_vec())
            .map_err(|_| crate::NetError::Network("the gateway's stack has stopped".into()))?;
        Ok(frame.len())
    }

    async fn recv(&self) -> crate::Result<Vec<u8>> {
        match self.from_stack.lock().try_recv() {
            Ok(frame) => Ok(frame),
            Err(mpsc::error::TryRecvError::Empty) => Ok(Vec::new()),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(crate::NetError::Network(
                "the gateway's stack has stopped".into(),
            )),
        }
    }

    fn ready(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(Arc::clone(&self.ready))
    }
}

// ── The stack ───────────────────────────────────────────────────────────────

/// Frames in and out of smoltcp, as queues.
#[derive(Default)]
struct Queues {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
}

struct Rx(Vec<u8>);
struct Tx<'a>(&'a mut VecDeque<Vec<u8>>);

impl smoltcp::phy::RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl smoltcp::phy::TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut frame = vec![0u8; len];
        let result = f(&mut frame);
        self.0.push_back(frame);
        result
    }
}

impl Device for Queues {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;

    fn receive(
        &mut self,
        _: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.rx.pop_front()?;
        Some((Rx(frame), Tx(&mut self.tx)))
    }

    fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(Tx(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = hv2_core::devices::virtio_net_mmio::MAX_FRAME_LEN;
        caps
    }
}

/// What a connection task tells the stack.
enum Event {
    /// Answer the held SYN.
    Accept(u64),
    /// Reset the connection.
    Abort(u64),
    /// The task will write no more; send FIN once what it wrote is out.
    WriteClosed(u64),
    /// There is something to look at.
    Wake,
    Dns {
        to: smoltcp::wire::IpEndpoint,
        reply: Vec<u8>,
    },
}

struct Conn {
    handle: SocketHandle,
    /// Guest bytes to the task. `None` once the guest has closed its side.
    to_task: Option<mpsc::Sender<Vec<u8>>>,
    from_task: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    write_closed: bool,
    created: std::time::Instant,
    key: (SocketAddr, SocketAddr),
}

/// How a connection is to be handled, decided at its SYN.
#[derive(Debug, Clone)]
enum Plan {
    Private,
    /// Dial first; the address is allowed. `via_names` is set when that is
    /// only because this gateway's DNS answered an allowed name with it.
    DialFirst {
        reason: &'static str,
        via_names: Option<Vec<String>>,
    },
    /// Accept, read the name, then decide.
    NameFirst,
}

/// Socket buffers per connection. The send side bounds what can be in flight
/// to the guest, so it bounds a download at buffer / round trip: 64 KiB over a
/// 3.5 ms round trip measured 15.5 MB/s, exactly that ceiling.
const TCP_TX_BUFFER: usize = 256 * 1024;
/// The window the guest may send into: the same bound on an upload that the
/// send side is on a download.
const TCP_RX_BUFFER: usize = 256 * 1024;
/// The largest chunk a task hands the stack at once.
const TASK_CHUNK: usize = 16 * 1024;
/// A listening socket whose SYN never took. Only a malformed frame gets here.
const LISTEN_TIMEOUT: Duration = Duration::from_secs(5);

struct PrivateUdpSession {
    to_peer: mpsc::Sender<Vec<u8>>,
    from_peer: mpsc::Receiver<Vec<u8>>,
    pending: Option<Vec<u8>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for PrivateUdpSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct PrivateUdpSocket {
    handle: SocketHandle,
    last_used: std::time::Instant,
}
struct Stack {
    shared: Arc<Shared>,
    start: std::time::Instant,
    device: Queues,
    iface: Interface,
    sockets: SocketSet<'static>,
    dns: SocketHandle,
    conns: HashMap<u64, Conn>,
    keys: HashMap<(SocketAddr, SocketAddr), u64>,
    next_id: AtomicU64,
    udp_sockets: HashMap<SocketAddr, PrivateUdpSocket>,
    udp_sessions: HashMap<(SocketAddr, SocketAddr), PrivateUdpSession>,
}

impl Stack {
    fn new(shared: Arc<Shared>) -> Self {
        let start = std::time::Instant::now();
        let mut device = Queues::default();
        let config = &shared.config;
        let mut iface_config = Config::new(HardwareAddress::Ethernet(EthernetAddress(config.mac)));
        iface_config.random_seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0x5eed, |d| d.as_nanos() as u64);
        let mut iface = Interface::new(iface_config, &mut device, smoltcp::time::Instant::ZERO);
        iface.update_ip_addrs(|addrs| {
            // Both fit: the capacity is a compile-time constant above two.
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(config.gateway), config.prefix));
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(config.dns), config.prefix));
        });
        // Accept packets for any destination: the guest's default route points
        // here, so every address it connects to is one this stack must answer
        // for. `any_ip` requires a route through one of our own addresses.
        iface.set_any_ip(true);
        // Two fixed receive buffers; incomplete fragment sets expire promptly.
        iface.set_reassembly_timeout(smoltcp::time::Duration::from_secs(5));
        let _ = iface.routes_mut().add_default_ipv4_route(config.gateway);

        let mut sockets = SocketSet::new(Vec::new());
        let mut dns = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 32], vec![0; 16 * 1024]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 32], vec![0; 16 * 1024]),
        );
        // Cannot fail: the port is non-zero and the socket unbound.
        let _ = dns.bind((IpAddress::Ipv4(config.dns), 53));
        let dns = sockets.add(dns);

        Self {
            shared,
            start,
            device,
            iface,
            sockets,
            dns,
            conns: HashMap::new(),
            keys: HashMap::new(),
            next_id: AtomicU64::new(1),
            udp_sockets: HashMap::new(),
            udp_sessions: HashMap::new(),
        }
    }

    fn now(&self) -> smoltcp::time::Instant {
        smoltcp::time::Instant::from_micros(self.start.elapsed().as_micros() as i64)
    }

    async fn run(
        mut self,
        mut frames_in: mpsc::UnboundedReceiver<Vec<u8>>,
        frames_out: mpsc::UnboundedSender<Vec<u8>>,
        ready: Arc<tokio::sync::Notify>,
    ) {
        let (events_tx, mut events) = mpsc::unbounded_channel();
        loop {
            while let Ok(frame) = frames_in.try_recv() {
                self.ingest(frame, &events_tx);
            }
            while let Ok(event) = events.try_recv() {
                self.handle_event(event);
            }

            let now = self.now();
            self.iface.poll(now, &mut self.device, &mut self.sockets);
            self.service_dns(&events_tx);
            self.service_private_udp();
            let blocked = self.service_connections();
            self.iface.poll(now, &mut self.device, &mut self.sockets);

            let mut sent = false;
            while let Some(frame) = self.device.tx.pop_front() {
                if frames_out.send(frame).is_err() {
                    return;
                }
                sent = true;
            }
            if sent {
                ready.notify_one();
            }

            let mut wait = self
                .iface
                .poll_delay(self.now(), &self.sockets)
                .map_or(Duration::from_secs(1), |d| {
                    Duration::from_micros(d.total_micros())
                });
            // A connection whose task is not reading has nothing to wake us
            // when it starts again; look back soon.
            if blocked || !self.udp_sockets.is_empty() {
                wait = wait.min(Duration::from_millis(2));
            }
            tokio::select! {
                frame = frames_in.recv() => match frame {
                    Some(frame) => self.ingest(frame, &events_tx),
                    None => return,
                },
                Some(event) = events.recv() => self.handle_event(event),
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// Queue a frame for the stack, creating a listening socket first if it
    /// is the SYN of a connection this policy may allow.
    fn ingest(&mut self, frame: Vec<u8>, events: &mpsc::UnboundedSender<Event>) {
        if let Some((source, destination)) = syn_of(&frame) {
            self.on_syn(source, destination, events);
        }
        if let Some(destination) = udp_destination_of(&frame) {
            self.on_private_udp(destination);
        }
        self.device.rx.push_back(frame);
    }

    fn on_private_udp(&mut self, destination: SocketAddr) {
        if self.udp_sockets.contains_key(&destination) {
            return;
        }
        if destination.port() == 0
            || self.conns.len() + self.udp_sockets.len() >= self.shared.config.max_connections
            || !self
                .shared
                .private_network
                .as_ref()
                .is_some_and(|p| p.owns_address(destination.ip()))
        {
            return;
        }
        let IpAddr::V4(ip) = destination.ip() else {
            return;
        };
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; private_udp::QUEUE_DEPTH],
                vec![0; private_udp::MAX_PAYLOAD * private_udp::QUEUE_DEPTH],
            ),
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; private_udp::QUEUE_DEPTH],
                vec![0; private_udp::MAX_PAYLOAD * private_udp::QUEUE_DEPTH],
            ),
        );
        if socket
            .bind((IpAddress::Ipv4(ip), destination.port()))
            .is_err()
        {
            return;
        }
        let handle = self.sockets.add(socket);
        self.udp_sockets.insert(
            destination,
            PrivateUdpSocket {
                handle,
                last_used: std::time::Instant::now(),
            },
        );
    }

    fn service_private_udp(&mut self) {
        let destinations: Vec<_> = self.udp_sockets.keys().copied().collect();
        for destination in destinations {
            let handle = self.udp_sockets[&destination].handle;
            for _ in 0..private_udp::QUEUE_DEPTH {
                let received = self
                    .sockets
                    .get_mut::<udp::Socket>(handle)
                    .recv()
                    .map(|(data, meta)| (data.to_vec(), meta.endpoint));
                let Ok((payload, endpoint)) = received else {
                    break;
                };
                let IpAddress::Ipv4(source_ip) = endpoint.addr;
                let source = SocketAddr::new(IpAddr::V4(source_ip), endpoint.port);
                let key = (source, destination);
                if !self.udp_sessions.contains_key(&key) {
                    if self.conns.len() + self.udp_sessions.len()
                        >= self.shared.config.max_connections
                    {
                        continue;
                    }
                    let Some(private) = self.shared.private_network.clone() else {
                        continue;
                    };
                    let (to_peer, input) = mpsc::channel(private_udp::QUEUE_DEPTH);
                    let (output, from_peer) = mpsc::channel(private_udp::QUEUE_DEPTH);
                    let shared = self.shared.clone();
                    let task = tokio::spawn(async move {
                        let stream = match tokio::time::timeout(
                            shared.config.connect_timeout,
                            private.dial_udp(destination),
                        )
                        .await
                        {
                            Ok(Ok(stream)) => stream,
                            _ => {
                                shared.record(
                                    "udp",
                                    destination,
                                    None,
                                    Verdict::Deny,
                                    "private network refused",
                                );
                                return;
                            }
                        };
                        shared.record("udp", destination, None, Verdict::Allow, "private network");
                        let _ = private_udp::relay(
                            stream,
                            input,
                            output,
                            Duration::from_secs(30),
                            Duration::from_secs(600),
                        )
                        .await;
                    });
                    self.udp_sessions.insert(
                        key,
                        PrivateUdpSession {
                            to_peer,
                            from_peer,
                            pending: None,
                            task,
                        },
                    );
                }
                // UDP pressure drops whole packets, never partial frame bytes.
                let _ = self.udp_sessions[&key].to_peer.try_send(payload);
                self.udp_sockets.get_mut(&destination).unwrap().last_used =
                    std::time::Instant::now();
            }
        }
        let keys: Vec<_> = self.udp_sessions.keys().copied().collect();
        for key @ (source, destination) in keys {
            let session = self.udp_sessions.get_mut(&key).unwrap();
            let handle = self.udp_sockets[&destination].handle;
            for _ in 0..private_udp::QUEUE_DEPTH {
                let payload = match session
                    .pending
                    .take()
                    .or_else(|| session.from_peer.try_recv().ok())
                {
                    Some(payload) => payload,
                    None => break,
                };
                let IpAddr::V4(ip) = source.ip() else {
                    break;
                };
                if self
                    .sockets
                    .get_mut::<udp::Socket>(handle)
                    .send_slice(&payload, (IpAddress::Ipv4(ip), source.port()))
                    .is_err()
                {
                    session.pending = Some(payload);
                    break;
                }
                self.udp_sockets.get_mut(&destination).unwrap().last_used =
                    std::time::Instant::now();
            }
            if session.task.is_finished()
                && session.pending.is_none()
                && session.from_peer.is_empty()
            {
                self.udp_sessions.remove(&key);
            }
        }
        let unused: Vec<_> = self
            .udp_sockets
            .iter()
            .filter_map(|(destination, socket)| {
                (!self.udp_sessions.keys().any(|(_, to)| to == destination)
                    && socket.last_used.elapsed() > Duration::from_secs(5)
                    && self.sockets.get::<udp::Socket>(socket.handle).send_queue() == 0)
                    .then_some(*destination)
            })
            .collect();
        for destination in unused {
            let socket = self.udp_sockets.remove(&destination).unwrap();
            self.sockets.remove(socket.handle);
        }
    }

    fn on_syn(
        &mut self,
        source: SocketAddr,
        destination: SocketAddr,
        events: &mpsc::UnboundedSender<Event>,
    ) {
        let key = (source, destination);
        if self.keys.contains_key(&key) {
            return; // a retransmitted SYN
        }
        let config = &self.shared.config;
        let address = destination.ip();

        let plan = if address == IpAddr::V4(config.gateway) || address == IpAddr::V4(config.dns) {
            Err("the gateway runs no TCP services")
        } else if self.conns.len() + self.udp_sessions.len() >= config.max_connections {
            Err("too many open connections")
        } else if self
            .shared
            .private_network
            .as_ref()
            .is_some_and(|p| p.owns_address(address))
        {
            Ok(Plan::Private)
        } else {
            let policy = self.shared.policy.read();
            match policy.decide_address(address) {
                AddressVerdict::Allow(reason) => Ok(Plan::DialFirst {
                    reason,
                    via_names: None,
                }),
                AddressVerdict::Deny(reason) => Err(reason),
                AddressVerdict::NeedsName => {
                    let names: Vec<String> = self
                        .shared
                        .names_for(address)
                        .into_iter()
                        .filter(|n| policy.decide(address, n).0 == Verdict::Allow)
                        .collect();
                    if names.is_empty() {
                        Ok(Plan::NameFirst)
                    } else {
                        Ok(Plan::DialFirst {
                            reason: "allowOut name, by this gateway's DNS answer",
                            via_names: Some(names),
                        })
                    }
                }
            }
        };

        let plan = match plan {
            Ok(plan) => plan,
            Err(reason) => {
                // No socket: the stack answers this SYN with a RST.
                self.shared
                    .record("tcp", destination, None, Verdict::Deny, reason);
                return;
            }
        };

        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; TCP_RX_BUFFER]),
            tcp::SocketBuffer::new(vec![0; TCP_TX_BUFFER]),
        );
        // `syn_of` reads IPv4 only, so this is always the first arm.
        let IpAddr::V4(listen_on) = destination.ip() else {
            return;
        };
        if socket
            .listen((IpAddress::Ipv4(listen_on), destination.port()))
            .is_err()
        {
            return;
        }
        socket.set_nagle_enabled(false);
        // Acknowledge at once. smoltcp delays an ACK up to 10 ms by default,
        // which, against the guest's sender, made every window's worth of an
        // upload wait out that delay: 64 KiB per ~11 ms, measured 5.9 MB/s,
        // while downloads -- which the guest acknowledges -- ran at 130.
        socket.set_ack_delay(None);
        socket.set_timeout(Some(smoltcp::time::Duration::from_secs(120)));
        socket.pause_synack(matches!(plan, Plan::DialFirst { .. } | Plan::Private));
        let handle = self.sockets.add(socket);

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (to_task, task_rx) = mpsc::channel(16);
        let (task_tx, from_task) = mpsc::channel(16);
        self.conns.insert(
            id,
            Conn {
                handle,
                to_task: Some(to_task),
                from_task,
                pending: Vec::new(),
                write_closed: false,
                created: std::time::Instant::now(),
                key,
            },
        );
        self.keys.insert(key, id);

        let guest = GuestStream::new(id, task_rx, task_tx, events.clone());
        tokio::spawn(connection(
            Arc::clone(&self.shared),
            id,
            plan,
            guest,
            destination,
            events.clone(),
        ));
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Accept(id) => {
                if let Some(conn) = self.conns.get(&id) {
                    self.sockets
                        .get_mut::<tcp::Socket>(conn.handle)
                        .pause_synack(false);
                }
            }
            Event::Abort(id) => {
                if let Some(conn) = self.conns.get(&id) {
                    self.sockets.get_mut::<tcp::Socket>(conn.handle).abort();
                }
            }
            Event::WriteClosed(id) => {
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.write_closed = true;
                }
            }
            Event::Wake => {}
            Event::Dns { to, reply } => {
                let socket = self.sockets.get_mut::<udp::Socket>(self.dns);
                if let Err(e) = socket.send_slice(&reply, to) {
                    tracing::debug!("gateway: a DNS answer could not be queued: {e}");
                }
            }
        }
    }

    fn service_dns(&mut self, events: &mpsc::UnboundedSender<Event>) {
        loop {
            let (query, from) = {
                let socket = self.sockets.get_mut::<udp::Socket>(self.dns);
                match socket.recv() {
                    Ok((data, meta)) => (dns::parse_query(data), meta.endpoint),
                    Err(_) => return,
                }
            };
            let Some(query) = query else { continue };
            let shared = Arc::clone(&self.shared);
            let events = events.clone();
            let dns_addr = SocketAddr::new(IpAddr::V4(shared.config.dns), 53);
            tokio::spawn(async move {
                let reply = resolve_query(&shared, &query, dns_addr).await;
                let _ = events.send(Event::Dns { to: from, reply });
            });
        }
    }

    /// Move bytes between sockets and tasks. Returns whether any connection
    /// is waiting on a task that is not reading.
    fn service_connections(&mut self) -> bool {
        let mut blocked = false;
        let mut finished = Vec::new();

        for (id, conn) in &mut self.conns {
            let socket = self.sockets.get_mut::<tcp::Socket>(conn.handle);

            // Guest to task.
            if let Some(to_task) = &conn.to_task {
                while socket.can_recv() {
                    match to_task.try_reserve() {
                        Ok(permit) => {
                            if let Ok(data) = socket.recv(|buf| (buf.len(), buf.to_vec())) {
                                permit.send(data);
                            }
                        }
                        Err(mpsc::error::TrySendError::Full(())) => {
                            blocked = true;
                            break;
                        }
                        Err(mpsc::error::TrySendError::Closed(())) => {
                            socket.abort();
                            break;
                        }
                    }
                }
                let guest_closed = matches!(
                    socket.state(),
                    tcp::State::CloseWait
                        | tcp::State::LastAck
                        | tcp::State::Closing
                        | tcp::State::TimeWait
                        | tcp::State::Closed
                );
                if guest_closed && socket.recv_queue() == 0 {
                    conn.to_task = None;
                }
            }

            // Task to guest.
            if socket.may_send() {
                loop {
                    if !conn.pending.is_empty() {
                        match socket.send_slice(&conn.pending) {
                            Ok(n) => {
                                conn.pending.drain(..n);
                            }
                            Err(_) => break,
                        }
                        if !conn.pending.is_empty() {
                            break;
                        }
                    }
                    match conn.from_task.try_recv() {
                        Ok(data) => conn.pending = data,
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            conn.write_closed = true;
                            break;
                        }
                    }
                }
                if conn.write_closed && conn.pending.is_empty() {
                    socket.close();
                }
            }

            let done = match socket.state() {
                tcp::State::Closed | tcp::State::TimeWait => true,
                tcp::State::Listen => conn.created.elapsed() > LISTEN_TIMEOUT,
                _ => false,
            };
            if done {
                finished.push(*id);
            }
        }

        for id in finished {
            if let Some(conn) = self.conns.remove(&id) {
                self.keys.remove(&conn.key);
                self.sockets.remove(conn.handle);
            }
        }
        blocked
    }
}

/// Only complete IPv4 UDP headers create private destination sockets. The stack
/// validates checksums before delivering their payload; other UDP stays dropped.
fn udp_destination_of(frame: &[u8]) -> Option<SocketAddr> {
    let eth = EthernetFrame::new_checked(frame).ok()?;
    if eth.ethertype() != EthernetProtocol::Ipv4 {
        return None;
    }
    let ip = Ipv4Packet::new_checked(eth.payload()).ok()?;
    if ip.next_header() != IpProtocol::Udp || ip.frag_offset() != 0 {
        return None;
    }
    // The first fragment contains the UDP header but not its declared whole
    // payload. Create only the bounded destination socket; the interface must
    // reassemble and validate the complete packet before session admission.
    if ip.payload().len() < 8 {
        return None;
    }
    let udp = UdpPacket::new_unchecked(ip.payload());
    Some(SocketAddr::new(IpAddr::V4(ip.dst_addr()), udp.dst_port()))
}

/// The source and destination of a TCP SYN, if `frame` is one.
fn syn_of(frame: &[u8]) -> Option<(SocketAddr, SocketAddr)> {
    let eth = EthernetFrame::new_checked(frame).ok()?;
    if eth.ethertype() != EthernetProtocol::Ipv4 {
        return None;
    }
    let ip = Ipv4Packet::new_checked(eth.payload()).ok()?;
    if ip.next_header() != IpProtocol::Tcp {
        return None;
    }
    let tcp = TcpPacket::new_checked(ip.payload()).ok()?;
    if !tcp.syn() || tcp.ack() {
        return None;
    }
    Some((
        SocketAddr::new(IpAddr::V4(ip.src_addr()), tcp.src_port()),
        SocketAddr::new(IpAddr::V4(ip.dst_addr()), tcp.dst_port()),
    ))
}

async fn resolve_query(shared: &Shared, query: &dns::Query, dns_addr: SocketAddr) -> Vec<u8> {
    let name = query.name.clone();
    if query.qtype != dns::TYPE_A && query.qtype != dns::TYPE_AAAA {
        return dns::answer(query, dns::Rcode::NotImp, &[], 0);
    }
    if let Some(private) = shared
        .private_network
        .as_ref()
        .filter(|p| p.owns_name(&name))
    {
        let result =
            tokio::time::timeout(shared.config.connect_timeout, private.resolve(&name)).await;
        let (code, addresses) = match result {
            Ok(Ok(addresses))
                if !addresses.is_empty() && addresses.iter().all(|a| private.owns_address(*a)) =>
            {
                (dns::Rcode::NoError, addresses)
            }
            _ => (dns::Rcode::Refused, Vec::new()),
        };
        shared.record(
            "dns",
            dns_addr,
            Some(name),
            if addresses.is_empty() {
                Verdict::Deny
            } else {
                Verdict::Allow
            },
            "private network lookup",
        );
        // Never remember private names as ordinary Internet policy exceptions.
        return dns::answer(query, code, &addresses, 0);
    }
    let may = shared.policy.read().may_resolve(&name);
    if !may {
        shared.record(
            "dns",
            dns_addr,
            Some(name),
            Verdict::Deny,
            "the policy would refuse where this name leads",
        );
        return dns::answer(query, dns::Rcode::Refused, &[], 0);
    }
    match shared.resolver.resolve(&name).await {
        Ok(addresses) if !addresses.is_empty() => {
            shared.remember(&name, &addresses);
            shared.record("dns", dns_addr, Some(name), Verdict::Allow, "resolved");
            dns::answer(
                query,
                dns::Rcode::NoError,
                &addresses,
                shared.config.dns_ttl,
            )
        }
        Ok(_) => dns::answer(query, dns::Rcode::NxDomain, &[], 0),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            dns::answer(query, dns::Rcode::NxDomain, &[], 0)
        }
        Err(e) => {
            tracing::debug!("gateway: resolving {name}: {e}");
            // `lookup_host` reports an unknown name as a generic error on most
            // platforms, and SERVFAIL makes a stub resolver retry for seconds.
            dns::answer(query, dns::Rcode::NxDomain, &[], 0)
        }
    }
}

// ── Connections ─────────────────────────────────────────────────────────────

/// One guest connection, from the task's side.
struct GuestStream {
    id: u64,
    rx: mpsc::Receiver<Vec<u8>>,
    buffered: Vec<u8>,
    tx: poll_sender::PollSender,
    events: mpsc::UnboundedSender<Event>,
    shut: bool,
}

impl GuestStream {
    fn new(
        id: u64,
        rx: mpsc::Receiver<Vec<u8>>,
        tx: mpsc::Sender<Vec<u8>>,
        events: mpsc::UnboundedSender<Event>,
    ) -> Self {
        Self {
            id,
            rx,
            buffered: Vec::new(),
            tx: poll_sender::PollSender::new(tx),
            events,
            shut: false,
        }
    }
}

impl AsyncRead for GuestStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.buffered.is_empty() {
            // A full channel is one the stack stopped filling; taking from it
            // is the moment it can fill again, and nothing else tells it so.
            let was_full = self.rx.len() >= self.rx.max_capacity();
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(data)) => {
                    self.buffered = data;
                    if was_full {
                        let _ = self.events.send(Event::Wake);
                    }
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = self.buffered.len().min(buf.remaining());
        buf.put_slice(&self.buffered[..n]);
        self.buffered.drain(..n);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for GuestStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.tx.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(())) => {
                return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
            }
            Poll::Pending => return Poll::Pending,
        }
        let n = data.len().min(TASK_CHUNK);
        if self.tx.send_item(data[..n].to_vec()).is_err() {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        let _ = self.events.send(Event::Wake);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shut {
            self.shut = true;
            let id = self.id;
            let _ = self.events.send(Event::WriteClosed(id));
        }
        Poll::Ready(Ok(()))
    }
}

/// A minimal poll-based sender over a bounded channel, so `GuestStream` can
/// implement `AsyncWrite` with backpressure.
mod poll_sender {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use tokio::sync::mpsc::{OwnedPermit, Sender};

    type Reserve = Pin<Box<dyn Future<Output = Result<OwnedPermit<Vec<u8>>, ()>> + Send>>;

    pub(super) struct PollSender {
        sender: Option<Sender<Vec<u8>>>,
        reserving: Option<Reserve>,
        permit: Option<OwnedPermit<Vec<u8>>>,
    }

    impl PollSender {
        pub(super) fn new(sender: Sender<Vec<u8>>) -> Self {
            Self {
                sender: Some(sender),
                reserving: None,
                permit: None,
            }
        }

        pub(super) fn poll_reserve(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
            if self.permit.is_some() {
                return Poll::Ready(Ok(()));
            }
            if self.reserving.is_none() {
                let Some(sender) = self.sender.take() else {
                    return Poll::Ready(Err(()));
                };
                self.reserving = Some(Box::pin(async move {
                    sender.reserve_owned().await.map_err(|_| ())
                }));
            }
            let Some(fut) = self.reserving.as_mut() else {
                return Poll::Ready(Err(()));
            };
            match fut.as_mut().poll(cx) {
                Poll::Ready(Ok(permit)) => {
                    self.reserving = None;
                    self.permit = Some(permit);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(())) => {
                    self.reserving = None;
                    Poll::Ready(Err(()))
                }
                Poll::Pending => Poll::Pending,
            }
        }

        pub(super) fn send_item(&mut self, item: Vec<u8>) -> Result<(), ()> {
            let permit = self.permit.take().ok_or(())?;
            self.sender = Some(permit.send(item));
            Ok(())
        }
    }
}

/// A stream that yields `prefix` before `inner`'s own bytes.
struct Prefixed<S> {
    prefix: Vec<u8>,
    inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.prefix.is_empty() {
            let n = self.prefix.len().min(buf.remaining());
            buf.put_slice(&self.prefix[..n]);
            self.prefix.drain(..n);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Read from `guest` until what it sent names something, or can't.
async fn read_name(guest: &mut GuestStream, wait: Duration) -> (Vec<u8>, Sniffed) {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + wait;
    let mut chunk = vec![0u8; 4096];
    loop {
        let sniffed = sniff::sniff(&seen);
        if sniffed != Sniffed::Incomplete || seen.len() >= sniff::SNIFF_LIMIT {
            return (seen, sniffed);
        }
        match tokio::time::timeout_at(deadline, guest.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => {
                let sniffed = if seen.is_empty() {
                    Sniffed::Incomplete
                } else {
                    sniff::sniff(&seen)
                };
                return (seen, sniffed);
            }
            Ok(Ok(n)) => seen.extend_from_slice(&chunk[..n]),
        }
    }
}

fn named(sniffed: &Sniffed) -> Option<&str> {
    match sniffed {
        Sniffed::Tls(Some(name)) | Sniffed::Http(Some(name)) => Some(name),
        _ => None,
    }
}

/// Open the host's side of a connection: directly, or through the sandbox's
/// egress proxy when it has one. `name` is sent to the proxy in place of the
/// address when the flow is known to be for it.
async fn dial(
    shared: &Shared,
    destination: SocketAddr,
    name: Option<&str>,
) -> io::Result<Box<dyn Upstream>> {
    let proxy = shared.egress_proxy.read().clone();
    let attempt = async {
        match proxy {
            None => shared.dialer.dial(destination).await,
            Some(proxy) => {
                let stream = tokio::net::TcpStream::connect(&proxy.address).await?;
                stream.set_nodelay(true)?;
                let mut stream: Box<dyn Upstream> = Box::new(stream);
                proxy.handshake(&mut stream, destination, name).await?;
                Ok(stream)
            }
        }
    };
    match tokio::time::timeout(shared.config.connect_timeout, attempt).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::from(io::ErrorKind::TimedOut)),
    }
}

/// Handle one guest connection to the end.
async fn connection(
    shared: Arc<Shared>,
    id: u64,
    plan: Plan,
    mut guest: GuestStream,
    destination: SocketAddr,
    events: mpsc::UnboundedSender<Event>,
) {
    let config = shared.config.clone();
    let refuse = |name: Option<String>, reason: String| {
        shared.record("tcp", destination, name, Verdict::Deny, reason);
        let _ = events.send(Event::Abort(id));
    };
    let (upstream, prefix, sniffed, name, reason) = match plan {
        Plan::Private => {
            let Some(private) = shared.private_network.as_ref() else {
                refuse(None, "private network unavailable".into());
                return;
            };
            let upstream =
                match tokio::time::timeout(config.connect_timeout, private.dial(destination)).await
                {
                    Ok(Ok(stream)) => stream,
                    _ => {
                        refuse(None, "private network refused".into());
                        return;
                    }
                };
            let _ = events.send(Event::Accept(id));
            // Opaque bytes: never sniff private traffic or substitute secrets.
            (
                upstream,
                Vec::new(),
                Sniffed::Incomplete,
                None,
                "private network".into(),
            )
        }

        Plan::DialFirst { reason, via_names } => {
            let known = via_names.as_ref().and_then(|n| n.first()).cloned();
            let upstream = match dial(&shared, destination, known.as_deref()).await {
                Ok(upstream) => upstream,
                Err(e) => {
                    refuse(None, format!("upstream: {e}"));
                    return;
                }
            };
            let _ = events.send(Event::Accept(id));

            // Only look inside when the answer could change something.
            let look = via_names.is_some()
                || shared.policy.read().has_transforms()
                || shared.secrets.read().is_some();
            let (prefix, sniffed) = if look {
                read_name(&mut guest, config.sniff_wait).await
            } else {
                (Vec::new(), Sniffed::Incomplete)
            };
            let name = named(&sniffed).map(str::to_string);
            if let (Some(allowed), Sniffed::Tls(Some(sni))) = (&via_names, &sniffed) {
                let verdict = shared.policy.read().decide(destination.ip(), sni).0;
                if verdict != Verdict::Allow {
                    refuse(
                        Some(sni.clone()),
                        format!(
                            "SNI names something the policy refuses; the address was allowed \
                             only as {}",
                            allowed.join(", ")
                        ),
                    );
                    return;
                }
            }
            let name = name.or_else(|| via_names.and_then(|n| n.into_iter().next()));
            (upstream, prefix, sniffed, name, reason.to_string())
        }
        Plan::NameFirst => {
            let _ = events.send(Event::Accept(id));
            let (prefix, sniffed) = read_name(&mut guest, config.name_timeout).await;
            let Some(name) = named(&sniffed).map(str::to_string) else {
                refuse(
                    None,
                    "the address needs a name rule, and the client sent no name".to_string(),
                );
                return;
            };
            let (verdict, reason) = shared.policy.read().decide(destination.ip(), &name);
            if verdict != Verdict::Allow {
                refuse(Some(name), reason.to_string());
                return;
            }
            // The name is allowed; is this where it actually points?
            match shared.resolver.resolve(&name).await {
                Ok(addresses) if addresses.contains(&destination.ip()) => {
                    shared.remember(&name, &addresses);
                }
                Ok(_) | Err(_) => {
                    refuse(
                        Some(name),
                        "the name does not resolve to the address connected to".to_string(),
                    );
                    return;
                }
            }
            let upstream = match dial(&shared, destination, Some(&name)).await {
                Ok(upstream) => upstream,
                Err(e) => {
                    refuse(Some(name), format!("upstream: {e}"));
                    return;
                }
            };
            (upstream, prefix, sniffed, Some(name), reason.to_string())
        }
    };

    // Header injection, for a TLS connection to a name with a rule.
    if let (Sniffed::Tls(Some(sni)), Some(_)) = (&sniffed, &name) {
        let headers = shared.policy.read().transform_for(sni).cloned();
        let secrets = shared
            .secrets
            .read()
            .clone()
            .filter(|store| store.has_host(sni));
        if headers.is_some() || secrets.is_some() {
            let headers = headers.unwrap_or_default();
            let Some(intercept) = shared.intercept.clone() else {
                refuse(
                    Some(sni.clone()),
                    "a transform rule names this host, and interception is not enabled".to_string(),
                );
                return;
            };
            shared.record(
                "https-intercept",
                destination,
                Some(sni.clone()),
                Verdict::Allow,
                format!("{reason}; {} header(s) injected", headers.len()),
            );
            let guest = Prefixed {
                prefix,
                inner: guest,
            };
            let tokens = shared.tokens.read().clone();
            let result = if let Some(secrets) = secrets {
                mitm::intercept_with_secrets(
                    guest,
                    upstream,
                    sni,
                    &headers,
                    tokens,
                    intercept.server,
                    intercept.client,
                    secrets,
                )
                .await
            } else {
                mitm::intercept(
                    guest,
                    upstream,
                    sni,
                    &headers,
                    tokens,
                    intercept.server,
                    intercept.client,
                )
                .await
            };
            if let Err(e) = result {
                tracing::debug!("gateway: intercepted connection to {sni} ended: {e}");
            }
            let _ = events.send(Event::WriteClosed(id));
            return;
        }
    }

    // Header injection into plain HTTP. No certificate will vouch for the
    // upstream here, so the address has to: without this check a guest sends
    // `Host: api.vendor.example` to a server of its own and collects the
    // credential the rule was written to protect.
    if let Sniffed::Http(Some(host)) = &sniffed {
        let headers = shared.policy.read().transform_for(host).cloned();
        if let Some(headers) = headers {
            let resolves_here = match shared.resolver.resolve(host).await {
                Ok(addresses) => addresses.contains(&destination.ip()),
                Err(_) => false,
            };
            if !resolves_here {
                refuse(
                    Some(host.clone()),
                    "a transform rule names this host, and the address connected to is not \
                     where it resolves"
                        .to_string(),
                );
                return;
            }
            shared.record(
                "http-inject",
                destination,
                Some(host.clone()),
                Verdict::Allow,
                format!("{reason}; {} header(s) injected", headers.len()),
            );
            let guest = Prefixed {
                prefix,
                inner: guest,
            };
            let tokens = shared.tokens.read().clone();
            if let Err(e) = mitm::relay_http(guest, upstream, &headers, tokens).await {
                tracing::debug!("gateway: injected HTTP connection to {host} ended: {e}");
            }
            let _ = events.send(Event::WriteClosed(id));
            return;
        }
    }

    shared.record("tcp", destination, name, Verdict::Allow, reason);
    let mut upstream = upstream;
    if !prefix.is_empty() && upstream.write_all(&prefix).await.is_err() {
        let _ = events.send(Event::Abort(id));
        return;
    }
    match tokio::io::copy_bidirectional(&mut guest, &mut upstream).await {
        Ok(_) => {
            let _ = guest.shutdown().await;
        }
        Err(_) => {
            let _ = events.send(Event::Abort(id));
        }
    }
}

#[cfg(test)]
mod tests;
