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
    if bytes[0] == 0x16 {
        return sniff_tls(bytes);
    }
    sniff_http(bytes)
}

fn sniff_tls(bytes: &[u8]) -> Sniffed {
    // Record header: type, version (2), length (2).
    if bytes.len() < 5 {
        return Sniffed::Incomplete;
    }
    let record_len = usize::from(u16::from_be_bytes([bytes[3], bytes[4]]));
    if bytes.len() < 5 + record_len {
        return if bytes.len() >= SNIFF_LIMIT {
            Sniffed::Tls(None)
        } else {
            Sniffed::Incomplete
        };
    }
    let record = &bytes[5..5 + record_len];
    Sniffed::Tls(client_hello_sni(record))
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

fn sniff_http(bytes: &[u8]) -> Sniffed {
    let could_be_method = METHODS.iter().any(|m| {
        let n = m.len().min(bytes.len());
        m[..n] == bytes[..n]
    });
    if !could_be_method {
        return Sniffed::Other;
    }
    if !METHODS.iter().any(|m| bytes.starts_with(m)) {
        return Sniffed::Incomplete;
    }
    let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if bytes.len() >= SNIFF_LIMIT {
            Sniffed::Http(None)
        } else {
            Sniffed::Incomplete
        };
    };
    let head = &bytes[..end];
    let host = head.split(|b| *b == b'\n').skip(1).find_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let colon = line.iter().position(|b| *b == b':')?;
        if !line[..colon].eq_ignore_ascii_case(b"host") {
            return None;
        }
        let value = std::str::from_utf8(&line[colon + 1..]).ok()?.trim();
        // Strip a port, minding that an IPv6 literal is bracketed.
        let without_port = if value.starts_with('[') {
            value.split(']').next().map(|v| v.trim_start_matches('['))
        } else {
            value.split(':').next()
        }?;
        host_name(without_port.as_bytes())
    });
    Sniffed::Http(host)
}

/// A host name as sent, if it is one: ASCII letters, digits, hyphens and
/// dots. Anything else is refused rather than normalised, since a name with
/// a NUL or a space in it is not a name any rule was written for.
fn host_name(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty()
        || bytes.len() > 253
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'.' || *b == b':')
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

    #[test]
    fn anything_else_is_other() {
        assert_eq!(sniff(b"SSH-2.0-OpenSSH_9.6\r\n"), Sniffed::Other);
        assert_eq!(sniff(&[0x00, 0x01]), Sniffed::Other);
    }

    #[test]
    fn a_name_with_forbidden_bytes_is_no_name() {
        let req = b"GET / HTTP/1.1\r\nHost: evil.com\x00.example.com\r\n\r\n";
        assert_eq!(sniff(req), Sniffed::Http(None));
    }
}
