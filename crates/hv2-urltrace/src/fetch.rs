use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;

use crate::cli::Config;
use crate::hints;
use crate::http::{self, Response};
use crate::net::{remaining, DeadlineStream};
use crate::report::{cap, Report, RequestInfo, ResponseInfo, UrlInfo, CAP_LONG, CAP_SHORT};
use crate::tls;
use crate::url::ParsedUrl;

pub const MAX_ADDRS: usize = 16;
pub const BODY_SAMPLE_BYTES: usize = 2048;

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn resolve_host(
    url: &ParsedUrl,
    cfg: &Config,
    deadline: Instant,
) -> Result<Vec<SocketAddrV4>, String> {
    if let Some(ip) = cfg.resolve {
        return Ok(vec![SocketAddrV4::new(ip, url.port)]);
    }
    if let Ok(ip) = url.host.parse::<Ipv4Addr>() {
        return Ok(vec![SocketAddrV4::new(ip, url.port)]);
    }
    let wait = remaining(deadline)
        .map(|d| d.min(cfg.timeout))
        .ok_or("deadline exceeded before resolving")?;
    let (tx, rx) = mpsc::channel();
    let host = url.host.clone();
    let port = url.port;
    thread::Builder::new()
        .spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|it| it.collect::<Vec<SocketAddr>>())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        })
        .map_err(|e| format!("cannot start the resolver thread: {e}"))?;
    let all = match rx.recv_timeout(wait) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(format!("name resolution timed out after {} ms", ms(wait))),
    };
    let total = all.len();
    let mut v4: Vec<SocketAddrV4> = Vec::new();
    for a in all {
        if let SocketAddr::V4(a) = a {
            if !v4.contains(&a) && v4.len() < MAX_ADDRS {
                v4.push(a);
            }
        }
    }
    if v4.is_empty() {
        return Err(format!(
            "no IPv4 address ({total} address(es) returned in all)"
        ));
    }
    Ok(v4)
}

fn build_request(url: &ParsedUrl, user_agent: &str) -> (Vec<u8>, RequestInfo) {
    let request_line = format!("GET {} HTTP/1.1", url.path_and_query);
    let headers = vec![
        ("Host".to_owned(), url.host_header()),
        ("User-Agent".to_owned(), user_agent.to_owned()),
        ("Accept".to_owned(), "*/*".to_owned()),
        ("Accept-Encoding".to_owned(), "identity".to_owned()),
        ("Connection".to_owned(), "close".to_owned()),
    ];
    let mut wire = String::new();
    wire.push_str(&request_line);
    wire.push_str("\r\n");
    for (n, v) in &headers {
        wire.push_str(n);
        wire.push_str(": ");
        wire.push_str(v);
        wire.push_str("\r\n");
    }
    wire.push_str("\r\n");
    (
        wire.into_bytes(),
        RequestInfo {
            request_line: cap(&request_line, CAP_LONG),
            headers: headers
                .into_iter()
                .map(|(n, v)| (n, cap(&v, CAP_LONG)))
                .collect(),
        },
    )
}

fn exchange<S: Read + Write>(
    stream: &mut S,
    wire: &[u8],
    max_body: usize,
    rep: &mut Report,
) -> (Response, Instant) {
    let sent = Instant::now();
    let write = stream.write_all(wire).and_then(|()| stream.flush());
    if let Err(e) = write {
        rep.error("request", format!("sending the request: {e}"));
        return (Response::default(), sent);
    }
    (http::read_response(stream, max_body), sent)
}

fn lossy(bytes: &[u8], max: usize, lossy_seen: &mut bool) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => cap(s, max),
        Err(_) => {
            *lossy_seen = true;
            cap(&String::from_utf8_lossy(bytes), max)
        }
    }
}

fn describe_response(resp: &Response, url: &ParsedUrl, rep: &mut Report) -> Option<ResponseInfo> {
    let head = resp.head.as_ref()?;
    let mut lossy_seen = false;
    let headers = head
        .headers
        .iter()
        .map(|(n, v)| {
            (
                lossy(n, CAP_SHORT, &mut lossy_seen),
                lossy(v, CAP_LONG, &mut lossy_seen),
            )
        })
        .collect();
    let reason = lossy(&head.reason, CAP_SHORT, &mut lossy_seen);
    let location = head
        .header("location")
        .map(|v| lossy(v, CAP_LONG, &mut lossy_seen));
    let content_type = head
        .header("content-type")
        .map(|v| lossy(v, CAP_SHORT, &mut lossy_seen));
    let location_count = head
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(b"location"))
        .count();
    if location_count > 1 {
        rep.note(format!(
            "{location_count} Location headers; the first was used"
        ));
    }
    let next_url = match head.header("location") {
        Some(raw) => match url.resolve(&String::from_utf8_lossy(raw)) {
            Ok(next) => Some(cap(&next.to_url_string(), CAP_LONG)),
            Err(why) => {
                rep.note(format!("next_url is null: {why}"));
                None
            }
        },
        None => None,
    };
    let sample = resp
        .body
        .get(..BODY_SAMPLE_BYTES.min(resp.body.len()))
        .unwrap_or(&[]);
    Some(ResponseInfo {
        http_version: head.version.clone(),
        status: head.status,
        reason,
        headers,
        headers_truncated: resp.headers_truncated,
        lossy: lossy_seen,
        location,
        next_url,
        content_type,
        body_bytes_read: resp.body.len(),
        body_truncated: resp.body_truncated,
        body_sha256: tls::sha256_hex(&resp.body),
        body_sample_encoding: "base64",
        body_sample: base64::engine::general_purpose::STANDARD.encode(sample),
    })
}

pub fn run(cfg: &Config) -> Report {
    let start = Instant::now();
    let deadline = start + cfg.timeout;
    let mut rep = Report::new(&cfg.url);
    for c in &cfg.clamped {
        rep.note(c);
    }
    trace(cfg, deadline, &mut rep);
    rep.timings_ms.total = ms(start.elapsed());
    rep
}

fn trace(cfg: &Config, deadline: Instant, rep: &mut Report) {
    let url = match ParsedUrl::parse(&cfg.url) {
        Ok(u) => u,
        Err(e) => {
            rep.error("url", e);
            return;
        }
    };
    rep.url = Some(UrlInfo {
        scheme: url.scheme(),
        host: url.host.clone(),
        port: url.port,
        path_and_query: cap(&url.path_and_query, CAP_LONG),
    });

    let t = Instant::now();
    let addrs = resolve_host(&url, cfg, deadline);
    rep.timings_ms.resolve = Some(ms(t.elapsed()));
    let addrs = match addrs {
        Ok(a) => a,
        Err(e) => {
            rep.error("resolve", e);
            return;
        }
    };
    rep.resolved = addrs.iter().map(ToString::to_string).collect();

    let t = Instant::now();
    let mut tcp = None;
    let mut failures = Vec::new();
    for addr in &addrs {
        let Some(budget) = remaining(deadline).map(|d| d.min(cfg.timeout)) else {
            failures.push("deadline exceeded".to_owned());
            break;
        };
        match TcpStream::connect_timeout(&SocketAddr::V4(*addr), budget) {
            Ok(s) => {
                rep.connected_to = Some(addr.to_string());
                tcp = Some(s);
                break;
            }
            Err(e) => failures.push(format!("{addr}: {e}")),
        }
    }
    rep.timings_ms.connect = Some(ms(t.elapsed()));
    let Some(tcp) = tcp else {
        rep.error("connect", failures.join("; "));
        return;
    };
    if !failures.is_empty() {
        rep.note(format!(
            "earlier connect attempts failed: {}",
            failures.join("; ")
        ));
    }

    let (wire, request) = build_request(&url, &cfg.user_agent);
    rep.request = Some(request);
    let stream = DeadlineStream::new(tcp, deadline, cfg.timeout);

    let (resp, sent) = if url.https {
        let t = Instant::now();
        let hs = tls::connect(&url.host, stream);
        rep.timings_ms.tls = Some(ms(t.elapsed()));
        rep.tls = Some(hs.info);
        match hs.stream {
            Ok(mut s) => exchange(&mut s, &wire, cfg.max_body, rep),
            Err(e) => {
                rep.error("tls", e);
                return;
            }
        }
    } else {
        let mut s = stream;
        exchange(&mut s, &wire, cfg.max_body, rep)
    };

    rep.timings_ms.first_byte = resp
        .first_byte
        .map(|fb| ms(fb.saturating_duration_since(sent)));
    for n in &resp.notes {
        rep.note(n);
    }
    if let Some(e) = &resp.error {
        let stage = if resp.head.is_none() {
            "response_head"
        } else {
            "response_body"
        };
        rep.error(stage, e);
    }
    rep.response = describe_response(&resp, &url, rep);
    if !resp.body.is_empty() {
        rep.hints = hints::scan(&resp.body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    fn serve_once(canned: &'static [u8]) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut stream = stream;
            stream.write_all(canned).unwrap();
            request
        });
        (port, handle)
    }

    #[test]
    fn end_to_end_redirect_over_plain_http() {
        let (port, server) = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: /landing?src=1\r\nContent-Type: text/html\r\nContent-Length: 83\r\nConnection: close\r\n\r\n<meta http-equiv=\"refresh\" content=\"0;url=https://next.test/\">window.location='/js'",
        );
        let mut cfg = Config::new(&format!("http://redir.test:{port}/start?a=b"));
        cfg.resolve = Some(Ipv4Addr::LOCALHOST);
        let rep = run(&cfg);
        let request = server.join().unwrap();

        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        assert_eq!(rep.resolved, [format!("127.0.0.1:{port}")]);
        assert_eq!(
            rep.connected_to.as_deref(),
            Some(format!("127.0.0.1:{port}").as_str())
        );
        assert!(rep.tls.is_none());
        let r = rep.response.as_ref().unwrap();
        assert_eq!(
            (r.status, r.reason.as_str(), r.http_version.as_str()),
            (302, "Found", "HTTP/1.1")
        );
        assert_eq!(r.location.as_deref(), Some("/landing?src=1"));
        assert_eq!(
            r.next_url.as_deref(),
            Some(format!("http://redir.test:{port}/landing?src=1").as_str())
        );
        assert_eq!(r.content_type.as_deref(), Some("text/html"));
        assert_eq!(r.body_bytes_read, 83);
        assert!(!r.body_truncated);
        assert_eq!(r.body_sha256, tls::sha256_hex(b"<meta http-equiv=\"refresh\" content=\"0;url=https://next.test/\">window.location='/js'"));
        assert_eq!(rep.hints.len(), 2);
        assert_eq!(rep.hints[0].kind, "meta_refresh");
        assert_eq!(rep.hints[0].value, "https://next.test/");
        assert_eq!(rep.hints[1].kind, "script_window_location");
        assert!(rep.timings_ms.resolve.is_some() && rep.timings_ms.first_byte.is_some());

        let req = rep.request.as_ref().unwrap();
        assert_eq!(req.request_line, "GET /start?a=b HTTP/1.1");
        let names: Vec<&str> = req.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Host",
                "User-Agent",
                "Accept",
                "Accept-Encoding",
                "Connection"
            ]
        );
        assert_eq!(
            request,
            format!(
                "GET /start?a=b HTTP/1.1\r\nHost: redir.test:{port}\r\nUser-Agent: {}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
                cfg.user_agent
            )
        );

        let json = serde_json::to_string(&rep).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["response"]["status"], 302);
        assert_eq!(value["tool"]["name"], "hv2-urltrace");
        assert!(value["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn javascript_location_has_no_next_url() {
        let (port, server) = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: javascript:alert(1)\r\nContent-Length: 0\r\n\r\n",
        );
        let rep = run(&Config::new(&format!("http://127.0.0.1:{port}/")));
        server.join().unwrap();
        let r = rep.response.as_ref().unwrap();
        assert_eq!(r.location.as_deref(), Some("javascript:alert(1)"));
        assert!(r.next_url.is_none());
        assert!(rep.notes.iter().any(|n| n.contains("next_url is null")));
    }

    #[test]
    fn bad_url_and_refused_connection_are_reported_not_fatal() {
        let rep = run(&Config::new("ftp://x/"));
        assert_eq!(rep.errors[0].stage, "url");
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let rep = run(&Config::new(&format!("http://127.0.0.1:{port}/")));
        assert_eq!(rep.errors[0].stage, "connect");
        assert!(rep.response.is_none());
    }

    #[test]
    fn header_and_body_with_invalid_utf8_are_lossy() {
        let (port, server) =
            serve_once(b"HTTP/1.1 200 OK\r\nX-Bin: a\xffb\r\nContent-Length: 2\r\n\r\n\xff\xfe");
        let rep = run(&Config::new(&format!("http://127.0.0.1:{port}/")));
        server.join().unwrap();
        let r = rep.response.as_ref().unwrap();
        assert!(r.lossy);
        assert!(r.headers[0].1.contains('\u{fffd}'));
        assert_eq!(r.body_sample, "//4=");
    }
}
