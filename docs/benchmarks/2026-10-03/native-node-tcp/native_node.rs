//! Reservation-checked mutual-TLS node connector for native TCP ingress.
use crate::{
    mtls::Mtls,
    native_tcp::{NativeTcpConnector, NativeTunnel},
    ports::{PortAllocation, PortProtocol},
    store::ClusterStore,
};
use std::{io, sync::Arc, time::Duration};

/// Trust is operator-provisioned. Callers cannot inject arbitrary HTTP clients,
/// redirect policy, upstream URLs or credentials through an ingress stream.
pub struct NativeNodeConnector {
    store: Arc<dyn ClusterStore>,
    client: reqwest::Client,
    token: reqwest::header::HeaderValue,
}
impl NativeNodeConnector {
    /// Requires cluster mutual TLS and a nonempty bounded cluster token.
    /// # Errors
    /// Invalid token or TLS client configuration; errors omit credential content.
    pub fn new(store: Arc<dyn ClusterStore>, tls: &Mtls, token: &str) -> io::Result<Self> {
        if token.trim().is_empty() || token.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid cluster token",
            ));
        }
        let mut token = reqwest::header::HeaderValue::from_str(token)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cluster token"))?;
        token.set_sensitive(true);
        let client = tls
            .tcp_http_client()
            .map_err(|_| io::Error::other("node TLS client configuration failed"))?;
        Ok(Self {
            store,
            client,
            token,
        })
    }
    async fn target(&self, allocation: &PortAllocation) -> io::Result<(String, reqwest::Url)> {
        allocation.validate().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native TCP reservation",
            )
        })?;
        if !matches!(
            allocation.protocol(),
            PortProtocol::Tcp | PortProtocol::Both
        ) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reservation does not permit TCP",
            ));
        }
        let rows = self
            .store
            .port_allocations(Some(allocation.sandbox_id()))
            .await
            .map_err(|_| io::Error::other("port reservation lookup unavailable"))?;
        if !rows.iter().any(|row| row == allocation) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "port reservation changed or removed",
            ));
        }
        let sandbox = self
            .store
            .sandbox(allocation.sandbox_id())
            .await
            .map_err(|_| io::Error::other("sandbox lookup unavailable"))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "sandbox missing"))?;
        if sandbox.paused {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "sandbox paused"));
        }
        let node = self
            .store
            .node(&sandbox.node_id)
            .await
            .map_err(|_| io::Error::other("node lookup unavailable"))?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "sandbox node unavailable")
            })?;
        let mut url = reqwest::Url::parse(&node.api)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid node API URL"))?;
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
                "native node API requires an HTTPS origin",
            ));
        }
        url.set_path(&format!(
            "/sandboxes/{}/ports/{}/tcp",
            allocation.sandbox_id(),
            allocation.machine_port()
        ));
        Ok((sandbox.node_id, url))
    }
    async fn open(&self, allocation: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
        let (node_id, url) = self.target(allocation).await?;
        let response = self
            .client
            .get(url.clone())
            .version(reqwest::Version::HTTP_11)
            .header("connection", "upgrade")
            .header("upgrade", hv2_api::tcp_tunnel::PROTOCOL)
            .header(crate::control::CLUSTER_TOKEN_HEADER, self.token.clone())
            .send()
            .await
            .map_err(|_| io::Error::other("authenticated node connection failed"))?;
        if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
            || response
                .headers()
                .get("upgrade")
                .and_then(|value| value.to_str().ok())
                != Some(hv2_api::tcp_tunnel::PROTOCOL)
            || !response
                .headers()
                .get_all("connection")
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(','))
                .any(|value| value.trim().eq_ignore_ascii_case("upgrade"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "node refused exact TCP upgrade",
            ));
        }
        let stream = response
            .upgrade()
            .await
            .map_err(|_| io::Error::other("node TCP upgrade failed"))?;
        // Refuse deletion, protocol changes, pause and relocation during setup.
        // Worker/node lifecycle cancellation must still close later changes.
        if self.target(allocation).await? != (node_id, url) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "sandbox node changed during TCP setup",
            ));
        }
        Ok(Box::new(stream))
    }
}
#[async_trait::async_trait]
impl NativeTcpConnector for NativeNodeConnector {
    async fn open_tcp(&self, allocation: &PortAllocation) -> io::Result<Box<dyn NativeTunnel>> {
        tokio::time::timeout(Duration::from_secs(15), self.open(allocation))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "native node TCP setup timed out")
            })?
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mtls::DEFAULT_NODE_NAME,
        ports::{PortClaim, PublicPortRange},
        store::MemoryStore,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    // Aborts fixture work if a timeout or assertion prevents awaiting it.
    struct OwnedTask<T>(tokio::task::JoinHandle<T>);
    impl<T> std::future::Future for OwnedTask<T> {
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
    struct Identity {
        _directory: tempfile::TempDir,
        node: Mtls,
        gateway: Mtls,
    }
    fn identity(name: &str) -> Identity {
        let directory = tempfile::tempdir().unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
        let ca = params.self_signed(&key).unwrap().pem();
        let issuer = rcgen::Issuer::new(params, key);
        let make = |stem: &str, name: &str, usage| {
            let key = rcgen::KeyPair::generate().unwrap();
            let mut params = rcgen::CertificateParams::new(vec![name.to_owned()]).unwrap();
            params.extended_key_usages = vec![usage];
            let cert = params.signed_by(&key, &issuer).unwrap().pem();
            let ca_path = directory.path().join(format!("{stem}-ca.pem"));
            let cert_path = directory.path().join(format!("{stem}.pem"));
            let key_path = directory.path().join(format!("{stem}.key"));
            std::fs::write(&ca_path, &ca).unwrap();
            std::fs::write(&cert_path, cert).unwrap();
            std::fs::write(&key_path, key.serialize_pem()).unwrap();
            Mtls::load(&ca_path, &cert_path, &key_path, DEFAULT_NODE_NAME).unwrap()
        };
        let node = make("node", name, rcgen::ExtendedKeyUsagePurpose::ServerAuth);
        let gateway = make(
            "gateway",
            "gateway",
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        );
        Identity {
            _directory: directory,
            node,
            gateway,
        }
    }
    async fn records(store: &MemoryStore, api: String) -> PortAllocation {
        let mut node = crate::store::tests::node("node", 0, 1);
        node.api = api;
        store
            .put_node(&node, Duration::from_secs(60))
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
                PublicPortRange::new(41000, 41000).unwrap(),
            )
            .await
            .unwrap()
        else {
            panic!("allocation failed");
        };
        row
    }
    enum SetupChange {
        Delete,
        Pause,
        UdpOnly,
        RelocateSameUrl,
    }
    async fn fixture(
        listener: TcpListener,
        node: &Mtls,
        response: &'static [u8],
        remove: Option<(Arc<MemoryStore>, SetupChange)>,
    ) -> OwnedTask<()> {
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(node.server_config().unwrap()));
        OwnedTask(tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                return;
            };
            assert_eq!(
                tls.get_ref().1.alpn_protocol(),
                Some(b"http/1.1".as_slice())
            );
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(tls.read_u8().await.unwrap());
                assert!(header.len() < 8192);
            }
            let text = std::str::from_utf8(&header).unwrap().to_ascii_lowercase();
            assert!(text.starts_with("get /sandboxes/owned-vm/ports/8080/tcp http/1.1\r\n"));
            assert!(text.contains("x-hv2-cluster-token: owned-cluster-token\r\n"));
            assert!(!text.contains("x-api-key:"));
            if let Some((store, action)) = remove {
                match action {
                    SetupChange::Delete => {
                        store.delete_sandbox("owned-vm").await.unwrap();
                    }
                    SetupChange::Pause => {
                        let mut record = store.sandbox("owned-vm").await.unwrap().unwrap();
                        record.paused = true;
                        store.put_sandbox(&record).await.unwrap();
                    }
                    SetupChange::UdpOnly => {
                        store
                            .claim_port(
                                "owned-vm",
                                8080,
                                "owner-a",
                                PortProtocol::Udp,
                                PublicPortRange::new(41000, 41000).unwrap(),
                            )
                            .await
                            .unwrap();
                    }
                    SetupChange::RelocateSameUrl => {
                        let mut record = store.sandbox("owned-vm").await.unwrap().unwrap();
                        let mut node = store.node(&record.node_id).await.unwrap().unwrap();
                        node.id = "other-node".into();
                        store
                            .put_node(&node, Duration::from_secs(60))
                            .await
                            .unwrap();
                        record.node_id = node.id;
                        store.put_sandbox(&record).await.unwrap();
                    }
                }
            }
            tls.write_all(response).await.unwrap();
            if response.starts_with(b"HTTP/1.1 101") {
                let mut bytes = [0; 8192];
                while let Ok(n) = tls.read(&mut bytes).await {
                    if n == 0 {
                        break;
                    }
                    if tls.write_all(&bytes[..n]).await.is_err() {
                        break;
                    }
                }
            }
            let _ = tls.shutdown().await;
        }))
    }
    const SUCCESS: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: hv2-tcp/1\r\n\r\nready\0\xff";
    #[tokio::test]
    async fn mutual_tls_exact_upgrade_and_early_bytes() {
        let identity = identity(DEFAULT_NODE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let store = Arc::new(MemoryStore::new());
        let row = records(&store, format!("https://{address}")).await;
        let server = fixture(listener, &identity.node, SUCCESS, None).await;
        let connector =
            NativeNodeConnector::new(store, &identity.gateway, "owned-cluster-token").unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = connector.open_tcp(&row).await.unwrap();
            let mut ready = [0; 7];
            stream.read_exact(&mut ready).await.unwrap();
            assert_eq!(&ready, b"ready\0\xff");
            stream.write_all(b"tcp\0binary\xff").await.unwrap();
            let mut bytes = [0; 11];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"tcp\0binary\xff");
            stream.shutdown().await.unwrap();
            drop(stream);
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn lifecycle_and_reservation_refusals_do_not_dial() {
        let identity = identity(DEFAULT_NODE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let store = Arc::new(MemoryStore::new());
        let row = records(&store, format!("https://{address}")).await;
        let connector =
            NativeNodeConnector::new(store.clone(), &identity.gateway, "owned-cluster-token")
                .unwrap();
        let mut foreign = row.clone();
        foreign.owner_id = "other-owner".into();
        assert!(connector.open_tcp(&foreign).await.is_err());
        let mut foreign = row.clone();
        foreign.machine_port = 8081;
        assert!(connector.open_tcp(&foreign).await.is_err());
        let mut record = store.sandbox("owned-vm").await.unwrap().unwrap();
        record.paused = true;
        store.put_sandbox(&record).await.unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        record.paused = false;
        store.put_sandbox(&record).await.unwrap();
        store.remove_node("node").await.unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        records(&store, format!("http://{address}")).await;
        assert!(connector.open_tcp(&row).await.is_err());
        records(&store, format!("https://{address}/?token=not-permitted")).await;
        assert!(connector.open_tcp(&row).await.is_err());
        records(&store, format!("https://{address}")).await;
        store
            .claim_port(
                "owned-vm",
                8080,
                "owner-a",
                PortProtocol::Udp,
                PublicPortRange::new(41000, 41000).unwrap(),
            )
            .await
            .unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        let udp = store
            .port_allocations(Some("owned-vm"))
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(connector.open_tcp(&udp).await.is_err());
        store.delete_sandbox("owned-vm").await.unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        for token in ["", " ", "bad\r\nheader"] {
            assert!(NativeNodeConnector::new(store.clone(), &identity.gateway, token).is_err());
        }
    }
    #[tokio::test]
    async fn exact_protocol_redirect_and_setup_deletion_are_refused() {
        let identity = identity(DEFAULT_NODE_NAME);
        for (response, remove) in [(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: hv2-udp/1\r\n\r\n".as_slice(), false),
            (b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://127.0.0.1:1/leak\r\nContent-Length: 0\r\n\r\n".as_slice(), false), (SUCCESS, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
            let store = Arc::new(MemoryStore::new()); let row = records(&store, format!("https://{address}")).await;
            let server = fixture(listener, &identity.node, response, if remove { Some((store.clone(), SetupChange::Delete)) } else { None }).await;
            let connector = NativeNodeConnector::new(store, &identity.gateway, "owned-cluster-token").unwrap();
            assert!(connector.open_tcp(&row).await.is_err());
            tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
        }
    }
    #[tokio::test]
    async fn wrong_node_certificate_name_is_refused() {
        let identity = identity("wrong-node-name");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let store = Arc::new(MemoryStore::new());
        let row = records(&store, format!("https://{address}")).await;
        let server = fixture(listener, &identity.node, SUCCESS, None).await;
        let connector =
            NativeNodeConnector::new(store, &identity.gateway, "owned-cluster-token").unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn native_ingress_through_authenticated_node_relay() {
        use crate::native_tcp::{serve_tcp, TcpRelayLimits};
        let identity = identity(DEFAULT_NODE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let node_address = listener.local_addr().unwrap();
        let ingress = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = ingress.local_addr().unwrap();
        let store = Arc::new(MemoryStore::new());
        records(&store, format!("https://{node_address}")).await;
        assert!(store
            .delete_port("owned-vm", 8080, "owner-a")
            .await
            .unwrap());
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
        let server = fixture(listener, &identity.node, SUCCESS, None).await;
        let connector = Arc::new(
            NativeNodeConnector::new(store, &identity.gateway, "owned-cluster-token").unwrap(),
        );
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let relay = OwnedTask(tokio::spawn(serve_tcp(
            ingress,
            row,
            connector,
            TcpRelayLimits::new(1, Duration::from_secs(3), Duration::from_secs(10)).unwrap(),
            async {
                let _ = stopped.await;
            },
        )));
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            let mut ready = [0; 7];
            client.read_exact(&mut ready).await.unwrap();
            assert_eq!(&ready, b"ready\0\xff");
            let (mut reader, mut writer) = client.into_split();
            let payload: Vec<u8> = (0..1_048_576).map(|n| (n % 251) as u8).collect();
            let (sent, bytes) = tokio::join!(
                async {
                    writer.write_all(&payload).await.unwrap();
                    writer.shutdown().await
                },
                async {
                    let mut bytes = Vec::new();
                    reader.read_to_end(&mut bytes).await.unwrap();
                    bytes
                }
            );
            sent.unwrap();
            assert_eq!(bytes, payload);
            server.await.unwrap();
            stop.send(()).unwrap();
            let stats = relay.await.unwrap().unwrap();
            assert_eq!(stats.admitted, 1);
            assert_eq!(stats.completed, 1);
        })
        .await
        .unwrap();
        let _released = TcpListener::bind(address).await.unwrap();
    }
    #[tokio::test]
    async fn foreign_certificate_authority_is_refused() {
        let node_identity = identity(DEFAULT_NODE_NAME);
        let foreign = identity(DEFAULT_NODE_NAME);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let store = Arc::new(MemoryStore::new());
        let row = records(&store, format!("https://{address}")).await;
        let server = fixture(listener, &node_identity.node, SUCCESS, None).await;
        let connector =
            NativeNodeConnector::new(store, &foreign.gateway, "owned-cluster-token").unwrap();
        assert!(connector.open_tcp(&row).await.is_err());
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn lifecycle_protocol_and_same_url_relocation_during_setup_are_refused() {
        let identity = identity(DEFAULT_NODE_NAME);
        for action in [
            SetupChange::Pause,
            SetupChange::UdpOnly,
            SetupChange::RelocateSameUrl,
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let store = Arc::new(MemoryStore::new());
            let row = records(&store, format!("https://{address}")).await;
            let server = fixture(
                listener,
                &identity.node,
                SUCCESS,
                Some((store.clone(), action)),
            )
            .await;
            let connector =
                NativeNodeConnector::new(store, &identity.gateway, "owned-cluster-token").unwrap();
            assert!(connector.open_tcp(&row).await.is_err());
            tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
