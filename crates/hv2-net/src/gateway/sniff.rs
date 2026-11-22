//! What name a client asked for, read from the first bytes it sent.
//!
//! Two places carry it in the clear: the `server_name` extension of a TLS
//! ClientHello, and the `Host` header of an HTTP/1 request. Nothing else is
//! attempted. A client that speaks some other protocol, or TLS without SNI,
//! has not named anything, and the gateway treats it as such.

/// How much a client may send before the gateway gives up looking for a name.
/// A ClientHello with post-quantum key shares runs past 1.5 KiB; 16 KiB is one
/// full TLS record, which is as long as a ClientHello can be in one record.
pub const SNIFF_LIMIT: usize = 16 * 1024 + 5;

/// What the first bytes turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sniffed {
    /// A TLS ClientHello, and the name in it if there was one.
    Tls(Option<String>),
    /// An HTTP/1 request, and its `Host` without the port.
    Http(Option<String>),
    /// Neither.
    Other,
    /// Could be either; more bytes are needed to tell.
    Incomplete,
}

/// Look at what a client has sent so far.
#[must_use]
pub fn sniff(bytes: &[u8]) -> Sniffed {
    if bytes.is_empty() {
        return Sniffed::Incomplete;
    }
    match bytes[0] {
        0x16 => sniff_tls(bytes),
        // Another TLS record first (some servers skip a warning alert before
        // the ClientHello): TLS, and whatever name follows is not looked for.
        0x14 | 0x15 | 0x17 | 0x18 if bytes.len() < 2 => Sniffed::Incomplete,
        0x14 | 0x15 | 0x17 | 0x18 if bytes[1] == 3 => Sniffed::Tls(None),
        _ => sniff_http(bytes),
    }
}

/// A ClientHello may be split over several records, so the handshake message
/// is put back together before it is read: a server does the same.
fn sniff_tls(bytes: &[u8]) -> Sniffed {
    let unfinished = if bytes.len() >= SNIFF_LIMIT {
        Sniffed::Tls(None)
    } else {
        Sniffed::Incomplete
    };
    let mut handshake = Vec::new();
    let mut rest = bytes;
    loop {
        // Record header: type, version (2), length (2).
        if rest.is_empty() {
            return unfinished;
        }
        if rest[0] != 0x16 {
            return Sniffed::Tls(None);
        }
        if rest.len() < 5 {
            return unfinished;
        }
        let record_len = usize::from(u16::from_be_bytes([rest[3], rest[4]]));
        if record_len == 0 {
            return Sniffed::Tls(None);
        }
        if rest.len() < 5 + record_len {
            return unfinished;
        }
        handshake.extend_from_slice(&rest[5..5 + record_len]);
        rest = &rest[5 + record_len..];
        if handshake[0] != 1 {
            return Sniffed::Tls(None); // not a ClientHello
        }
        if let [_, a, b, c, ..] = handshake[..] {
            let message_len = 4 + usize::from(a) * 65536 + usize::from(b) * 256 + usize::from(c);
            if handshake.len() >= message_len {
                return Sniffed::Tls(client_hello_sni(&handshake));
            }
        }
    }
}

/// The SNI host name from a handshake message, or `None` if there is none or
/// the message is not a well-formed ClientHello.
fn client_hello_sni(handshake: &[u8]) -> Option<String> {
    let mut r = Reader(handshake);
    if r.u8()? != 1 {
        return None; // not a ClientHello
    }
    let body_len = r.u24()?;
    let mut body = Reader(r.take(body_len)?);
    body.take(2)?; // legacy_version
    body.take(32)?; // random
    let session = usize::from(body.u8()?);
    body.take(session)?;
    let suites = usize::from(body.u16()?);
    body.take(suites)?;
    let compression = usize::from(body.u8()?);
    body.take(compression)?;
    let extensions_len = usize::from(body.u16()?);
    let mut extensions = Reader(body.take(extensions_len)?);
    while !extensions.0.is_empty() {
        let kind = extensions.u16()?;
        let len = usize::from(extensions.u16()?);
        let data = extensions.take(len)?;
        if kind != 0 {
            continue;
        }
        let mut list = Reader(data);
        let list_len = usize::from(list.u16()?);
        let mut names = Reader(list.take(list_len)?);
        while !names.0.is_empty() {
            let name_type = names.u8()?;
            let name_len = usize::from(names.u16()?);
            let name = names.take(name_len)?;
            if name_type == 0 {
                return host_name(name);
            }
        }
        return None;
    }
    None
}

const METHODS: [&[u8]; 9] = [
    b"GET ",
    b"POST ",
    b"HEAD ",
    b"PUT ",
    b"DELETE ",
    b"OPTIONS ",
    b"PATCH ",
    b"CONNECT ",
    b"TRACE ",
];

/// HTTP/1, and the one host a request names. Servers differ on requests that
/// are not plainly formed, so one that could be read two ways -- two `Host`s,
/// a target naming another host, a folded or unterminated line -- has no name.
fn sniff_http(bytes: &[u8]) -> Sniffed {
    let unfinished = if bytes.len() >= SNIFF_LIMIT {
        Sniffed::Http(None)
    } else {
        Sniffed::Incomplete
    };
    // A server skips blank lines before the request line.
    let Some(start) = bytes.iter().position(|b| !matches!(b, b'\r' | b'\n')) else {
        return unfinished;
    };
    let bytes = &bytes[start..];
    let line_end = bytes.iter().position(|b| *b == b'\n');
    let line = &bytes[..line_end.unwrap_or(bytes.len())];
    let method = line.split(|b| *b == b' ').next().unwrap_or_default();
    let method = method.strip_suffix(b"\r").unwrap_or(method);
    if !method.iter().all(|b| is_token(*b)) || line.iter().any(|b| is_control(*b)) {
        return Sniffed::Other;
    }
    if line_end.is_none() {
        // Wait for the rest only where it could be a method as servers take
        // them, in capitals: other text protocols need not end a line.
        let could_be_method = method
            .iter()
            .all(|b| b.is_ascii_uppercase() || matches!(b, b'-' | b'_'));
        return if could_be_method {
            unfinished
        } else {
            Sniffed::Other
        };
    }
    let names_http = line.windows(5).any(|w| w.eq_ignore_ascii_case(b"HTTP/"));
    if !names_http && !METHODS.iter().any(|m| line.starts_with(m)) {
        return Sniffed::Other;
    }
    let mut lines = Vec::new();
    let mut rest = bytes;
    loop {
        let Some(end) = rest.iter().position(|b| *b == b'\n') else {
            return unfinished;
        };
        let line = &rest[..end];
        rest = &rest[end + 1..];
        if matches!(line, b"" | b"\r") {
            break;
        }
        lines.push(line);
    }
    Sniffed::Http(http_host(&lines))
}

/// The host a request head names, if it names exactly one.
fn http_host(lines: &[&[u8]]) -> Option<String> {
    let mut lines = lines.iter().map(|line| line.strip_suffix(b"\r"));
    let request: Vec<&[u8]> = lines.next()??.split(|b| *b == b' ').collect();
    let [method, target, version] = request[..] else {
        return None;
    };
    if method.is_empty() || !version.starts_with(b"HTTP/1.") {
        return None;
    }
    let mut host = None;
    for line in lines {
        let line = line?; // a line ended by a bare LF
        let colon = line.iter().position(|b| *b == b':')?;
        // Also refuses a folded line and a space before the colon.
        if colon == 0 || !line[..colon].iter().all(|b| is_token(*b)) || line.contains(&b'\r') {
            return None;
        }
        if line[..colon].eq_ignore_ascii_case(b"host") {
            if host.is_some() {
                return None;
            }
            host = Some(authority_host(&line[colon + 1..])?);
        }
    }
    let host = host?;
    if method != b"CONNECT" && (target.starts_with(b"/") || target == b"*") {
        return Some(host);
    }
    // Absolute form, or CONNECT's authority: a server goes by this one.
    let authority = match target.windows(3).position(|w| w == b"://") {
        Some(at) => &target[at + 3..],
        None if method == b"CONNECT" => target,
        None => return None,
    };
    let end = authority
        .iter()
        .position(|b| matches!(b, b'/' | b'?' | b'#' | b'\\'))
        .unwrap_or(authority.len());
    (authority_host(&authority[..end])? == host).then_some(host)
}

/// The host of `host[:port]`, minding that an IPv6 literal is bracketed.
fn authority_host(value: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(value).ok()?;
    let value = value.trim_matches([' ', '\t']);
    let host = if let Some(literal) = value.strip_prefix('[') {
        literal.split_once(']')?.0
    } else {
        value.split(':').next()?
    };
    host_name(host.as_bytes())
}

/// A byte of an HTTP method or header name.
fn is_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// A byte no text protocol's line holds.
fn is_control(b: u8) -> bool {
    (b < 0x20 && !matches!(b, b'\t' | b'\r')) || b == 0x7f
}

/// A host name as sent, if it is one: ASCII letters, digits, hyphens,
/// underscores and dots. Anything else is refused rather than normalised, since a name with
/// a NUL or a space in it is not a name any rule was written for.
fn host_name(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty()
        || bytes.len() > 253
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(crate::network_policy::normalise(
        std::str::from_utf8(bytes).ok()?,
    ))
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn u24(&mut self) -> Option<usize> {
        self.take(3)
            .map(|b| (usize::from(b[0]) << 16) | (usize::from(b[1]) << 8) | usize::from(b[2]))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ClientHello carrying `name`, laid out by hand so the test does not
    /// depend on the parser it is testing.
    pub(crate) fn client_hello(name: &str) -> Vec<u8> {
        let name = name.as_bytes();
        let mut sni = Vec::new();
        sni.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
        sni.push(0);
        sni.extend_from_slice(&(name.len() as u16).to_be_bytes());
        sni.extend_from_slice(name);

        let mut extensions = Vec::new();
        // An unrelated extension first, so the walk is exercised.
        extensions.extend_from_slice(&[0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]);
        extensions.extend_from_slice(&[0x00, 0x00]);
        extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&sni);

        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[7u8; 32]);
        body.push(0); // session id
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // one suite
        body.extend_from_slice(&[0x01, 0x00]); // null compression
        body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        body.extend_from_slice(&extensions);

        let mut handshake = vec![0x01];
        handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend_from_slice(&body);

        let mut record = vec![0x16, 0x03, 0x01];
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);
        record
    }

    #[test]
    fn reads_sni_from_a_client_hello() {
        assert_eq!(
            sniff(&client_hello("API.Example.com")),
            Sniffed::Tls(Some("api.example.com".into()))
        );
    }

    #[test]
    fn a_hello_split_across_segments_waits_for_the_rest() {
        let hello = client_hello("example.com");
        for cut in [1, 4, 5, 20, hello.len() - 1] {
            assert_eq!(sniff(&hello[..cut]), Sniffed::Incomplete, "cut at {cut}");
        }
    }

    #[test]
    fn reads_host_from_http_and_drops_the_port() {
        let req = b"GET / HTTP/1.1\r\nUser-Agent: x\r\nhOsT: Example.com:8080\r\n\r\n";
        assert_eq!(sniff(req), Sniffed::Http(Some("example.com".into())));
        assert_eq!(sniff(b"GET / HTTP/1.1\r\nHost: exa"), Sniffed::Incomplete);
        assert_eq!(sniff(b"GE"), Sniffed::Incomplete);
    }

    /// The same ClientHello cut into two records.
    pub(crate) fn fragmented_client_hello(name: &str, at: usize) -> Vec<u8> {
        let hello = client_hello(name);
        let mut out = Vec::new();
        for part in [&hello[5..5 + at], &hello[5 + at..]] {
            out.extend_from_slice(&[0x16, 0x03, 0x01]);
            out.extend_from_slice(&(part.len() as u16).to_be_bytes());
            out.extend_from_slice(part);
        }
        out
    }

    #[test]
    fn a_hello_in_several_records_is_read_whole() {
        for at in [1, 3, 4, 10, 60] {
            let hello = fragmented_client_hello("example.com", at);
            assert_eq!(
                sniff(&hello),
                Sniffed::Tls(Some("example.com".into())),
                "cut at {at}"
            );
            // The first record alone is not yet a name, nor the lack of one.
            assert_eq!(sniff(&hello[..5 + at]), Sniffed::Incomplete);
            assert_eq!(sniff(&hello[..hello.len() - 1]), Sniffed::Incomplete);
        }
        // An alert, an empty record, or another type between.
        assert_eq!(sniff(&[0x15, 3, 1, 0, 2, 1, 90]), Sniffed::Tls(None));
        assert_eq!(sniff(&[0x16, 3, 1, 0, 0]), Sniffed::Tls(None));
        assert_eq!(sniff(&[0x16, 3, 1, 0, 1, 1, 0x15]), Sniffed::Tls(None));
        // A message longer than the limit never finishes, and is not waited for.
        let mut long = vec![0x16, 3, 1, 0x40, 0x00, 1, 0xff, 0xff, 0xff];
        long.resize(SNIFF_LIMIT, 0);
        assert_eq!(sniff(&long), Sniffed::Tls(None));
    }

    /// Requests a server may read another host from carry no name.
    #[test]
    fn an_http_request_that_could_name_two_hosts_names_none() {
        for request in [
            "GET / HTTP/1.1\r\nHost: a.example\r\nHost: b.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: b!.example\r\nHost: a.example\r\n\r\n",
            "GET http://b.example/ HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "GET http://a.example@b.example/ HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "CONNECT b.example:443 HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "GET / HTTP/1.1\r\nX: y\r\n Host: b.example\r\nHost: a.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost : b.example\r\nHost: a.example\r\n\r\n",
            "GET / HTTP/1.1\nHost: a.example\n\n",
            "GET / HTTP/1.1\r\nHost: a.example\rHost: b.example\r\n\r\n",
            "GET  / HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "GET / HTTP/1.1\r\n\r\n",
            "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
        ] {
            assert_eq!(
                sniff(request.as_bytes()),
                Sniffed::Http(None),
                "{request:?}"
            );
        }
    }

    #[test]
    fn unusual_but_plain_http_is_still_read() {
        let named = Sniffed::Http(Some("a.example".into()));
        for request in [
            "\r\n\r\nGET / HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "PROPFIND / HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "GET http://a.example:80/x HTTP/1.1\r\nHost: a.example\r\n\r\n",
            "CONNECT a.example:443 HTTP/1.1\r\nHost: a.example:443\r\n\r\n",
            "OPTIONS * HTTP/1.1\r\nHost:a.example\r\n\r\n",
        ] {
            assert_eq!(sniff(request.as_bytes()), named, "{request:?}");
            for cut in 1..request.len() {
                assert_eq!(
                    sniff(&request.as_bytes()[..cut]),
                    Sniffed::Incomplete,
                    "{request:?} cut at {cut}"
                );
            }
        }
        assert_eq!(
            sniff(b"GET / HTTP/1.1\r\nHost: my_app.example\r\n\r\n"),
            Sniffed::Http(Some("my_app.example".into()))
        );
    }

    #[test]
    fn anything_else_is_other() {
        assert_eq!(sniff(b"EHLO client.example\r\n"), Sniffed::Other);
        assert_eq!(sniff(b"*1\r\n$4\r\nPING\r\n"), Sniffed::Other);
        assert_eq!(sniff(&[0x41, 0, 0, 0, 1]), Sniffed::Other);
        assert_eq!(sniff(b"hi"), Sniffed::Other);
        assert_eq!(sniff(b"SSH-2.0"), Sniffed::Other);
        assert_eq!(sniff(b"SSH-2.0-OpenSSH_9.6\r\n"), Sniffed::Other);
        assert_eq!(sniff(&[0x00, 0x01]), Sniffed::Other);
    }

    #[test]
    fn a_name_with_forbidden_bytes_is_no_name() {
        let req = b"GET / HTTP/1.1\r\nHost: evil.com\x00.example.com\r\n\r\n";
        assert_eq!(sniff(req), Sniffed::Http(None));
    }
}
