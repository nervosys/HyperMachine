use std::io::{self, Read};
use std::time::Instant;

pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_HEADER_LINES: usize = 200;
pub const MAX_STATUS_LINE: usize = 8192;
pub const MAX_CHUNK_LINE: usize = 128;
pub const HARD_MAX_BODY: usize = 4 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Head {
    pub version: String,
    pub status: u16,
    pub reason: Vec<u8>,
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, v)| v.as_slice())
    }
}

#[derive(Debug, Default)]
pub struct Response {
    pub head: Option<Head>,
    pub headers_truncated: bool,
    pub body: Vec<u8>,
    pub body_truncated: bool,
    pub error: Option<String>,
    pub notes: Vec<String>,
    pub first_byte: Option<Instant>,
}

enum LineErr {
    Eof(bool),
    TooLong,
    Io(io::Error),
}

impl LineErr {
    fn describe(&self, what: &str, limit: usize) -> String {
        match self {
            LineErr::Eof(false) => format!("connection closed before the {what} began"),
            LineErr::Eof(true) => format!("connection closed inside the {what}"),
            LineErr::TooLong => format!("{what} line longer than {limit} bytes"),
            LineErr::Io(e) => format!("reading the {what}: {e}"),
        }
    }
}

struct Buf<'a, R: Read> {
    r: &'a mut R,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    unclean_eof: bool,
    first_byte: Option<Instant>,
}

impl<'a, R: Read> Buf<'a, R> {
    fn new(r: &'a mut R) -> Self {
        Self {
            r,
            buf: Vec::new(),
            pos: 0,
            eof: false,
            unclean_eof: false,
            first_byte: None,
        }
    }

    fn fill(&mut self) -> io::Result<()> {
        if self.pos < self.buf.len() || self.eof {
            return Ok(());
        }
        self.buf.clear();
        self.pos = 0;
        let mut tmp = [0u8; 8192];
        loop {
            match self.r.read(&mut tmp) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(());
                }
                Ok(n) => {
                    if self.first_byte.is_none() {
                        self.first_byte = Some(Instant::now());
                    }
                    self.buf.extend_from_slice(tmp.get(..n).unwrap_or(&[]));
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    self.unclean_eof = true;
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn read_line(&mut self, max: usize) -> Result<Vec<u8>, LineErr> {
        let mut line = Vec::new();
        loop {
            self.fill().map_err(LineErr::Io)?;
            if self.pos >= self.buf.len() {
                return Err(LineErr::Eof(!line.is_empty()));
            }
            let avail = self.buf.get(self.pos..).unwrap_or(&[]);
            match avail.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    if line.len() + i > max {
                        return Err(LineErr::TooLong);
                    }
                    line.extend_from_slice(avail.get(..i).unwrap_or(&[]));
                    self.pos += i + 1;
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    return Ok(line);
                }
                None => {
                    if line.len() + avail.len() > max {
                        return Err(LineErr::TooLong);
                    }
                    line.extend_from_slice(avail);
                    self.pos = self.buf.len();
                }
            }
        }
    }

    fn take(&mut self, out: &mut Vec<u8>, n: usize) -> io::Result<usize> {
        let mut got = 0;
        while got < n {
            self.fill()?;
            if self.pos >= self.buf.len() {
                break;
            }
            let k = (n - got).min(self.buf.len() - self.pos);
            out.extend_from_slice(self.buf.get(self.pos..self.pos + k).unwrap_or(&[]));
            self.pos += k;
            got += k;
        }
        Ok(got)
    }

    fn more_data(&mut self) -> bool {
        self.fill().is_ok() && self.pos < self.buf.len()
    }
}

fn is_token(name: &[u8]) -> bool {
    !name.is_empty()
        && name
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if *first == b' ' || *first == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if *last == b' ' || *last == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn parse_status_line(line: &[u8]) -> Result<(String, u16, Vec<u8>), String> {
    let malformed = || {
        format!(
            "malformed status line: {:?}",
            String::from_utf8_lossy(line.get(..line.len().min(64)).unwrap_or(&[]))
        )
    };
    let rest = line.strip_prefix(b"HTTP/1.").ok_or_else(malformed)?;
    let minor = *rest.first().ok_or_else(malformed)?;
    if !minor.is_ascii_digit() || rest.get(1) != Some(&b' ') {
        return Err(malformed());
    }
    let code = rest.get(2..5).ok_or_else(malformed)?;
    if !code.iter().all(u8::is_ascii_digit) {
        return Err(malformed());
    }
    let status = code
        .iter()
        .fold(0u16, |acc, d| acc * 10 + u16::from(d - b'0'));
    if !(100..=599).contains(&status) {
        return Err(malformed());
    }
    let reason = match rest.get(5) {
        None => Vec::new(),
        Some(b' ') => rest.get(6..).unwrap_or(&[]).to_vec(),
        Some(_) => return Err(malformed()),
    };
    Ok((format!("HTTP/1.{}", char::from(minor)), status, reason))
}

fn read_head<R: Read>(b: &mut Buf<'_, R>, resp: &mut Response) {
    let line = match b.read_line(MAX_STATUS_LINE) {
        Ok(l) => l,
        Err(e) => {
            resp.error = Some(e.describe("status line", MAX_STATUS_LINE));
            return;
        }
    };
    let (version, status, reason) = match parse_status_line(&line) {
        Ok(x) => x,
        Err(m) => {
            resp.error = Some(m);
            return;
        }
    };
    let mut head = Head {
        version,
        status,
        reason,
        headers: Vec::new(),
    };
    let mut total = line.len() + 2;
    loop {
        let line = match b.read_line(MAX_HEADER_BYTES) {
            Ok(l) => l,
            Err(e) => {
                if matches!(e, LineErr::TooLong) {
                    resp.headers_truncated = true;
                }
                resp.error = Some(e.describe("header section", MAX_HEADER_BYTES));
                break;
            }
        };
        if line.is_empty() {
            break;
        }
        total += line.len() + 2;
        if total > MAX_HEADER_BYTES || head.headers.len() >= MAX_HEADER_LINES {
            resp.headers_truncated = true;
            resp.error = Some("header section exceeds its limits; body not read".into());
            break;
        }
        if line.first() == Some(&b' ') || line.first() == Some(&b'\t') {
            if let Some(last) = head.headers.last_mut() {
                last.1.push(b' ');
                last.1.extend_from_slice(trim(&line));
            } else {
                resp.notes
                    .push("skipped a continuation line with no header".into());
            }
            continue;
        }
        match line.iter().position(|c| *c == b':') {
            Some(i) if is_token(line.get(..i).unwrap_or(&[])) => {
                head.headers.push((
                    line.get(..i).unwrap_or(&[]).to_vec(),
                    trim(line.get(i + 1..).unwrap_or(&[])).to_vec(),
                ));
            }
            _ => resp.notes.push("skipped a malformed header line".into()),
        }
    }
    resp.head = Some(head);
}

enum Framing {
    NoBody,
    Length(u64),
    Chunked,
    UntilClose,
}

fn framing(head: &Head, notes: &mut Vec<String>) -> Framing {
    if head.status == 204 || head.status == 304 || head.status < 200 {
        return Framing::NoBody;
    }
    let te: Vec<&[u8]> = head
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(b"transfer-encoding"))
        .map(|(_, v)| v.as_slice())
        .collect();
    if let Some(last) = te.last() {
        let final_coding = last.rsplit(|c| *c == b',').next().map(trim).unwrap_or(&[]);
        if final_coding.eq_ignore_ascii_case(b"chunked") {
            return Framing::Chunked;
        }
        notes.push("unsupported Transfer-Encoding; reading until the connection closes".into());
        return Framing::UntilClose;
    }
    let mut length: Option<u64> = None;
    for (n, v) in &head.headers {
        if !n.eq_ignore_ascii_case(b"content-length") {
            continue;
        }
        let v = trim(v);
        let parsed = if v.is_empty() || v.len() > 19 || !v.iter().all(u8::is_ascii_digit) {
            None
        } else {
            std::str::from_utf8(v)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
        };
        match (parsed, length) {
            (Some(n), None) => length = Some(n),
            (Some(n), Some(prev)) if n == prev => {}
            _ => {
                notes.push(
                    "invalid or conflicting Content-Length; reading until the connection closes"
                        .into(),
                );
                return Framing::UntilClose;
            }
        }
    }
    match length {
        Some(n) => Framing::Length(n),
        None => Framing::UntilClose,
    }
}

fn parse_chunk_size(line: &[u8]) -> Result<u64, String> {
    let size_part = line.split(|c| *c == b';').next().map(trim).unwrap_or(&[]);
    if size_part.is_empty() || size_part.len() > 16 || !size_part.iter().all(u8::is_ascii_hexdigit)
    {
        return Err(format!(
            "chunk size is not hexadecimal: {:?}",
            String::from_utf8_lossy(line.get(..line.len().min(32)).unwrap_or(&[]))
        ));
    }
    std::str::from_utf8(size_part)
        .ok()
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        .ok_or_else(|| "chunk size is not hexadecimal".to_owned())
}

fn read_chunked<R: Read>(b: &mut Buf<'_, R>, resp: &mut Response, max: usize) {
    loop {
        let line = match b.read_line(MAX_CHUNK_LINE) {
            Ok(l) => l,
            Err(e) => {
                resp.error = Some(e.describe("chunk size", MAX_CHUNK_LINE));
                return;
            }
        };
        let size = match parse_chunk_size(&line) {
            Ok(s) => s,
            Err(m) => {
                resp.error = Some(m);
                return;
            }
        };
        if size == 0 {
            for _ in 0..MAX_HEADER_LINES {
                match b.read_line(MAX_STATUS_LINE) {
                    Ok(l) if l.is_empty() => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            return;
        }
        if resp.body.len() >= max {
            resp.body_truncated = true;
            return;
        }
        let room = u64::try_from(max - resp.body.len()).unwrap_or(u64::MAX);
        let want = usize::try_from(size.min(room)).unwrap_or(0);
        match b.take(&mut resp.body, want) {
            Ok(got) if got == want => {}
            Ok(got) => {
                resp.error = Some(format!(
                    "connection closed {got} bytes into a chunk of {want} bytes"
                ));
                return;
            }
            Err(e) => {
                resp.error = Some(format!("reading the body: {e}"));
                return;
            }
        }
        if size > room {
            resp.body_truncated = true;
            return;
        }
        let mut crlf = Vec::with_capacity(2);
        match b.take(&mut crlf, 2) {
            Ok(2) if crlf == b"\r\n" => {}
            Ok(_) => {
                resp.error = Some("chunk data is not followed by CRLF".into());
                return;
            }
            Err(e) => {
                resp.error = Some(format!("reading the body: {e}"));
                return;
            }
        }
    }
}

fn read_body<R: Read>(b: &mut Buf<'_, R>, resp: &mut Response, max: usize) {
    let mut notes = Vec::new();
    let framing = match &resp.head {
        Some(h) => framing(h, &mut notes),
        None => return,
    };
    resp.notes.extend(notes);
    match framing {
        Framing::NoBody => {}
        Framing::Length(n) => {
            let want = usize::try_from(n).unwrap_or(usize::MAX).min(max);
            match b.take(&mut resp.body, want) {
                Ok(got) if got == want => {
                    if u64::try_from(want).unwrap_or(u64::MAX) < n {
                        resp.body_truncated = true;
                    }
                }
                Ok(got) => {
                    resp.error = Some(format!("connection closed after {got} of {n} body bytes"));
                }
                Err(e) => resp.error = Some(format!("reading the body: {e}")),
            }
        }
        Framing::Chunked => read_chunked(b, resp, max),
        Framing::UntilClose => match b.take(&mut resp.body, max) {
            Ok(got) => {
                if got == max && b.more_data() {
                    resp.body_truncated = true;
                }
            }
            Err(e) => resp.error = Some(format!("reading the body: {e}")),
        },
    }
}

pub fn read_response<R: Read>(r: &mut R, max_body: usize) -> Response {
    let max_body = max_body.min(HARD_MAX_BODY);
    let mut resp = Response::default();
    let mut b = Buf::new(r);
    read_head(&mut b, &mut resp);
    if resp.head.is_some() && resp.error.is_none() {
        read_body(&mut b, &mut resp, max_body);
    }
    resp.first_byte = b.first_byte;
    if b.unclean_eof {
        resp.notes
            .push("the connection ended without a clean shutdown (no TLS close_notify)".into());
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bytes: &[u8], max: usize) -> Response {
        let mut r = bytes;
        read_response(&mut r, max)
    }

    #[test]
    fn content_length_body() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-A: b\r\n\r\nhelloEXTRA",
            100,
        );
        assert!(r.error.is_none());
        let h = r.head.unwrap();
        assert_eq!((h.version.as_str(), h.status), ("HTTP/1.1", 200));
        assert_eq!(h.reason, b"OK");
        assert_eq!(h.headers[1], (b"X-A".to_vec(), b"b".to_vec()));
        assert_eq!(r.body, b"hello");
        assert!(!r.body_truncated);
    }

    #[test]
    fn content_length_over_cap_is_truncated() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123456789",
            4,
        );
        assert!(r.error.is_none());
        assert_eq!(r.body, b"0123");
        assert!(r.body_truncated);
    }

    #[test]
    fn content_length_short_read_is_an_error() {
        let r = run(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123", 100);
        assert_eq!(r.body, b"0123");
        assert!(r.error.unwrap().contains("4 of 10"));
    }

    #[test]
    fn chunked_body_with_extension_and_trailer() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\nA\r\n world!!!!\r\n0\r\nTrailer: x\r\n\r\n",
            100,
        );
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.body, b"hello world!!!!");
        assert!(!r.body_truncated);
    }

    #[test]
    fn chunked_body_over_cap() {
        let r = run(
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: gzip, Chunked\r\n\r\n6\r\nabcdef\r\n6\r\nghijkl\r\n0\r\n\r\n",
            8,
        );
        assert_eq!(r.body, b"abcdefgh");
        assert!(r.body_truncated);
        assert!(r.error.is_none());
    }

    #[test]
    fn chunked_exactly_at_cap_is_not_truncated() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcd\r\n0\r\n\r\n",
            4,
        );
        assert_eq!(r.body, b"abcd");
        assert!(!r.body_truncated);
    }

    #[test]
    fn chunk_size_not_hex() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nabc\r\n0\r\n\r\n",
            100,
        );
        assert!(r.error.unwrap().contains("not hexadecimal"));
        assert!(r.body.is_empty());
    }

    #[test]
    fn chunk_size_line_too_long() {
        let mut v = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        v.extend(std::iter::repeat_n(b'1', 500));
        v.extend_from_slice(b"\r\n");
        let r = run(&v, 100);
        assert!(r.error.unwrap().contains("longer than"));
    }

    #[test]
    fn chunk_size_overflow_and_bad_terminator() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFFF\r\n",
            100,
        );
        assert!(r.error.is_some());
        let r = run(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcXX0\r\n\r\n",
            100,
        );
        assert!(r.error.unwrap().contains("CRLF"));
    }

    #[test]
    fn read_to_close() {
        let r = run(b"HTTP/1.0 200 OK\r\n\r\nstream of bytes", 100);
        assert!(r.error.is_none());
        assert_eq!(r.body, b"stream of bytes");
        assert!(!r.body_truncated);
        let r = run(b"HTTP/1.0 200 OK\r\n\r\n0123456789", 4);
        assert_eq!(r.body, b"0123");
        assert!(r.body_truncated);
        let r = run(b"HTTP/1.0 200 OK\r\n\r\n0123", 4);
        assert!(!r.body_truncated);
    }

    #[test]
    fn conflicting_content_length_reads_to_close() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\nabcdef",
            100,
        );
        assert_eq!(r.body, b"abcdef");
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn redirect_without_body() {
        let r = run(b"HTTP/1.1 302 Found\r\nLocation: /x\r\n\r\n", 100);
        assert!(r.error.is_none());
        assert_eq!(r.head.unwrap().header("location"), Some(&b"/x"[..]));
        let r = run(b"HTTP/1.1 204 No Content\r\nContent-Length: 9\r\n\r\n", 100);
        assert!(r.error.is_none());
    }

    #[test]
    fn header_overflow_by_count() {
        let mut v = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..300 {
            v.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
        }
        v.extend_from_slice(b"\r\nbody");
        let r = run(&v, 100);
        assert!(r.headers_truncated);
        assert_eq!(r.head.unwrap().headers.len(), MAX_HEADER_LINES);
        assert!(r.error.is_some());
        assert!(r.body.is_empty());
    }

    #[test]
    fn header_overflow_by_bytes() {
        let mut v = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..100 {
            v.extend_from_slice(format!("X-{i}: {}\r\n", "a".repeat(1000)).as_bytes());
        }
        v.extend_from_slice(b"\r\n");
        let r = run(&v, 100);
        assert!(r.headers_truncated);
        assert!(r.head.unwrap().headers.len() < 100);
    }

    #[test]
    fn single_header_line_too_long() {
        let mut v = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        v.extend(std::iter::repeat_n(b'a', MAX_HEADER_BYTES + 10));
        v.extend_from_slice(b"\r\n\r\n");
        let r = run(&v, 100);
        assert!(r.headers_truncated);
        assert!(r.error.is_some());
    }

    #[test]
    fn malformed_status_lines() {
        for bad in [
            &b"garbage\r\n\r\n"[..],
            b"HTTP/2 200 OK\r\n\r\n",
            b"HTTP/1.1 20 OK\r\n\r\n",
            b"HTTP/1.1 2000 OK\r\n\r\n",
            b"HTTP/1.1 abc OK\r\n\r\n",
            b"HTTP/1.1 099 Low\r\n\r\n",
            b"HTTP/1.1 600 High\r\n\r\n",
            b"HTTP/1.1\r\n\r\n",
            b"HTTP/1.x 200 OK\r\n\r\n",
            b"\r\n",
        ] {
            let r = run(bad, 100);
            assert!(r.error.is_some(), "{:?}", String::from_utf8_lossy(bad));
            assert!(r.head.is_none());
        }
        let r = run(b"HTTP/1.1 200\r\n\r\n", 100);
        assert!(r.error.is_none());
        assert!(r.head.unwrap().reason.is_empty());
    }

    #[test]
    fn early_eof() {
        let r = run(b"", 100);
        assert!(r.error.unwrap().contains("before the status line"));
        let r = run(b"HTTP/1.1 200 OK\r\nX-Part", 100);
        assert!(r.error.unwrap().contains("inside the header section"));
        let r = run(b"HTTP/1.1 200 OK\r\nX-A: b\r\n", 100);
        assert!(r.error.is_some());
        assert_eq!(r.head.unwrap().headers.len(), 1);
    }

    #[test]
    fn invalid_utf8_header_is_kept_as_bytes() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nX-Bin: a\xff\xfeb\r\nContent-Length: 0\r\n\r\n",
            100,
        );
        assert!(r.error.is_none());
        let h = r.head.unwrap();
        assert_eq!(h.header("x-bin"), Some(&b"a\xff\xfeb"[..]));
    }

    #[test]
    fn folding_and_malformed_header_lines() {
        let r = run(
            b"HTTP/1.1 200 OK\r\nA: one\r\n  two\r\nno colon here\r\n: empty name\r\nContent-Length: 0\r\n\r\n",
            100,
        );
        let h = r.head.unwrap();
        assert_eq!(h.header("a"), Some(&b"one two"[..]));
        assert_eq!(r.notes.len(), 2);
    }

    #[test]
    fn bare_lf_line_endings_are_tolerated() {
        let r = run(b"HTTP/1.1 200 OK\nContent-Length: 2\n\nhi", 100);
        assert!(r.error.is_none());
        assert_eq!(r.body, b"hi");
    }

    struct Failing(usize);

    impl Read for Failing {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let data = b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\npart";
            if self.0 == 0 {
                self.0 = 1;
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(n)
            } else {
                Err(io::Error::new(io::ErrorKind::TimedOut, "read timed out"))
            }
        }
    }

    #[test]
    fn io_error_mid_body_keeps_partial_body() {
        let r = read_response(&mut Failing(0), 100);
        assert_eq!(r.body, b"part");
        assert!(r.error.unwrap().contains("timed out"));
    }

    #[test]
    fn hard_cap_applies() {
        let r = run(b"HTTP/1.0 200 OK\r\n\r\nabc", usize::MAX);
        assert_eq!(r.body, b"abc");
    }

    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn random_input_never_panics() {
        let template: &[u8] =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX: y\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        for round in 0..600 {
            let mut bytes = Vec::new();
            if round % 2 == 0 {
                let len = (xorshift(&mut s) % 600) as usize;
                for _ in 0..len {
                    bytes.push((xorshift(&mut s) & 0xff) as u8);
                }
            } else {
                bytes.extend_from_slice(template);
                for _ in 0..(xorshift(&mut s) % 6) {
                    let i = (xorshift(&mut s) as usize) % bytes.len();
                    bytes[i] = (xorshift(&mut s) & 0xff) as u8;
                }
                let cut = (xorshift(&mut s) as usize) % (bytes.len() + 1);
                bytes.truncate(cut);
            }
            let max = (xorshift(&mut s) % 16) as usize;
            let r = run(&bytes, max);
            assert!(r.body.len() <= max);
        }
    }
}
