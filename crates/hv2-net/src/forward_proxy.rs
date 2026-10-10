//! A forward proxy that lets through what a [`NetworkPolicy`] allows.
//!
//! # What this is for
//!
//! The [`gateway`](crate::gateway) decides egress for a guest by being its
//! router: every packet passes through it. A sandboxed *process* has no
//! router to put in the way. What it can be given instead is a network of
//! exactly one address, a port on the host's loopback
//! (`hv2_sandbox::NetworkPolicy::Proxy`), and then whatever listens there is
//! its whole network. This is something to listen there.
//!
//! It speaks the two things a program's `HTTPS_PROXY` and `HTTP_PROXY` make
//! it send: `CONNECT host:port`, and a plain request with an absolute
//! `http://` target. For each it asks the same policy the gateway asks, and
//! either connects or answers 403.
//!
//! # What is decided, and on what
//!
//! - **A name is checked before it is resolved.** Looking a name up is
//!   itself egress, so a name the policy does not allow is refused without a
//!   query ([`NetworkPolicy::may_resolve`]).
//! - **The address connected to is the one that was checked.** The name is
//!   resolved here, each answer is put to the policy with the name, and the
//!   connection goes to an address that passed, not to the name again. An
//!   allowed name that resolves to a reserved address is refused: that is
//!   the route to the host's own loopback and the network behind it.
//! - **A bare address is decided as an address.** No name rule speaks for
//!   it.
//!
//! # What is not looked at
//!
//! Nothing inside a `CONNECT` tunnel: the program's TLS runs end to end and
//! this sees ciphertext. So the name decided on is the one the program asked
//! the proxy for. A program that asks for an allowed name and then speaks to
//! a different site on the same address, which a shared front end makes
//! possible, is not noticed here.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::network_policy::{normalise, AddressVerdict, NetworkPolicy, Verdict};

/// The most a request's head may be before it is refused.
const MAX_HEAD: usize = 16 * 1024;
/// How long a client has to send its request's head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a destination has to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// One request the proxy decided, for whoever is watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The host the program asked for: a name or an address.
    pub host: String,
    /// The port it asked for.
    pub port: u16,
    /// Whether it was let through.
    pub allowed: bool,
    /// Why, in the policy's words.
    pub reason: &'static str,
}

/// Called with each decision as it is made.
pub type Observer = Arc<dyn Fn(&Decision) + Send + Sync>;

/// A proxy listening on this machine's loopback until it is dropped.
pub struct ForwardProxy {
    port: u16,
    accepting: tokio::task::JoinHandle<()>,
}

impl ForwardProxy {
    /// Listen on a port of the system's choosing on `127.0.0.1`, deciding
    /// each request with `policy`.
    ///
    /// # Errors
    ///
    /// When nothing can be bound on loopback.
    pub async fn start(policy: NetworkPolicy, observer: Option<Observer>) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let policy = Arc::new(policy);
        let accepting = tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    // Out of descriptors, most likely. Give up the slice
                    // and try again; there is nobody to report it to.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let (policy, observer) = (Arc::clone(&policy), observer.clone());
                tokio::spawn(async move {
                    serve(client, &policy, observer.as_ref()).await;
                });
            }
        });
        Ok(Self { port, accepting })
    }

    /// The port it listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for ForwardProxy {
    fn drop(&mut self) {
        // Stops accepting. A connection already being carried is left to
        // finish; its ends close when the program's does.
        self.accepting.abort();
    }
}

/// What a request asked to reach.
#[derive(Debug, PartialEq, Eq)]
struct Target {
    host: String,
    port: u16,
    /// For a plain request, its head as the destination should see it. A
    /// `CONNECT` has none: nothing of it is forwarded.
    forward: Option<Vec<u8>>,
}

/// A refusal, as a status line and a sentence.
type Refusal = (&'static str, String);

fn refusal(status: &'static str, why: impl Into<String>) -> Refusal {
    (status, why.into())
}

/// Split `host:port`, with an IPv6 address in brackets.
fn host_and_port(authority: &str, default_port: Option<u16>) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        (host, after.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(text) => text.parse().ok().filter(|port| *port != 0)?,
        None => default_port?,
    };
    (!host.is_empty()).then(|| (host.to_string(), port))
}

/// Read a request's head into what it asks to reach.
fn parse(head: &[u8]) -> Result<Target, Refusal> {
    let bad = |why: &str| refusal("400 Bad Request", why);
    let text = std::str::from_utf8(head).map_err(|_| bad("the request is not text"))?;
    let (line, headers) = text
        .split_once("\r\n")
        .ok_or_else(|| bad("the request has no first line"))?;
    let mut words = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (words.next(), words.next(), words.next(), words.next())
    else {
        return Err(bad(
            "the request line is not a method, a target and a version",
        ));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(bad("only HTTP/1 is spoken here"));
    }
    if method == "CONNECT" {
        let (host, port) =
            host_and_port(target, None).ok_or_else(|| bad("CONNECT takes host:port"))?;
        return Ok(Target {
            host,
            port,
            forward: None,
        });
    }
    // A plain request names its destination in full, which is how a program
    // told to use a proxy sends one. Anything else is a request meant for
    // this proxy itself, and there is nothing here to serve.
    let rest = target
        .strip_prefix("http://")
        .ok_or_else(|| bad("a request through this proxy names an http:// address in full"))?;
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let (host, port) =
        host_and_port(authority, Some(80)).ok_or_else(|| bad("the address has no host"))?;
    // To the destination: the path alone, none of what was said to the
    // proxy, and one request to a connection, so that a second request on
    // it cannot be for somewhere this never decided.
    let mut forward = format!("{method} {path} {version}\r\n");
    for header in headers.split("\r\n").filter(|header| !header.is_empty()) {
        let name = header.split(':').next().unwrap_or("").trim();
        let dropped = ["proxy-connection", "proxy-authorization", "connection"]
            .iter()
            .any(|unwanted| name.eq_ignore_ascii_case(unwanted));
        if !dropped {
            forward.push_str(header);
            forward.push_str("\r\n");
        }
    }
    forward.push_str("Connection: close\r\n\r\n");
    Ok(Target {
        host,
        port,
        forward: Some(forward.into_bytes()),
    })
}

/// Where `target` may be connected to, or why not.
async fn decide(
    policy: &NetworkPolicy,
    host: &str,
    port: u16,
) -> Result<(SocketAddr, &'static str), (Refusal, &'static str)> {
    let forbidden = |reason: &'static str, why: String| (refusal("403 Forbidden", why), reason);
    if let Ok(address) = host.parse::<IpAddr>() {
        return match policy.decide_address(address) {
            AddressVerdict::Allow(reason) => Ok((SocketAddr::new(address, port), reason)),
            AddressVerdict::Deny(reason) => Err(forbidden(
                reason,
                format!("{address} is not allowed ({reason})"),
            )),
            // Only a name rule could allow it, and no name was given.
            AddressVerdict::NeedsName => Err(forbidden(
                "address without a name",
                format!("{address} is not allowed by address; ask for it by an allowed name"),
            )),
        };
    }
    let name = normalise(host);
    let plausible = !name.is_empty()
        && name.len() <= 253
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    if !plausible {
        return Err((
            refusal("400 Bad Request", "that is not a host name"),
            "not a host name",
        ));
    }
    // Before any query: the question itself would leave the machine.
    if !policy.may_resolve(&name) {
        return Err(forbidden(
            "name not allowed",
            format!("{name} is not an allowed host"),
        ));
    }
    let answers = tokio::net::lookup_host((name.as_str(), port))
        .await
        .map_err(|e| {
            (
                refusal("502 Bad Gateway", format!("{name} did not resolve: {e}")),
                "did not resolve",
            )
        })?;
    let mut last = "did not resolve";
    for answer in answers {
        match policy.decide(answer.ip(), &name) {
            (Verdict::Allow, reason) => return Ok((answer, reason)),
            (Verdict::Deny, reason) => last = reason,
        }
    }
    Err(forbidden(
        last,
        format!("{name} resolves to nothing that is allowed ({last})"),
    ))
}

async fn refuse(client: &mut TcpStream, (status, why): &Refusal) {
    let body = format!("{why}\n");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = client.write_all(response.as_bytes()).await;
    let _ = client.shutdown().await;
    // Closing with the client's bytes unread resets the connection, and the
    // answer just written can be lost with it. So what it is still sending
    // is read and dropped, for a moment and up to a size.
    let mut unread = [0u8; 4096];
    let mut dropped = 0;
    while dropped < 8 * MAX_HEAD {
        match tokio::time::timeout(Duration::from_secs(1), client.read(&mut unread)).await {
            Ok(Ok(read)) if read > 0 => dropped += read,
            _ => break,
        }
    }
}

/// Read up to the end of a request's head. Returns the head, terminator
/// included, and whatever arrived after it.
async fn read_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), Refusal> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = memchr::memmem::find(&buffer, b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            return Ok((buffer, rest));
        }
        if buffer.len() > MAX_HEAD {
            return Err(refusal(
                "431 Request Header Fields Too Large",
                "the request's head is too large",
            ));
        }
        match client.read(&mut chunk).await {
            Ok(0) | Err(_) => {
                return Err(refusal("400 Bad Request", "the request ended early"));
            }
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
}

async fn serve(mut client: TcpStream, policy: &NetworkPolicy, observer: Option<&Observer>) {
    let head = match tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client)).await {
        Ok(Ok(head)) => head,
        Ok(Err(why)) => return refuse(&mut client, &why).await,
        // Said nothing in time: there is nobody to answer.
        Err(_) => return,
    };
    let (head, early) = head;
    let target = match parse(&head) {
        Ok(target) => target,
        Err(why) => return refuse(&mut client, &why).await,
    };
    let decided = decide(policy, &target.host, target.port).await;
    if let Some(observer) = observer {
        observer(&Decision {
            host: target.host.clone(),
            port: target.port,
            allowed: decided.is_ok(),
            reason: match &decided {
                Ok((_, reason)) | Err((_, reason)) => reason,
            },
        });
    }
    let address = match decided {
        Ok((address, _)) => address,
        Err((why, _)) => return refuse(&mut client, &why).await,
    };
    let mut destination =
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(destination)) => destination,
            Ok(Err(e)) => {
                let why = refusal(
                    "502 Bad Gateway",
                    format!("could not connect to {}: {e}", target.host),
                );
                return refuse(&mut client, &why).await;
            }
            Err(_) => {
                let why = refusal(
                    "504 Gateway Timeout",
                    format!("{} did not answer", target.host),
                );
                return refuse(&mut client, &why).await;
            }
        };
    let opened = match &target.forward {
        Some(forward) => destination.write_all(forward).await,
        None => {
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
        }
    };
    if opened.is_err() || destination.write_all(&early).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut destination).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_policy::Cidr;
    use std::sync::Mutex;

    /// A policy that allows `allowed` and nothing else. Loopback is reserved,
    /// so a test that must reach a listener here opens it as an operator
    /// would.
    fn only(allowed: &[&str], open_loopback: bool) -> NetworkPolicy {
        let allowed: Vec<String> = allowed.iter().map(ToString::to_string).collect();
        let policy = NetworkPolicy::from_e2b(Some(false), &allowed, &[], &[], Verdict::Deny)
            .expect("a policy");
        if open_loopback {
            policy.with_tenant_reserved(&[Cidr::parse("127.0.0.0/8").expect("a range")])
        } else {
            policy
        }
    }

    /// Send `request` to the proxy and read everything it answers.
    async fn ask(proxy: &ForwardProxy, request: &str) -> String {
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port()))
            .await
            .expect("connect");
        // A refusal can arrive, and the connection close, while a long
        // request is still being written.
        let _ = client.write_all(request.as_bytes()).await;
        let mut answer = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut answer))
            .await
            .expect("an answer in time");
        String::from_utf8_lossy(&answer).to_string()
    }

    /// A destination that records what it was sent and answers one line.
    async fn destination() -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let record = Arc::clone(&record);
                tokio::spawn(async move {
                    let mut buffer = [0u8; 4096];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    record
                        .lock()
                        .expect("lock")
                        .push(String::from_utf8_lossy(&buffer[..read]).to_string());
                    let _ = stream.write_all(b"from the destination\n").await;
                });
            }
        });
        (port, seen)
    }

    #[test]
    fn a_request_head_says_where_it_is_going() {
        let connect = parse(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .expect("a target");
        assert_eq!(
            (connect.host.as_str(), connect.port, connect.forward),
            ("example.com", 443, None)
        );
        let v6 = parse(b"CONNECT [2001:db8::1]:8443 HTTP/1.1\r\n\r\n").expect("a target");
        assert_eq!((v6.host.as_str(), v6.port), ("2001:db8::1", 8443));

        // A plain request goes on without what was said to the proxy, and
        // as the only request on its connection.
        let plain = parse(
            b"GET http://example.com:8080/a/b?c=d HTTP/1.1\r\nHost: example.com:8080\r\n\
              Proxy-Connection: keep-alive\r\nProxy-Authorization: Basic eDp5\r\n\
              Connection: keep-alive\r\nAccept: */*\r\n\r\n",
        )
        .expect("a target");
        assert_eq!((plain.host.as_str(), plain.port), ("example.com", 8080));
        assert_eq!(
            String::from_utf8(plain.forward.expect("a head to forward")).expect("text"),
            "GET /a/b?c=d HTTP/1.1\r\nHost: example.com:8080\r\nAccept: */*\r\n\
             Connection: close\r\n\r\n"
        );
        let bare = parse(b"GET http://example.com HTTP/1.1\r\n\r\n").expect("a target");
        assert_eq!((bare.host.as_str(), bare.port), ("example.com", 80));
        assert!(String::from_utf8(bare.forward.expect("a head"))
            .expect("text")
            .starts_with("GET / HTTP/1.1\r\n"));

        for wrong in [
            &b"GET /index.html HTTP/1.1\r\n\r\n"[..],
            b"CONNECT example.com HTTP/1.1\r\n\r\n",
            b"CONNECT example.com:0 HTTP/1.1\r\n\r\n",
            b"CONNECT :443 HTTP/1.1\r\n\r\n",
            b"GET https://example.com/ HTTP/1.1\r\n\r\n",
            b"GET http://example.com/ HTTP/2\r\n\r\n",
            b"GET  http://example.com/ HTTP/1.1\r\n\r\n",
            b"nonsense\r\n\r\n",
        ] {
            let refused = parse(wrong).expect_err("a refusal");
            assert_eq!(
                refused.0,
                "400 Bad Request",
                "{:?}",
                String::from_utf8_lossy(wrong)
            );
        }
    }

    #[tokio::test]
    async fn an_allowed_address_is_tunnelled_and_the_bytes_arrive() {
        let (port, seen) = destination().await;
        let proxy = ForwardProxy::start(only(&["127.0.0.1/32"], true), None)
            .await
            .expect("a proxy");
        let answer = ask(
            &proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\nthrough the tunnel"),
        )
        .await;
        assert_eq!(
            answer,
            "HTTP/1.1 200 Connection established\r\n\r\nfrom the destination\n"
        );
        assert_eq!(*seen.lock().expect("lock"), ["through the tunnel"]);
    }

    #[tokio::test]
    async fn a_plain_request_reaches_the_destination_without_the_proxys_part() {
        let (port, seen) = destination().await;
        let proxy = ForwardProxy::start(only(&["127.0.0.1/32"], true), None)
            .await
            .expect("a proxy");
        let answer = ask(
            &proxy,
            &format!(
                "GET http://127.0.0.1:{port}/page HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
                 Proxy-Connection: keep-alive\r\n\r\n"
            ),
        )
        .await;
        assert_eq!(answer, "from the destination\n");
        assert_eq!(
            *seen.lock().expect("lock"),
            [format!(
                "GET /page HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            )]
        );
    }

    /// What is not allowed gets a 403 that says so, the destination hears
    /// nothing, and whoever is watching is told each one.
    #[tokio::test]
    async fn what_the_policy_does_not_allow_is_refused_and_reported() {
        let (port, seen) = destination().await;
        let decisions = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&decisions);
        let observer: Observer = Arc::new(move |decision: &Decision| {
            record.lock().expect("lock").push(decision.clone());
        });
        // One name allowed, and loopback not opened.
        let proxy = ForwardProxy::start(only(&["localhost"], false), Some(observer))
            .await
            .expect("a proxy");

        // An address nobody allowed, and a reserved one at that.
        let by_address = ask(
            &proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n"),
        )
        .await;
        assert!(
            by_address.starts_with("HTTP/1.1 403 Forbidden\r\n"),
            "{by_address}"
        );
        // A name nobody allowed: refused before it is looked up.
        let by_name = ask(&proxy, "CONNECT not-allowed.invalid:443 HTTP/1.1\r\n\r\n").await;
        assert!(
            by_name.starts_with("HTTP/1.1 403 Forbidden\r\n"),
            "{by_name}"
        );
        assert!(
            by_name.contains("not-allowed.invalid is not an allowed host"),
            "{by_name}"
        );
        // An allowed name that resolves to this machine's loopback: the name
        // does not open a reserved address.
        let rebound = ask(
            &proxy,
            &format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n"),
        )
        .await;
        assert!(
            rebound.starts_with("HTTP/1.1 403 Forbidden\r\n"),
            "{rebound}"
        );
        assert!(rebound.contains("reserved address"), "{rebound}");
        // The same over a plain request.
        let plain = ask(
            &proxy,
            &format!("GET http://127.0.0.1:{port}/ HTTP/1.1\r\n\r\n"),
        )
        .await;
        assert!(plain.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{plain}");
        // And what is not a request at all.
        let nonsense = ask(&proxy, "nonsense\r\n\r\n").await;
        assert!(
            nonsense.starts_with("HTTP/1.1 400 Bad Request\r\n"),
            "{nonsense}"
        );

        assert!(
            seen.lock().expect("lock").is_empty(),
            "the destination was reached"
        );
        let decisions = decisions.lock().expect("lock");
        let told: Vec<(&str, bool, &str)> = decisions
            .iter()
            .map(|d| (d.host.as_str(), d.allowed, d.reason))
            .collect();
        assert_eq!(
            told,
            [
                ("127.0.0.1", false, "reserved address"),
                ("not-allowed.invalid", false, "name not allowed"),
                ("localhost", false, "reserved address"),
                ("127.0.0.1", false, "reserved address"),
            ]
        );
    }

    #[tokio::test]
    async fn a_head_that_never_ends_is_refused_at_a_size() {
        let proxy = ForwardProxy::start(only(&[], false), None)
            .await
            .expect("a proxy");
        let endless = format!(
            "GET http://example.com/ HTTP/1.1\r\nX: {}",
            "a".repeat(MAX_HEAD * 2)
        );
        let answer = ask(&proxy, &endless).await;
        assert!(answer.starts_with("HTTP/1.1 431 "), "{answer}");
    }
}
