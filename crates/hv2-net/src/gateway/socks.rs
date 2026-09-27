//! E2B's `egressProxy`: outbound TCP tunnelled through a SOCKS5 proxy.
//!
//! Applied after the policy has allowed a connection, on the host, so the
//! guest can neither see the proxy nor route around it. It fails closed: a
//! proxy that is down, refuses, or does not speak SOCKS5 fails the guest's
//! connection rather than letting it out directly. When the gateway knows
//! the name a flow is for, it sends the name (`ATYP` domain) and the proxy
//! resolves it, as E2B does.

use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::Upstream;

/// Where to tunnel, and how to authenticate (RFC 1929).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socks5Proxy {
    /// `host:port`; a host name is resolved at dial time.
    pub address: String,
    pub credentials: Option<(String, String)>,
}

impl Socks5Proxy {
    /// Parse E2B's `{address, username, password}`.
    ///
    /// # Errors
    ///
    /// An address without a port, a password without a username, or a field
    /// over RFC 1929's 255 bytes.
    pub fn new(
        address: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let (host, port) = address
            .rsplit_once(':')
            .ok_or_else(|| format!("egress proxy '{address}' needs host:port"))?;
        if host.is_empty() || port.parse::<u16>().is_err() {
            return Err(format!("egress proxy '{address}' needs host:port"));
        }
        let credentials = match (username, password) {
            (None, None) => None,
            (Some(u), p) => {
                let p = p.unwrap_or("");
                if u.len() > 255 || p.len() > 255 || u.is_empty() {
                    return Err("egress proxy credentials must be 1-255 bytes".to_string());
                }
                Some((u.to_string(), p.to_string()))
            }
            (None, Some(_)) => {
                return Err("an egress proxy password needs a username".to_string());
            }
        };
        Ok(Self {
            address: address.to_string(),
            credentials,
        })
    }

    /// Open `stream` (already connected to the proxy) through to
    /// `destination`, naming `name` instead of the address when given.
    ///
    /// # Errors
    ///
    /// Anything but a clean SOCKS5 success.
    pub async fn handshake(
        &self,
        stream: &mut Box<dyn Upstream>,
        destination: SocketAddr,
        name: Option<&str>,
    ) -> io::Result<()> {
        let refused =
            |what: &str| io::Error::new(io::ErrorKind::ConnectionRefused, what.to_string());

        let greeting: &[u8] = if self.credentials.is_some() {
            &[5, 2, 0, 2]
        } else {
            &[5, 1, 0]
        };
        stream.write_all(greeting).await?;
        let mut choice = [0u8; 2];
        stream.read_exact(&mut choice).await?;
        if choice[0] != 5 {
            return Err(refused("the egress proxy does not speak SOCKS5"));
        }
        match (choice[1], &self.credentials) {
            (0, _) => {}
            (2, Some((user, pass))) => {
                let mut auth = vec![1, user.len() as u8];
                auth.extend_from_slice(user.as_bytes());
                auth.push(pass.len() as u8);
                auth.extend_from_slice(pass.as_bytes());
                stream.write_all(&auth).await?;
                let mut status = [0u8; 2];
                stream.read_exact(&mut status).await?;
                if status[1] != 0 {
                    return Err(refused("the egress proxy refused the credentials"));
                }
            }
            _ => return Err(refused("the egress proxy accepts no method offered")),
        }

        let mut request = vec![5, 1, 0];
        match (name, destination.ip()) {
            (Some(name), _) if name.len() <= 255 => {
                request.push(3);
                request.push(name.len() as u8);
                request.extend_from_slice(name.as_bytes());
            }
            (_, IpAddr::V4(v4)) => {
                request.push(1);
                request.extend_from_slice(&v4.octets());
            }
            (_, IpAddr::V6(v6)) => {
                request.push(4);
                request.extend_from_slice(&v6.octets());
            }
        }
        request.extend_from_slice(&destination.port().to_be_bytes());
        stream.write_all(&request).await?;

        let mut head = [0u8; 4];
        stream.read_exact(&mut head).await?;
        if head[0] != 5 {
            return Err(refused("the egress proxy's reply is not SOCKS5"));
        }
        if head[1] != 0 {
            return Err(refused(match head[1] {
                2 => "the egress proxy's rules refused the connection",
                3 => "the egress proxy: network unreachable",
                4 => "the egress proxy: host unreachable",
                5 => "the egress proxy: connection refused",
                _ => "the egress proxy failed the connection",
            }));
        }
        // The bound address, which nothing here needs.
        let rest = match head[3] {
            1 => 4 + 2,
            4 => 16 + 2,
            3 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                usize::from(len[0]) + 2
            }
            _ => {
                return Err(refused(
                    "the egress proxy's reply has an unknown address type",
                ))
            }
        };
        let mut skip = vec![0u8; rest];
        stream.read_exact(&mut skip).await?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    /// A SOCKS5 server that records what it was asked for and connects to
    /// `forward` whatever that was.
    pub(crate) async fn socks_server(
        forward: SocketAddr,
        credentials: Option<(&'static str, &'static str)>,
    ) -> (SocketAddr, Arc<parking_lot::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut head = [0u8; 2];
                    s.read_exact(&mut head).await.ok()?;
                    let mut methods = vec![0u8; usize::from(head[1])];
                    s.read_exact(&mut methods).await.ok()?;
                    if let Some((user, pass)) = credentials {
                        s.write_all(&[5, 2]).await.ok()?;
                        let mut v = [0u8; 2];
                        s.read_exact(&mut v).await.ok()?;
                        let mut u = vec![0u8; usize::from(v[1])];
                        s.read_exact(&mut u).await.ok()?;
                        let mut pl = [0u8; 1];
                        s.read_exact(&mut pl).await.ok()?;
                        let mut p = vec![0u8; usize::from(pl[0])];
                        s.read_exact(&mut p).await.ok()?;
                        let ok = u == user.as_bytes() && p == pass.as_bytes();
                        s.write_all(&[1, if ok { 0 } else { 1 }]).await.ok()?;
                        if !ok {
                            return None;
                        }
                    } else {
                        s.write_all(&[5, 0]).await.ok()?;
                    }
                    let mut req = [0u8; 4];
                    s.read_exact(&mut req).await.ok()?;
                    let target = match req[3] {
                        1 => {
                            let mut a = [0u8; 6];
                            s.read_exact(&mut a).await.ok()?;
                            format!(
                                "{}.{}.{}.{}:{}",
                                a[0],
                                a[1],
                                a[2],
                                a[3],
                                u16::from_be_bytes([a[4], a[5]])
                            )
                        }
                        3 => {
                            let mut l = [0u8; 1];
                            s.read_exact(&mut l).await.ok()?;
                            let mut n = vec![0u8; usize::from(l[0]) + 2];
                            s.read_exact(&mut n).await.ok()?;
                            let port = u16::from_be_bytes([n[n.len() - 2], n[n.len() - 1]]);
                            format!("{}:{port}", String::from_utf8_lossy(&n[..n.len() - 2]))
                        }
                        _ => return None,
                    };
                    log.lock().push(target);
                    let mut up = tokio::net::TcpStream::connect(forward).await.ok()?;
                    s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.ok()?;
                    let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
                    Some(())
                });
            }
        });
        (addr, seen)
    }

    #[test]
    fn parses_e2b_shapes_and_refuses_the_rest() {
        assert!(Socks5Proxy::new("proxy.example.com:1080", None, None).is_ok());
        assert!(Socks5Proxy::new("proxy.example.com", None, None).is_err());
        assert!(Socks5Proxy::new("p:1080", None, Some("x")).is_err());
        assert!(Socks5Proxy::new("p:1080", Some(&"u".repeat(256)), Some("x")).is_err());
    }
}
