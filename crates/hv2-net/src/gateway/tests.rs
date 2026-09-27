//! The gateway against a real TCP/IP stack standing in for the guest.
//!
//! The guest here is a second smoltcp interface, exchanging real Ethernet
//! frames with the gateway through its [`HostLink`] side -- ARP, TCP
//! handshakes, resets and DNS all happen as they would with a Linux guest on
//! the other end of a virtio-net device. The outside world is local listeners,
//! reached through a dialer that maps public-looking addresses onto them.

use super::*;
use std::collections::HashMap;
use std::net::SocketAddr;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::network_policy::{Headers, NetworkPolicy, Verdict};

const EXAMPLE: Ipv4Addr = Ipv4Addr::new(93, 184, 216, 34);
const OTHER: Ipv4Addr = Ipv4Addr::new(198, 18, 0, 7);

struct MapResolver(HashMap<String, Vec<IpAddr>>);

#[async_trait::async_trait]
impl Resolver for MapResolver {
    async fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

/// Maps public-looking destinations onto local listeners.
struct MapDialer(HashMap<SocketAddr, SocketAddr>);

#[async_trait::async_trait]
impl Dialer for MapDialer {
    async fn dial(&self, destination: SocketAddr) -> io::Result<Box<dyn Upstream>> {
        let real = self
            .0
            .get(&destination)
            .ok_or_else(|| io::Error::from(io::ErrorKind::ConnectionRefused))?;
        Ok(Box::new(tokio::net::TcpStream::connect(real).await?))
    }
}

/// A local server that echoes, prefixed so a test can tell it answered.
async fn echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            let mut reply = b"echo:".to_vec();
                            reply.extend_from_slice(&buf[..n]);
                            if s.write_all(&reply).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

/// A local server that speaks first, like SSH or SMTP.
async fn banner_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let _ = s.write_all(b"220 banner\r\n").await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    addr
}

struct Guest {
    start: std::time::Instant,
    device: Queues,
    iface: Interface,
    sockets: SocketSet<'static>,
    next_port: u16,
}

impl Guest {
    fn new() -> Self {
        let mut device = Queues::default();
        let config = Config::new(HardwareAddress::Ethernet(EthernetAddress([
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56,
        ])));
        let mut iface = Interface::new(config, &mut device, smoltcp::time::Instant::ZERO);
        iface.update_ip_addrs(|a| {
            a.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24))
                .unwrap();
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Addr::new(10, 0, 2, 2))
            .unwrap();
        Self {
            start: std::time::Instant::now(),
            device,
            iface,
            sockets: SocketSet::new(Vec::new()),
            next_port: 49152,
        }
    }

    fn now(&self) -> smoltcp::time::Instant {
        smoltcp::time::Instant::from_micros(self.start.elapsed().as_micros() as i64)
    }

    async fn step(&mut self, gateway: &Gateway) {
        loop {
            let frame = gateway.recv().await.unwrap();
            if frame.is_empty() {
                break;
            }
            self.device.rx.push_back(frame);
        }
        let now = self.now();
        self.iface.poll(now, &mut self.device, &mut self.sockets);
        while let Some(frame) = self.device.tx.pop_front() {
            gateway.send(&frame).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    /// Step until `done` says so, or give up after `limit`.
    async fn until(
        &mut self,
        gateway: &Gateway,
        limit: Duration,
        mut done: impl FnMut(&mut SocketSet<'static>) -> bool,
    ) -> bool {
        let deadline = std::time::Instant::now() + limit;
        while std::time::Instant::now() < deadline {
            self.step(gateway).await;
            if done(&mut self.sockets) {
                return true;
            }
        }
        false
    }

    fn connect(&mut self, to: Ipv4Addr, port: u16) -> SocketHandle {
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 32 * 1024]),
            tcp::SocketBuffer::new(vec![0; 32 * 1024]),
        );
        self.next_port += 1;
        socket
            .connect(
                self.iface.context(),
                (IpAddress::Ipv4(to), port),
                self.next_port,
            )
            .unwrap();
        self.sockets.add(socket)
    }

    /// Connect, send `data`, and return what came back before `limit`, or
    /// `None` if the connection was refused or reset.
    async fn exchange(
        &mut self,
        gateway: &Gateway,
        to: Ipv4Addr,
        port: u16,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        let handle = self.connect(to, port);
        let mut sent = false;
        let mut received = Vec::new();
        let mut reset = false;
        self.until(gateway, Duration::from_secs(5), |sockets| {
            let s = sockets.get_mut::<tcp::Socket>(handle);
            if !sent && s.may_send() {
                s.send_slice(data).unwrap();
                sent = true;
            }
            while s.can_recv() {
                s.recv(|b| {
                    received.extend_from_slice(b);
                    (b.len(), ())
                })
                .unwrap();
            }
            if s.state() == tcp::State::Closed {
                reset = received.is_empty();
                return true;
            }
            received.len() >= data.len() + 5
        })
        .await;
        self.sockets.get_mut::<tcp::Socket>(handle).abort();
        if reset {
            None
        } else {
            Some(received)
        }
    }

    /// Ask the gateway's DNS, returning the response's rcode and addresses.
    async fn lookup(&mut self, gateway: &Gateway, name: &str) -> (u8, Vec<Ipv4Addr>) {
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
        );
        self.next_port += 1;
        socket.bind(self.next_port).unwrap();
        let handle = self.sockets.add(socket);
        let query = dns::tests::query(7, name, dns::TYPE_A);
        self.sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(&query, IpEndpoint::new(IpAddress::v4(10, 0, 2, 3), 53))
            .unwrap();
        let mut answer = Vec::new();
        self.until(gateway, Duration::from_secs(5), |sockets| {
            match sockets.get_mut::<udp::Socket>(handle).recv() {
                Ok((data, _)) => {
                    answer = data.to_vec();
                    true
                }
                Err(_) => false,
            }
        })
        .await;
        self.sockets.remove(handle);
        assert!(answer.len() >= 12, "no DNS answer for {name}");
        let rcode = answer[3] & 0x0f;
        let count = usize::from(u16::from_be_bytes([answer[6], answer[7]]));
        let addresses = (0..count)
            .map(|i| {
                let at = answer.len() - (count - i) * 16 + 12;
                Ipv4Addr::new(answer[at], answer[at + 1], answer[at + 2], answer[at + 3])
            })
            .collect();
        (rcode, addresses)
    }
}

fn gateway(policy: NetworkPolicy, routes: &[(Ipv4Addr, u16, SocketAddr)]) -> Gateway {
    let mut names = HashMap::new();
    names.insert("example.com".to_string(), vec![IpAddr::V4(EXAMPLE)]);
    names.insert("evil.example".to_string(), vec![IpAddr::V4(OTHER)]);
    names.insert(
        "rebind.example".to_string(),
        vec!["10.0.0.5".parse().unwrap()],
    );
    let dials = routes
        .iter()
        .map(|(ip, port, real)| (SocketAddr::new(IpAddr::V4(*ip), *port), *real))
        .collect();
    Gateway::builder(policy)
        .resolver(Arc::new(MapResolver(names)))
        .dialer(Arc::new(MapDialer(dials)))
        .build()
        .unwrap()
}

fn e2b(allow: &[&str], deny: &[&str]) -> NetworkPolicy {
    NetworkPolicy::from_e2b(
        None,
        &allow.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        &deny.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        &[],
        Verdict::Deny,
    )
    .unwrap()
}

#[tokio::test]
async fn an_allowed_address_is_carried_both_ways() {
    let echo = echo_server().await;
    let gw = gateway(
        e2b(&["93.184.216.34"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 80, echo)],
    );
    let mut guest = Guest::new();
    let reply = guest.exchange(&gw, EXAMPLE, 80, b"hello").await;
    assert_eq!(reply.as_deref(), Some(&b"echo:hello"[..]));
    let log = gw.handle().decisions();
    assert!(log
        .iter()
        .any(|d| d.verdict == Verdict::Allow && d.reason == "allowOut address"));
}

#[tokio::test]
async fn a_refused_address_is_reset_at_the_syn_and_never_dialled() {
    // The route exists, so a dial *would* succeed: the refusal is the policy's.
    let echo = echo_server().await;
    let gw = gateway(NetworkPolicy::new(Verdict::Deny), &[(EXAMPLE, 80, echo)]);
    let mut guest = Guest::new();
    let started = std::time::Instant::now();
    assert_eq!(guest.exchange(&gw, EXAMPLE, 80, b"hello").await, None);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a RST, not a timeout: {:?}",
        started.elapsed()
    );
    let stats = gw.handle().stats();
    assert_eq!(stats.connections_refused, 1);
    assert_eq!(stats.connections_allowed, 0);
}

#[tokio::test]
async fn reserved_addresses_are_refused_even_by_a_permissive_policy() {
    // Every target has a working route, so a refusal here is the policy's and
    // not an upstream that happened to be missing.
    let echo = echo_server().await;
    let targets = [
        Ipv4Addr::new(169, 254, 169, 254),
        Ipv4Addr::new(192, 168, 1, 1),
        Ipv4Addr::new(127, 0, 0, 1),
    ];
    let routes: Vec<_> = targets.iter().map(|t| (*t, 80, echo)).collect();
    let gw = gateway(NetworkPolicy::new(Verdict::Allow), &routes);
    let mut guest = Guest::new();
    for target in targets {
        assert_eq!(
            guest.exchange(&gw, target, 80, b"x").await,
            None,
            "{target}"
        );
    }
    let log = gw.handle().decisions();
    assert_eq!(log.len(), targets.len());
    assert!(log
        .iter()
        .all(|d| d.verdict == Verdict::Deny && d.reason == "reserved address"));
}

#[tokio::test]
async fn an_upstream_refusal_reaches_the_guest_as_a_reset() {
    // Allowed, but nothing listens: the dialer refuses.
    let gw = gateway(NetworkPolicy::new(Verdict::Allow), &[]);
    let mut guest = Guest::new();
    assert_eq!(guest.exchange(&gw, EXAMPLE, 81, b"x").await, None);
    let log = gw.handle().decisions();
    assert!(log.iter().any(|d| d.reason.starts_with("upstream")));
}

#[tokio::test]
async fn dns_answers_allowed_names_and_refuses_the_rest() {
    let gw = gateway(e2b(&["example.com"], &["0.0.0.0/0"]), &[]);
    let mut guest = Guest::new();
    let (rcode, addresses) = guest.lookup(&gw, "example.com").await;
    assert_eq!(rcode, 0);
    assert_eq!(addresses, vec![EXAMPLE]);

    let (rcode, addresses) = guest.lookup(&gw, "secret-data.evil.example").await;
    assert_eq!(
        rcode, 5,
        "REFUSED: the query itself would be the exfiltration"
    );
    assert!(addresses.is_empty());
}

#[tokio::test]
async fn a_dns_answer_for_an_allowed_name_opens_its_address() {
    let echo = echo_server().await;
    let gw = gateway(
        e2b(&["example.com"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 22, echo)],
    );
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    // Port 22 and not TLS or HTTP: allowed on the strength of the answer.
    let reply = guest.exchange(&gw, EXAMPLE, 22, b"SSH-2.0-x\r\n").await;
    assert_eq!(reply.as_deref(), Some(&b"echo:SSH-2.0-x\r\n"[..]));
}

#[tokio::test]
async fn a_protocol_where_the_server_speaks_first_still_works() {
    let banner = banner_server().await;
    let gw = gateway(
        e2b(&["example.com"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 25, banner)],
    );
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    let handle = guest.connect(EXAMPLE, 25);
    let mut got = Vec::new();
    guest
        .until(&gw, Duration::from_secs(5), |s| {
            let s = s.get_mut::<tcp::Socket>(handle);
            while s.can_recv() {
                s.recv(|b| {
                    got.extend_from_slice(b);
                    (b.len(), ())
                })
                .unwrap();
            }
            got.ends_with(b"\r\n")
        })
        .await;
    assert_eq!(got, b"220 banner\r\n");
}

#[tokio::test]
async fn an_http_host_can_open_an_address_the_policy_would_refuse() {
    let echo = echo_server().await;
    let gw = gateway(
        e2b(&["example.com"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 80, echo)],
    );
    let mut guest = Guest::new();
    let request = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
    let reply = guest.exchange(&gw, EXAMPLE, 80, request).await.unwrap();
    assert!(reply.starts_with(b"echo:GET / HTTP/1.1"));
}

#[tokio::test]
async fn a_refused_host_on_the_same_address_is_reset() {
    let echo = echo_server().await;
    let gw = gateway(
        e2b(&["example.com"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 80, echo)],
    );
    let mut guest = Guest::new();
    let request = b"GET / HTTP/1.1\r\nHost: evil.example\r\n\r\n";
    assert_eq!(guest.exchange(&gw, EXAMPLE, 80, request).await, None);
}

/// The guest chooses both the address and the name. An allowed name aimed at
/// an address it does not resolve to must not be an allowed connection.
#[tokio::test]
async fn an_allowed_name_aimed_at_another_address_is_reset() {
    let echo = echo_server().await;
    let gw = gateway(e2b(&["example.com"], &["0.0.0.0/0"]), &[(OTHER, 80, echo)]);
    let mut guest = Guest::new();
    let request = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
    assert_eq!(guest.exchange(&gw, OTHER, 80, request).await, None);
    let log = gw.handle().decisions();
    assert!(log
        .iter()
        .any(|d| d.reason.contains("does not resolve to the address")));
}

/// Two names on one address: the answer for the allowed one does not carry
/// a TLS connection naming the other.
#[tokio::test]
async fn a_refused_sni_on_an_address_opened_by_dns_is_reset() {
    let echo = echo_server().await;
    let gw = gateway(
        e2b(&["example.com"], &["0.0.0.0/0"]),
        &[(EXAMPLE, 443, echo)],
    );
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    let hello = sniff::tests::client_hello("evil.example");
    assert_eq!(guest.exchange(&gw, EXAMPLE, 443, &hello).await, None);

    let hello = sniff::tests::client_hello("example.com");
    let reply = guest.exchange(&gw, EXAMPLE, 443, &hello).await.unwrap();
    assert!(reply.starts_with(b"echo:"));
}

#[tokio::test]
async fn dns_rebinding_to_a_private_address_is_refused() {
    let echo = echo_server().await;
    let target: Ipv4Addr = "10.0.0.5".parse().unwrap();
    let gw = gateway(e2b(&["rebind.example"], &[]), &[(target, 80, echo)]);
    let mut guest = Guest::new();
    let (rcode, addresses) = guest.lookup(&gw, "rebind.example").await;
    assert_eq!((rcode, addresses), (0, vec![target]));
    assert_eq!(guest.exchange(&gw, target, 80, b"x").await, None);
}

#[tokio::test]
async fn changing_the_policy_applies_to_the_next_connection() {
    let echo = echo_server().await;
    let gw = gateway(NetworkPolicy::new(Verdict::Deny), &[(EXAMPLE, 80, echo)]);
    let handle = gw.handle();
    let mut guest = Guest::new();
    assert_eq!(guest.exchange(&gw, EXAMPLE, 80, b"a").await, None);
    handle.set_policy(e2b(&["93.184.216.34/32"], &[]));
    assert_eq!(
        guest.exchange(&gw, EXAMPLE, 80, b"b").await.as_deref(),
        Some(&b"echo:b"[..])
    );
}

#[tokio::test]
async fn a_transform_rule_without_interception_is_refused_rather_than_sent_bare() {
    let echo = echo_server().await;
    let mut headers = Headers::new();
    headers.insert("Authorization".into(), "Bearer secret".into());
    let policy = NetworkPolicy::from_e2b(
        None,
        &["example.com".into()],
        &["0.0.0.0/0".into()],
        &[("example.com".into(), headers)],
        Verdict::Deny,
    )
    .unwrap();
    let gw = gateway(policy, &[(EXAMPLE, 443, echo)]);
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    let hello = sniff::tests::client_hello("example.com");
    assert_eq!(
        guest.exchange(&gw, EXAMPLE, 443, &hello).await,
        None,
        "a request that was meant to carry a credential does not go without it"
    );
}

/// An HTTP server that answers with the request head it received.
async fn http_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buf = vec![0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let body = String::from_utf8_lossy(&request).to_lowercase();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(reply.as_bytes()).await;
            });
        }
    });
    addr
}

fn with_rule(allow: &[&str], deny: &[&str], host: &str, value: &str) -> NetworkPolicy {
    let mut headers = Headers::new();
    headers.insert("Authorization".into(), value.into());
    NetworkPolicy::from_e2b(
        None,
        &allow.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        &deny.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        &[(host.to_string(), headers)],
        Verdict::Deny,
    )
    .unwrap()
}

#[tokio::test]
async fn a_rule_injects_into_plain_http_to_the_named_host() {
    let upstream = http_echo_server().await;
    let policy = with_rule(
        &["example.com"],
        &["0.0.0.0/0"],
        "example.com",
        "Bearer real",
    );
    let gw = gateway(policy, &[(EXAMPLE, 80, upstream)]);
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    let request =
        b"GET /x HTTP/1.1\r\nHost: example.com\r\nAuthorization: Bearer placeholder\r\n\r\n";
    let reply = guest.exchange(&gw, EXAMPLE, 80, request).await.unwrap();
    let reply = String::from_utf8_lossy(&reply);
    assert!(reply.contains("authorization: bearer real"), "{reply}");
    assert!(
        !reply.contains("placeholder"),
        "the guest's value was replaced: {reply}"
    );
    assert_eq!(gw.handle().stats().intercepted, 1);
}

/// Plain HTTP has no certificate to prove where it went, so a guest that
/// names the rule's host while connecting somewhere else gets nothing.
#[tokio::test]
async fn a_rule_does_not_follow_a_spoofed_host_to_another_address() {
    let upstream = http_echo_server().await;
    // Everything but 8.8.8.8 allowed: the connection itself is permitted.
    let policy = with_rule(&[], &["8.8.8.8"], "example.com", "Bearer real");
    let gw = gateway(policy, &[(OTHER, 80, upstream)]);
    let mut guest = Guest::new();
    let request = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
    assert_eq!(guest.exchange(&gw, OTHER, 80, request).await, None);
    assert!(gw
        .handle()
        .decisions()
        .iter()
        .any(|d| d.reason.contains("not where it resolves")));
}

#[tokio::test]
async fn an_egress_proxy_carries_allowed_traffic_and_is_told_the_name() {
    let echo = echo_server().await;
    let (proxy, seen) = socks::tests::socks_server(echo, None).await;
    // No direct route at all: only the proxy can reach anything.
    let gw = gateway(e2b(&["example.com"], &["0.0.0.0/0"]), &[]);
    gw.handle().set_egress_proxy(Some(
        socks::Socks5Proxy::new(&proxy.to_string(), None, None).unwrap(),
    ));
    let mut guest = Guest::new();
    guest.lookup(&gw, "example.com").await;
    let reply = guest.exchange(&gw, EXAMPLE, 22, b"hi").await;
    assert_eq!(reply.as_deref(), Some(&b"echo:hi"[..]));
    assert_eq!(
        seen.lock().as_slice(),
        ["example.com:22"],
        "remote DNS, by name"
    );

    // The policy still applies first: a refused address never reaches it.
    assert_eq!(guest.exchange(&gw, OTHER, 22, b"x").await, None);
    assert_eq!(seen.lock().len(), 1);
}

#[tokio::test]
async fn an_egress_proxy_that_refuses_fails_closed() {
    let echo = echo_server().await;
    let (proxy, seen) = socks::tests::socks_server(echo, Some(("user", "right"))).await;
    // A direct route exists; failing closed means it is not used.
    let gw = gateway(NetworkPolicy::new(Verdict::Allow), &[(EXAMPLE, 22, echo)]);
    gw.handle().set_egress_proxy(Some(
        socks::Socks5Proxy::new(&proxy.to_string(), Some("user"), Some("wrong")).unwrap(),
    ));
    let mut guest = Guest::new();
    assert_eq!(guest.exchange(&gw, EXAMPLE, 22, b"x").await, None);
    assert!(seen.lock().is_empty());

    gw.handle().set_egress_proxy(Some(
        socks::Socks5Proxy::new(&proxy.to_string(), Some("user"), Some("right")).unwrap(),
    ));
    assert_eq!(
        guest.exchange(&gw, EXAMPLE, 22, b"y").await.as_deref(),
        Some(&b"echo:y"[..])
    );
    assert_eq!(
        seen.lock().as_slice(),
        ["93.184.216.34:22"],
        "no name known: by address"
    );
}

// ── Interception, end to end over TLS ───────────────────────────────────────

/// An HTTPS upstream that answers every request with the `authorization` and
/// `x-guest` headers it received.
async fn header_echo_upstream(
    name: &str,
) -> (SocketAddr, rustls::pki_types::CertificateDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![name.to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let der = cert.der().clone();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![der.clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(s).await else {
                    return;
                };
                let mut buf = vec![0u8; 8192];
                let mut request = Vec::new();
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request).to_lowercase();
                let header = |h: &str| {
                    text.lines()
                        .find_map(|l| l.strip_prefix(&format!("{h}: ")))
                        .unwrap_or("-")
                        .trim()
                        .to_string()
                };
                let body = format!(
                    "auth={} guest={}",
                    header("authorization"),
                    header("x-guest")
                );
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(reply.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    (addr, der)
}

/// The guest's TLS client, over a `GuestStream`-shaped pipe into `intercept`.
#[tokio::test]
async fn interception_injects_the_header_and_the_guest_never_holds_it() {
    let (upstream_addr, upstream_root) = header_echo_upstream("api.example.com").await;
    let authority = Arc::new(mitm::Authority::generate().unwrap());

    let (guest_side, gateway_side) = tokio::io::duplex(64 * 1024);
    let upstream = tokio::net::TcpStream::connect(upstream_addr).await.unwrap();
    let mut headers = Headers::new();
    headers.insert("Authorization".into(), "Bearer real-secret".into());
    let server = authority.server_config().unwrap();
    let client = mitm::upstream_config(&[upstream_root]).unwrap();
    tokio::spawn(async move {
        mitm::intercept(
            gateway_side,
            upstream,
            "api.example.com",
            &headers,
            server,
            client,
        )
        .await
    });

    // The guest trusts the sandbox CA, and nothing else.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(authority.ca_der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            rustls::pki_types::ServerName::try_from("api.example.com").unwrap(),
            guest_side,
        )
        .await
        .expect("the guest accepts the gateway's leaf");
    tls.write_all(
        b"GET / HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: placeholder\r\nX-Guest: kept\r\n\r\n",
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    let _ = tls.read_to_end(&mut response).await;
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.ends_with("auth=bearer real-secret guest=kept"),
        "the placeholder was replaced and other headers kept: {response}"
    );
}

/// An upstream that cannot prove it is the name gets no request, and so no
/// secret: a guest cannot redirect an intercepted name to its own server.
#[tokio::test]
async fn interception_refuses_an_upstream_that_is_not_the_name() {
    let (upstream_addr, _not_trusted) = header_echo_upstream("api.example.com").await;
    let authority = Arc::new(mitm::Authority::generate().unwrap());
    let (guest_side, gateway_side) = tokio::io::duplex(64 * 1024);
    let upstream = tokio::net::TcpStream::connect(upstream_addr).await.unwrap();
    let mut headers = Headers::new();
    headers.insert("Authorization".into(), "Bearer real-secret".into());
    let server = authority.server_config().unwrap();
    let client = mitm::upstream_config(&[]).unwrap();
    let relay = tokio::spawn(async move {
        mitm::intercept(
            gateway_side,
            upstream,
            "api.example.com",
            &headers,
            server,
            client,
        )
        .await
    });

    let mut roots = rustls::RootCertStore::empty();
    roots.add(authority.ca_der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    // The guest's handshake completes (it trusts the sandbox CA)...
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            rustls::pki_types::ServerName::try_from("api.example.com").unwrap(),
            guest_side,
        )
        .await;
    drop(tls);
    // ...and the relay fails on the upstream side, before any request.
    let result = tokio::time::timeout(Duration::from_secs(5), relay)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_err(), "the upstream was not verified");
}

#[test]
fn the_kernel_argument_configures_the_guest_for_this_gateway() {
    assert_eq!(
        GatewayConfig::default().kernel_ip_arg(),
        "ip=10.0.2.15::10.0.2.2:255.255.255.0::eth0:off:10.0.2.3"
    );
}
