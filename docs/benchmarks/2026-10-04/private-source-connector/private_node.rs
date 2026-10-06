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
fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "private source or route refused",
    )
}
fn node_origin(value: &str) -> io::Result<reqwest::Url> {
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
        lease.validate(&self.source)?;
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
        lease.validate(&self.source)?;
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
        let attempt = async {
            let (claim, node, url) = self
                .target(destination, network.clone(), port, lease.as_ref())
                .await?;
            let context = serde_json::to_string(&claim)
                .map_err(|_| io::Error::other("private route context encoding failed"))?;
            if context.len() > 2048 {
                return Err(refused());
            }
            let response = self
                .client
                .get(url.clone())
                .version(reqwest::Version::HTTP_11)
                .header("connection", "upgrade")
                .header("upgrade", hv2_api::tcp_tunnel::PROTOCOL)
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
                    != Some(hv2_api::tcp_tunnel::PROTOCOL)
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
            }) as Box<dyn NativeTunnel>)
        };
        tokio::time::timeout(Duration::from_secs(15), attempt)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "private TCP setup timed out"))?
    }
}
struct LeasedStream {
    stream: Option<Box<dyn NativeTunnel>>,
    lease: Option<Box<dyn PrivateSourceLease>>,
    source: SandboxRecord,
}
impl LeasedStream {
    fn check_source(&mut self) -> io::Result<()> {
        let result = self
            .lease
            .as_ref()
            .map(|lease| lease.validate(&self.source))
            .unwrap_or_else(|| Err(refused()));
        if result.is_err() {
            self.stream.take();
            self.lease.take();
        }
        result
    }
}
impl AsyncRead for LeasedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Err(error) = self.check_source() {
            return Poll::Ready(Err(error));
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
        if let Err(error) = self.check_source() {
            return Poll::Ready(Err(error));
        }
        match self.stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_write(cx, buf),
            None => Poll::Ready(Err(refused())),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(error) = self.check_source() {
            return Poll::Ready(Err(error));
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
}
