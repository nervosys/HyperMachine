//! Source-bound private TCP node dialing. Guest gateway integration must supply
//! a lease derived from its actual live VM; guest headers are never identity.
use crate::{
    model::SandboxRecord,
    mtls::Mtls,
    native_tcp::NativeTunnel,
    private_networks::{
        NetworkTag, PRIVATE_ROUTE_HEADER, PRIVATE_SOURCE_NODE_HEADER, PrivateRouteClaim,
    },
    store::ClusterStore,
};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Holds source activity/lifecycle ownership for a connection. Implementations
/// must refuse a removed/replaced VM or pending local registration. Validation
/// runs before/after setup and every stream poll; it must be fast and nonblocking.
// `WouldBlock` means validation is temporarily unavailable, never permission
// to carry bytes. Setup retries within its deadline; streams apply backpressure
// for at most one second before closing without forwarding unchecked bytes.
pub trait PrivateSourceLease: Send + Sync + Unpin {
    fn validate(&self, expected_source: &SandboxRecord) -> io::Result<()>;
}
/// Fixed trusted source incarnation, never selected by a guest request. Node
/// identity uses the current shared-credential mTLS cluster trust model.
pub struct PrivateNodeConnector {
    store: Arc<dyn ClusterStore>,
    source: SandboxRecord,
    client: reqwest::Client,
    token: reqwest::header::HeaderValue,
}
pub(crate) async fn validate_source(
    lease: &dyn PrivateSourceLease,
    source: &SandboxRecord,
) -> io::Result<()> {
    loop {
        match lease.validate(source) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                tokio::time::sleep(Duration::from_millis(1)).await
            }
            result => return result,
        }
    }
}
fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "private source or route refused",
    )
}
pub(crate) fn node_origin(value: &str) -> io::Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid private node origin"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private node requires an HTTPS origin",
        ));
    }
    Ok(url)
}
impl PrivateNodeConnector {
    pub(crate) fn source_record(&self) -> &SandboxRecord {
        &self.source
    }
    pub(crate) fn cluster_store(&self) -> Arc<dyn ClusterStore> {
        self.store.clone()
    }

    /// `source` comes from the caller's own registered VM. A matching lease is
    /// still mandatory on every open; a shared record is not local publication.
    pub fn new(
        store: Arc<dyn ClusterStore>,
        tls: &Mtls,
        token: &str,
        source: SandboxRecord,
    ) -> io::Result<Self> {
        crate::private_networks::NetworkMembershipState::new(&source, None)
            .map_err(|_| refused())?;
        if token.trim().is_empty() || token.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid private cluster token",
            ));
        }
        let mut token = reqwest::header::HeaderValue::from_str(token).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid private cluster token")
        })?;
        token.set_sensitive(true);
        let client = tls
            .tcp_http_client()
            .map_err(|_| io::Error::other("private TLS client configuration failed"))?;
        Ok(Self {
            store,
            source,
            client,
            token,
        })
    }
    async fn target(
        &self,
        destination: &str,
        network: NetworkTag,
        port: u16,
        lease: &dyn PrivateSourceLease,
    ) -> io::Result<(PrivateRouteClaim, String, reqwest::Url)> {
        validate_source(lease, &self.source).await?;
        let snapshot = self
            .store
            .private_route_snapshot(&self.source.sandbox_id, destination)
            .await
            .map_err(|_| io::Error::other("private route lookup unavailable"))?
            .ok_or_else(refused)?;
        let source = &snapshot.source_record;
        if source.sandbox_id != self.source.sandbox_id
            || source.owner_id != self.source.owner_id
            || source.node_id != self.source.node_id
            || source.started_at_ms != self.source.started_at_ms
        {
            return Err(refused());
        }
        let claim = snapshot
            .claim(network, port, crate::model::now_ms(), false, false)
            .map_err(|_| refused())?;
        let (source_node, destination_node) = tokio::join!(
            self.store.node(&source.node_id),
            self.store.node(&snapshot.destination_record.node_id)
        );
        let source_node = source_node
            .map_err(|_| io::Error::other("private source node lookup unavailable"))?
            .ok_or_else(refused)?;
        let destination_node = destination_node
            .map_err(|_| io::Error::other("private destination node lookup unavailable"))?
            .ok_or_else(refused)?;
        if source_node.id != source.node_id
            || destination_node.id != snapshot.destination_record.node_id
        {
            return Err(refused());
        }
        let mut url = node_origin(&destination_node.api)?;
        url.set_path(&format!(
            "/sandboxes/{destination}/private-ports/{port}/tcp"
        ));
        validate_source(lease, &self.source).await?;
        Ok((claim, destination_node.id, url))
    }
    /// Dial the current authorized node with a 15-second total setup deadline.
    /// A changed claim, placement or origin during upgrade is refused. The lease
    /// stays with the returned stream, including read/write-half operations.
    pub async fn open_tcp(
        &self,
        destination: &str,
        network: NetworkTag,
        port: u16,
        lease: Box<dyn PrivateSourceLease>,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        self.open_inner(destination, network, port, lease, None, false)
            .await
    }
    /// Dial an address binding without refreshing it to a new membership
    /// generation. A stale binding is refused before any node HTTP request.
    /// The claim is context, not a credential; normal local/source/destination
    /// authentication and fresh route checks remain mandatory.
    pub async fn open_bound_tcp(
        &self,
        expected: PrivateRouteClaim,
        lease: Box<dyn PrivateSourceLease>,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        self.open_bound(expected, lease, false).await
    }
    /// Open a generation-bound framed IPv4 UDP tunnel. The caller must encode
    /// and validate bounded datagram frames; this layer preserves stream bytes.
    pub async fn open_bound_udp(
        &self,
        expected: PrivateRouteClaim,
        lease: Box<dyn PrivateSourceLease>,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        self.open_bound(expected, lease, true).await
    }
    async fn open_bound(
        &self,
        expected: PrivateRouteClaim,
        lease: Box<dyn PrivateSourceLease>,
        udp: bool,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        expected.validate().map_err(|_| refused())?;
        if expected.source_id != self.source.sandbox_id {
            return Err(refused());
        }
        let destination = expected.destination_id.clone();
        self.open_inner(
            &destination,
            expected.network.clone(),
            expected.port,
            lease,
            Some(expected),
            udp,
        )
        .await
    }
    async fn open_inner(
        &self,
        destination: &str,
        network: NetworkTag,
        port: u16,
        lease: Box<dyn PrivateSourceLease>,
        expected: Option<PrivateRouteClaim>,
        udp: bool,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        let attempt = async {
            let (claim, node, url) = self
                .target(destination, network.clone(), port, lease.as_ref())
                .await?;
            if expected.as_ref().is_some_and(|bound| bound != &claim) {
                return Err(refused());
            }
            let context = serde_json::to_string(&claim)
                .map_err(|_| io::Error::other("private route context encoding failed"))?;
            if context.len() > 2048 {
                return Err(refused());
            }
            let protocol = if udp {
                hv2_api::udp_tunnel::PROTOCOL
            } else {
                hv2_api::tcp_tunnel::PROTOCOL
            };
            let mut request_url = url.clone();
            if udp {
                request_url.set_path(&format!(
                    "/sandboxes/{destination}/private-ports/{port}/udp"
                ));
            }
            let response = self
                .client
                .get(request_url)
                .version(reqwest::Version::HTTP_11)
                .header("connection", "upgrade")
                .header("upgrade", protocol)
                .header(crate::control::CLUSTER_TOKEN_HEADER, self.token.clone())
                .header(PRIVATE_ROUTE_HEADER, context)
                .header(PRIVATE_SOURCE_NODE_HEADER, &self.source.node_id)
                .send()
                .await
                .map_err(|_| io::Error::other("authenticated private node connection failed"))?;
            if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
                || response.headers().get_all("upgrade").iter().count() != 1
                || response
                    .headers()
                    .get("upgrade")
                    .and_then(|v| v.to_str().ok())
                    != Some(protocol)
                || !response
                    .headers()
                    .get_all("connection")
                    .iter()
                    .filter_map(|v| v.to_str().ok())
                    .flat_map(|v| v.split(','))
                    .any(|v| v.trim().eq_ignore_ascii_case("upgrade"))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "node refused exact private upgrade",
                ));
            }
            let stream = response
                .upgrade()
                .await
                .map_err(|_| io::Error::other("private node upgrade failed"))?;
            if self
                .target(destination, network, port, lease.as_ref())
                .await?
                != (claim, node, url)
            {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "private route changed during setup",
                ));
            }
            Ok(Box::new(LeasedStream {
                stream: Some(Box::new(stream)),
                lease: Some(lease),
                source: self.source.clone(),
                retry: None,
                contention_since: None,
                waiters: [None, None],
            }) as Box<dyn NativeTunnel>)
        };
        tokio::time::timeout(Duration::from_secs(15), attempt)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "private tunnel setup timed out"))?
    }
}
struct LeasedStream {
    stream: Option<Box<dyn NativeTunnel>>,
    lease: Option<Box<dyn PrivateSourceLease>>,
    source: SandboxRecord,
    retry: Option<Pin<Box<tokio::time::Sleep>>>,
    contention_since: Option<tokio::time::Instant>,
    waiters: [Option<std::task::Waker>; 2],
}
impl LeasedStream {
    fn poll_source(&mut self, cx: &mut Context<'_>, slot: usize) -> Poll<io::Result<()>> {
        use std::future::Future;
        if let Some(retry) = &mut self.retry {
            if retry.as_mut().poll(cx).is_pending() {
                self.waiters[slot] = Some(cx.waker().clone());
                return Poll::Pending;
            }
            self.retry = None;
            for waiter in &mut self.waiters {
                if let Some(waiter) = waiter.take() {
                    waiter.wake();
                }
            }
        }
        let result = self
            .lease
            .as_ref()
            .map(|lease| lease.validate(&self.source))
            .unwrap_or_else(|| Err(refused()));
        if matches!(&result,Err(error) if error.kind()==io::ErrorKind::WouldBlock) {
            let started = self
                .contention_since
                .get_or_insert_with(tokio::time::Instant::now);
            if started.elapsed() < Duration::from_secs(1) {
                let mut retry = Box::pin(tokio::time::sleep(Duration::from_millis(1)));
                let _ = retry.as_mut().poll(cx);
                self.retry = Some(retry);
                self.waiters[slot] = Some(cx.waker().clone());
                return Poll::Pending;
            }
            self.stream.take();
            self.lease.take();
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        self.contention_since = None;
        if result.is_err() {
            self.stream.take();
            self.lease.take();
        }
        Poll::Ready(result)
    }
}
impl AsyncRead for LeasedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.poll_source(cx, 0) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }
        match self.stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_read(cx, buf),
            None => Poll::Ready(Err(refused())),
        }
    }
}
impl AsyncWrite for LeasedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.poll_source(cx, 1) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }
        match self.stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_write(cx, buf),
            None => Poll::Ready(Err(refused())),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.poll_source(cx, 1) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }
        match self.stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_flush(cx),
            None => Poll::Ready(Err(refused())),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Always permit local shutdown, including after source revocation.
        match self.stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_shutdown(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Lease {
        active: Arc<AtomicBool>,
        dropped: Arc<AtomicUsize>,
    }
    impl PrivateSourceLease for Lease {
        fn validate(&self, _: &SandboxRecord) -> io::Result<()> {
            if self.active.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(refused())
            }
        }
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn target_requires_current_source_incarnation_lease_and_safe_destination_origin() {
        use crate::{
            ownership::OwnerId,
            private_networks::{MembershipChange, NetworkMembershipState},
            store::MemoryStore,
        };
        let store = Arc::new(MemoryStore::new());
        let mut source = crate::store::tests::sandbox("source", "source-node");
        source.owner_id = Some(OwnerId::parse("owner").unwrap());
        let mut destination = crate::store::tests::sandbox("destination", "destination-node");
        destination.owner_id = source.owner_id.clone();
        for r in [&source, &destination] {
            store.put_sandbox(r).await.unwrap();
        }
        for id in ["source-node", "destination-node"] {
            let mut node = crate::store::tests::node(id, 1, 4);
            node.api = "https://127.0.0.1:1".into();
            store
                .put_node(&node, Duration::from_secs(60))
                .await
                .unwrap();
        }
        let tag = || NetworkTag::parse("team").unwrap();
        let sm = NetworkMembershipState::new(&source, Some(vec![tag()])).unwrap();
        let dm = NetworkMembershipState::new(&destination, Some(vec![tag()])).unwrap();
        for m in [&sm, &dm] {
            assert_eq!(
                store.compare_private_membership(None, m).await.unwrap(),
                MembershipChange::Applied
            );
        }
        // Target lookup does not perform HTTP; the test client is never dialed.
        let connector = PrivateNodeConnector {
            store: store.clone(),
            source: source.clone(),
            client: reqwest::Client::new(),
            token: reqwest::header::HeaderValue::from_static("owned-test"),
        };
        let active = Arc::new(AtomicBool::new(true));
        let dropped = Arc::new(AtomicUsize::new(0));
        let lease = Lease {
            active: active.clone(),
            dropped: dropped.clone(),
        };
        let (claim, node, url) = connector
            .target("destination", tag(), 18082, &lease)
            .await
            .unwrap();
        claim.validate().unwrap();
        assert_eq!(node, "destination-node");
        assert_eq!(url.path(), "/sandboxes/destination/private-ports/18082/tcp");
        active.store(false, Ordering::SeqCst);
        assert_eq!(
            connector
                .target("destination", tag(), 18082, &lease)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        active.store(true, Ordering::SeqCst);
        let mut unsafe_node = crate::store::tests::node("destination-node", 1, 4);
        unsafe_node.api = "http://127.0.0.1:1".into();
        store
            .put_node(&unsafe_node, Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            connector
                .target("destination", tag(), 18082, &lease)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        unsafe_node.api = "https://127.0.0.1:1".into();
        store
            .put_node(&unsafe_node, Duration::from_secs(60))
            .await
            .unwrap();
        source.started_at_ms += 1;
        store.put_sandbox(&source).await.unwrap();
        let replacement = NetworkMembershipState::new(&source, Some(vec![tag()])).unwrap();
        assert_eq!(
            store
                .compare_private_membership(Some(sm.revision()), &replacement)
                .await
                .unwrap(),
            MembershipChange::Applied
        );
        assert_eq!(
            connector
                .target("destination", tag(), 18082, &lease)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        fn assert_send<T: Send>(_: T) {}
        assert_send(connector.open_tcp(
            "destination",
            tag(),
            18082,
            Box::new(Lease { active, dropped }),
        ));
    }

    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    #[derive(Clone, Copy)]
    enum Change {
        None,
        SourceRejoin,
        DestinationRemove,
        DestinationMove,
        LeaseRevoke,
        StallUpgrade,
    }
    async fn transport_case(
        change: Change,
        certificate_name: &str,
        response: &'static [u8],
        anonymous: bool,
        wrong_token: bool,
    ) -> io::Result<()> {
        transport_case_mode(
            change,
            certificate_name,
            response,
            anonymous,
            wrong_token,
            false,
        )
        .await
    }
    async fn transport_case_mode(
        change: Change,
        certificate_name: &str,
        response: &'static [u8],
        anonymous: bool,
        wrong_token: bool,
        bound: bool,
    ) -> io::Result<()> {
        transport_case_protocol(change, certificate_name, response, anonymous, wrong_token, bound, false, false).await
    }
    async fn transport_case_protocol(
        change: Change,
        certificate_name: &str,
        response: &'static [u8],
        anonymous: bool,
        wrong_token: bool,
        bound: bool,
        udp: bool,
        router: bool,
    ) -> io::Result<()> {
        use crate::{
            mtls::DEFAULT_NODE_NAME,
            ownership::OwnerId,
            private_networks::{MembershipChange, NetworkMembershipState},
            store::MemoryStore,
        };
        let identity = crate::native_node::tests::identity(certificate_name);
        let store = Arc::new(MemoryStore::new());
        let mut source = crate::store::tests::sandbox("source", "source-node");
        source.owner_id = Some(OwnerId::parse("owner").unwrap());
        let mut destination = crate::store::tests::sandbox("destination", "destination-node");
        destination.owner_id = source.owner_id.clone();
        for r in [&source, &destination] {
            store.put_sandbox(r).await.unwrap();
        }
        let tag = || NetworkTag::parse("team").unwrap();
        let sm = NetworkMembershipState::new(&source, Some(vec![tag()])).unwrap();
        let dm = NetworkMembershipState::new(&destination, Some(vec![tag()])).unwrap();
        for m in [&sm, &dm] {
            assert_eq!(
                store.compare_private_membership(None, m).await.unwrap(),
                MembershipChange::Applied
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        for id in ["source-node", "destination-node"] {
            let mut node = crate::store::tests::node(id, 1, 4);
            node.api = format!("https://{address}");
            store
                .put_node(&node, Duration::from_secs(60))
                .await
                .unwrap();
        }
        let active = Arc::new(AtomicBool::new(true));
        let dropped = Arc::new(AtomicUsize::new(0));
        let acceptor =
            tokio_rustls::TlsAcceptor::from(Arc::new(identity.node.server_config().unwrap()));
        let bound_claim = store
            .private_route_snapshot("source", "destination")
            .await
            .unwrap()
            .unwrap()
            .claim(tag(), 18082, crate::model::now_ms(), false, false)
            .unwrap();
        let server_store = store.clone();
        let server_active = active.clone();
        let mut server = Server(tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                return;
            };
            assert_eq!(
                tls.get_ref().1.alpn_protocol(),
                Some(b"http/1.1".as_slice())
            );
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let Ok(byte) = tls.read_u8().await else {
                    return;
                };
                bytes.push(byte);
                assert!(bytes.len() < 8192);
            }
            let text = std::str::from_utf8(&bytes).unwrap();
            let mut route = None;
            let mut credential = None;
            let mut source_node = None;
            for line in text.split("\r\n") {
                if let Some((name, value)) = line.split_once(':') {
                    match name.to_ascii_lowercase().as_str() {
                        "x-hv2-private-route" => {
                            route = Some(
                                serde_json::from_str::<PrivateRouteClaim>(value.trim()).unwrap(),
                            )
                        }
                        "x-hv2-private-source-node" => source_node = Some(value.trim().to_owned()),
                        "x-hv2-cluster-token" => credential = Some(value.trim().to_owned()),
                        _ => {}
                    }
                }
            }
            let transport = if udp { "udp" } else { "tcp" };
            assert!(text.starts_with(&format!("GET /sandboxes/destination/private-ports/18082/{transport} HTTP/1.1\r\n")));
            assert!(text.to_ascii_lowercase().contains(if udp { "upgrade: hv2-udp/1" } else { "upgrade: hv2-tcp/1" }));
            assert!(!text.to_ascii_lowercase().contains("x-api-key:"));
            assert_eq!(source_node.as_deref(), Some("source-node"));
            let route = route.unwrap();
            route.validate().unwrap();
            assert_eq!(route.source_generation, sm.revision());
            assert_eq!(route.destination_generation, dm.revision());
            if credential.as_deref() != Some("owned-cluster-token") {
                tls.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                return;
            }
            match change {
                Change::StallUpgrade => {
                    // No upgrade response: the client's total setup deadline
                    // must close TLS and release source activity ownership.
                    assert!(tls.read_u8().await.is_err());
                    return;
                }
                Change::None => {}
                Change::SourceRejoin => {
                    let next = NetworkMembershipState::new(&source, Some(vec![tag()])).unwrap();
                    assert_eq!(
                        server_store
                            .compare_private_membership(Some(sm.revision()), &next)
                            .await
                            .unwrap(),
                        MembershipChange::Applied
                    );
                }
                Change::DestinationRemove => {
                    let next = NetworkMembershipState::new(&destination, None).unwrap();
                    assert_eq!(
                        server_store
                            .compare_private_membership(Some(dm.revision()), &next)
                            .await
                            .unwrap(),
                        MembershipChange::Applied
                    );
                }
                Change::DestinationMove => {
                    let mut node = server_store
                        .node("destination-node")
                        .await
                        .unwrap()
                        .unwrap();
                    node.id = "moved-node".into();
                    server_store
                        .put_node(&node, Duration::from_secs(60))
                        .await
                        .unwrap();
                    destination.node_id = node.id;
                    server_store.put_sandbox(&destination).await.unwrap();
                    let next =
                        NetworkMembershipState::new(&destination, Some(vec![tag()])).unwrap();
                    assert_eq!(
                        server_store
                            .compare_private_membership(Some(dm.revision()), &next)
                            .await
                            .unwrap(),
                        MembershipChange::Applied
                    );
                }
                Change::LeaseRevoke => server_active.store(false, Ordering::SeqCst),
            }
            tls.write_all(response).await.unwrap();
            if response.starts_with(b"HTTP/1.1 101") {
                let mut data = [0; 8192];
                while let Ok(n) = tls.read(&mut data).await {
                    if n == 0 {
                        break;
                    }
                    if tls.write_all(&data[..n]).await.is_err() {
                        break;
                    }
                }
            }
            let _ = tls.shutdown().await;
        }));
        let source = store.sandbox("source").await.unwrap().unwrap();
        let mut connector = PrivateNodeConnector::new(
            store,
            &identity.gateway,
            if wrong_token {
                "wrong-cluster-token"
            } else {
                "owned-cluster-token"
            },
            source,
        )
        .unwrap();
        if anonymous {
            // Negative fixture only: present no client certificate, while
            // allowing server verification so rejection is at the TLS server.
            connector.client = reqwest::Client::builder()
                .http1_only()
                .no_proxy()
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap();
        }
        let router_active = active.clone();
        let lease = Box::new(Lease {
            active,
            dropped: dropped.clone(),
        });
        let result = if router {
            use crate::private_router::{PrivateSourceRouter, PrivateSourceLeaseFactory};
            struct Factory { active: Arc<AtomicBool>, dropped: Arc<AtomicUsize> }
            impl PrivateSourceLeaseFactory for Factory {
                fn acquire(&self) -> io::Result<Box<dyn PrivateSourceLease>> {
                    Ok(Box::new(Lease { active: self.active.clone(), dropped: self.dropped.clone() }))
                }
            }
            drop(lease);
            let router = PrivateSourceRouter::new(Arc::new(connector), Arc::new(Factory { active: router_active, dropped: dropped.clone() }));
            let address = router.resolve("destination.team.hv2.internal").await.unwrap()[0];
            router.dial_udp(std::net::SocketAddr::new(address, 18082)).await
        } else if udp {
            assert!(bound);
            connector.open_bound_udp(bound_claim, lease).await
        } else if bound {
            connector.open_bound_tcp(bound_claim, lease).await
        } else {
            connector.open_tcp("destination", tag(), 18082, lease).await
        };
        let output = match result {
            Ok(mut stream) => {
                let mut early = [0; 7];
                stream.read_exact(&mut early).await.unwrap();
                assert_eq!(&early[..], if udp { &b"\0\x05ready"[..] } else { &b"ready\0\xff"[..] });
                let mut payload: Vec<u8> = (0..8192).map(|i| (i % 256) as u8).collect();
                if udp { payload[..2].copy_from_slice(&8190u16.to_be_bytes()); }
                stream.write_all(&payload).await.unwrap();
                let mut echoed = vec![0; payload.len()];
                stream.read_exact(&mut echoed).await.unwrap();
                assert_eq!(echoed, payload);
                stream.shutdown().await.unwrap();
                drop(stream);
                Ok(())
            }
            Err(error) => Err(error),
        };
        assert_eq!(dropped.load(Ordering::SeqCst), if router { 3 } else { 1 });
        tokio::time::timeout(Duration::from_secs(5), &mut server.0)
            .await
            .unwrap()
            .unwrap();
        let _ = DEFAULT_NODE_NAME;
        output
    }
    #[tokio::test]
    async fn private_udp_router_commits_dns_binding_and_dispatches_framed_transport() {
        transport_case_protocol(Change::None, crate::mtls::DEFAULT_NODE_NAME, PRIVATE_UDP_SUCCESS, false, false, true, true, true).await.unwrap();
    }
    const PRIVATE_UDP_SUCCESS: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: hv2-udp/1\r\n\r\n\0\x05ready";
    #[tokio::test]
    async fn bound_private_udp_preserves_framed_early_and_binary_bytes() {
        transport_case_protocol(Change::None, crate::mtls::DEFAULT_NODE_NAME, PRIVATE_UDP_SUCCESS, false, false, true, true, false).await.unwrap();
    }
    #[tokio::test]
    async fn bound_private_udp_refuses_setup_generation_placement_and_lease_races() {
        for change in [Change::SourceRejoin, Change::DestinationRemove, Change::DestinationMove, Change::LeaseRevoke] {
            assert!(transport_case_protocol(change, crate::mtls::DEFAULT_NODE_NAME, PRIVATE_UDP_SUCCESS, false, false, true, true, false).await.is_err());
        }
    }
    #[tokio::test]
    async fn bound_private_udp_refuses_tcp_upgrade_bad_token_and_anonymous_tls() {
        for (response, anonymous, wrong_token) in [(PRIVATE_SUCCESS,false,false),(PRIVATE_UDP_SUCCESS,false,true),(PRIVATE_UDP_SUCCESS,true,false)] {
            assert!(transport_case_protocol(Change::None, crate::mtls::DEFAULT_NODE_NAME, response, anonymous, wrong_token, true, true, false).await.is_err());
        }
    }
    const PRIVATE_SUCCESS:&[u8]=b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: hv2-tcp/1\r\n\r\nready\0\xff";
    #[tokio::test]
    async fn mutual_tls_private_upgrade_preserves_context_early_and_binary_bytes() {
        transport_case(
            Change::None,
            crate::mtls::DEFAULT_NODE_NAME,
            PRIVATE_SUCCESS,
            false,
            false,
        )
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn changes_during_private_upgrade_refuse_stale_claims_and_release_lease() {
        for (change, expected) in [
            (Change::SourceRejoin, io::ErrorKind::Interrupted),
            (Change::DestinationRemove, io::ErrorKind::PermissionDenied),
            (Change::DestinationMove, io::ErrorKind::Interrupted),
            (Change::LeaseRevoke, io::ErrorKind::PermissionDenied),
        ] {
            assert_eq!(
                transport_case(
                    change,
                    crate::mtls::DEFAULT_NODE_NAME,
                    PRIVATE_SUCCESS,
                    false,
                    false
                )
                .await
                .unwrap_err()
                .kind(),
                expected
            );
        }
    }
    #[tokio::test]
    async fn private_transport_refuses_bad_cert_anonymous_peer_bad_token_and_wrong_upgrade() {
        for (name,response,anonymous,wrong_token) in [
            ("wrong-node",PRIVATE_SUCCESS,false,false),
            (crate::mtls::DEFAULT_NODE_NAME,PRIVATE_SUCCESS,true,false),
            (crate::mtls::DEFAULT_NODE_NAME,PRIVATE_SUCCESS,false,true),
            (crate::mtls::DEFAULT_NODE_NAME,b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: wrong/1\r\n\r\n".as_slice(),false,false),
        ] { assert!(transport_case(Change::None,name,response,anonymous,wrong_token).await.is_err()); }
    }
    #[test]
    fn node_urls_are_https_origins_without_target_or_credentials() {
        assert!(node_origin("https://127.0.0.1:1234").is_ok());
        for value in [
            "http://localhost",
            "https://user@localhost",
            "https://localhost/a",
            "https://localhost?x=1",
            "https://localhost/#x",
        ] {
            assert!(node_origin(value).is_err());
        }
    }
    #[tokio::test]
    async fn leased_stream_refuses_revoked_source_and_releases_activity() {
        let active = Arc::new(AtomicBool::new(true));
        let dropped = Arc::new(AtomicUsize::new(0));
        let (a, mut b) = tokio::io::duplex(128);
        let source = crate::store::tests::sandbox("source", "node");
        let mut stream = LeasedStream {
            retry: None,
            contention_since: None,
            waiters: [None, None],
            stream: Some(Box::new(a)),
            source,
            lease: Some(Box::new(Lease {
                active: active.clone(),
                dropped: dropped.clone(),
            })),
        };
        stream.write_all(b"exact-source").await.unwrap();
        let mut bytes = [0; 12];
        b.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"exact-source");
        active.store(false, Ordering::SeqCst);
        assert_eq!(
            stream.write_all(b"refused").await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            stream.read(&mut bytes).await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(b.read(&mut bytes).await.unwrap(), 0);
        active.store(true, Ordering::SeqCst);
        assert_eq!(
            stream.write_all(b"cannot-revive").await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        stream.shutdown().await.unwrap();
        drop(stream);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn stalled_private_upgrade_times_out_and_releases_source_lease() {
        let started = std::time::Instant::now();
        let error = transport_case(
            Change::StallUpgrade,
            crate::mtls::DEFAULT_NODE_NAME,
            PRIVATE_SUCCESS,
            false,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_secs(15));
        assert!(started.elapsed() < Duration::from_secs(20));
    }
    #[tokio::test]
    async fn bound_private_transport_preserves_bytes_and_refuses_upgrade_races() {
        transport_case_mode(
            Change::None,
            crate::mtls::DEFAULT_NODE_NAME,
            PRIVATE_SUCCESS,
            false,
            false,
            true,
        )
        .await
        .unwrap();
        for change in [
            Change::SourceRejoin,
            Change::DestinationRemove,
            Change::DestinationMove,
            Change::LeaseRevoke,
        ] {
            assert!(
                transport_case_mode(
                    change,
                    crate::mtls::DEFAULT_NODE_NAME,
                    PRIVATE_SUCCESS,
                    false,
                    false,
                    true
                )
                .await
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn stale_address_bindings_are_refused_without_contacting_destination() {
        use crate::{
            ownership::OwnerId,
            private_addresses::PrivateAddressBook,
            private_networks::{MembershipChange, NetworkMembershipState},
            store::MemoryStore,
        };
        let store = Arc::new(MemoryStore::new());
        let mut source = crate::store::tests::sandbox("source", "source-node");
        source.owner_id = Some(OwnerId::parse("owner").unwrap());
        let mut destination = crate::store::tests::sandbox("destination", "destination-node");
        destination.owner_id = source.owner_id.clone();
        let tag = || NetworkTag::parse("team").unwrap();
        for record in [&source, &destination] {
            store.put_sandbox(record).await.unwrap();
            let state = NetworkMembershipState::new(record, Some(vec![tag()])).unwrap();
            assert_eq!(
                store
                    .compare_private_membership(None, &state)
                    .await
                    .unwrap(),
                MembershipChange::Applied
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        for id in ["source-node", "destination-node"] {
            let mut node = crate::store::tests::node(id, 1, 4);
            node.api = format!("https://{}", listener.local_addr().unwrap());
            store
                .put_node(&node, Duration::from_secs(60))
                .await
                .unwrap();
        }
        let view = store
            .private_route_snapshot("source", "destination")
            .await
            .unwrap()
            .unwrap();
        let book = PrivateAddressBook::new(&source).unwrap();
        let ip = book
            .allocate(&view, tag(), crate::model::now_ms(), false, false)
            .unwrap();
        let old = book.binding(ip, 18082).unwrap();
        let next = NetworkMembershipState::new(&destination, Some(vec![tag()])).unwrap();
        assert_eq!(
            store
                .compare_private_membership(Some(&old.destination_generation), &next)
                .await
                .unwrap(),
            MembershipChange::Applied
        );
        let connector = PrivateNodeConnector {
            store,
            source,
            client: reqwest::Client::new(),
            token: reqwest::header::HeaderValue::from_static("owned-test"),
        };
        let current = connector
            .store
            .private_route_snapshot("source", "destination")
            .await
            .unwrap()
            .unwrap()
            .claim(tag(), 18082, crate::model::now_ms(), false, false)
            .unwrap();
        let mut claims = vec![old];
        let mut changed = current.clone();
        changed.source_generation = uuid::Uuid::new_v4().to_string();
        claims.push(changed);
        let mut changed = current.clone();
        changed.source_id = "forged".into();
        claims.push(changed);
        let mut changed = current.clone();
        changed.port = 0;
        claims.push(changed);
        let mut changed = current;
        changed.network = NetworkTag::parse("other").unwrap();
        claims.push(changed);
        for claim in claims {
            for udp in [false, true] {
                let dropped = Arc::new(AtomicUsize::new(0));
                let lease = Box::new(Lease {
                    active: Arc::new(AtomicBool::new(true)),
                    dropped: dropped.clone(),
                });
                let result = if udp {
                    connector.open_bound_udp(claim.clone(), lease).await
                } else {
                    connector.open_bound_tcp(claim.clone(), lease).await
                };
                assert!(matches!(result, Err(ref e) if e.kind()==io::ErrorKind::PermissionDenied));
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn transient_source_validation_backpressures_both_halves_without_reviving_revocation() {
        struct BusyLease(Arc<AtomicUsize>);
        impl PrivateSourceLease for BusyLease {
            fn validate(&self, _: &SandboxRecord) -> io::Result<()> {
                match self.0.load(Ordering::SeqCst) {
                    1 => Ok(()),
                    2 => Err(io::ErrorKind::WouldBlock.into()),
                    _ => Err(refused()),
                }
            }
        }
        let state = Arc::new(AtomicUsize::new(2));
        let (local, mut remote) = tokio::io::duplex(64);
        let stream = LeasedStream {
            stream: Some(Box::new(local)),
            lease: Some(Box::new(BusyLease(state.clone()))),
            source: crate::store::tests::sandbox("source", "node"),
            retry: None,
            contention_since: None,
            waiters: [None, None],
        };
        let (mut read, mut write) = tokio::io::split(stream);
        remote.write_all(b"ready").await.unwrap();
        let signal = state.clone();
        let release = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            signal.store(1, Ordering::SeqCst);
        });
        let mut bytes = [0; 5];
        let (a, b) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(read.read_exact(&mut bytes), write.write_all(b"out"))
        })
        .await
        .unwrap();
        a.unwrap();
        b.unwrap();
        assert_eq!(&bytes, b"ready");
        let mut sent = [0; 3];
        remote.read_exact(&mut sent).await.unwrap();
        assert_eq!(&sent, b"out");
        release.await.unwrap();
        state.store(0, Ordering::SeqCst);
        assert_eq!(
            write.write_all(b"denied").await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        state.store(1, Ordering::SeqCst);
        assert!(write.write_all(b"revive").await.is_err());
        assert_eq!(
            remote.read_u8().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn sustained_source_contention_closes_without_reading_unchecked_bytes() {
        struct Busy(Arc<AtomicUsize>);
        impl Drop for Busy {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl PrivateSourceLease for Busy {
            fn validate(&self, _: &SandboxRecord) -> io::Result<()> {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let (local, mut remote) = tokio::io::duplex(64);
        let mut stream = LeasedStream {
            stream: Some(Box::new(local)),
            lease: Some(Box::new(Busy(drops.clone()))),
            source: crate::store::tests::sandbox("source", "node"),
            retry: None,
            contention_since: None,
            waiters: [None, None],
        };
        remote.write_all(b"unchecked").await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), stream.read_u8())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            remote.read_u8().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(stream.read_u8().await.is_err());
    }
}
