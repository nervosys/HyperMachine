use serde::Serialize;

use crate::hints::Hint;

pub const CAP_SHORT: usize = 512;
pub const CAP_LONG: usize = 8192;

pub fn cap(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s.get(..end).unwrap_or("").to_owned();
    out.push_str("...[truncated]");
    out
}

#[derive(Debug, Serialize)]
pub struct ToolInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct StageError {
    pub stage: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct UrlInfo {
    pub scheme: &'static str,
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
}

#[derive(Debug, Serialize)]
pub struct RequestInfo {
    pub request_line: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct CertInfo {
    pub sha256: String,
    pub subject: Option<String>,
    pub issuer: Option<String>,
    pub serial: Option<String>,
    pub not_before: Option<i64>,
    pub not_after: Option<i64>,
    pub san_dns: Vec<String>,
    pub san_dns_truncated: bool,
    pub parse_error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct TlsInfo {
    pub protocol_version: Option<String>,
    pub cipher_suite: Option<String>,
    pub alpn: Option<String>,
    pub verified: Option<bool>,
    pub verification_error: Option<String>,
    pub certificates: Vec<CertInfo>,
    pub certificates_truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ResponseInfo {
    pub http_version: String,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub headers_truncated: bool,
    pub lossy: bool,
    pub location: Option<String>,
    pub next_url: Option<String>,
    pub content_type: Option<String>,
    pub body_bytes_read: usize,
    pub body_truncated: bool,
    pub body_sha256: String,
    pub body_sample_encoding: &'static str,
    pub body_sample: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Timings {
    pub resolve: Option<u64>,
    pub connect: Option<u64>,
    pub tls: Option<u64>,
    pub first_byte: Option<u64>,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub tool: ToolInfo,
    pub input_url: String,
    pub url: Option<UrlInfo>,
    pub resolved: Vec<String>,
    pub connected_to: Option<String>,
    pub tls: Option<TlsInfo>,
    pub request: Option<RequestInfo>,
    pub response: Option<ResponseInfo>,
    pub hints: Vec<Hint>,
    pub timings_ms: Timings,
    pub errors: Vec<StageError>,
    pub notes: Vec<String>,
}

impl Report {
    pub fn new(input_url: &str) -> Self {
        Self {
            tool: ToolInfo {
                name: env!("CARGO_PKG_NAME"),
                version: env!("CARGO_PKG_VERSION"),
            },
            input_url: cap(input_url, CAP_LONG),
            url: None,
            resolved: Vec::new(),
            connected_to: None,
            tls: None,
            request: None,
            response: None,
            hints: Vec::new(),
            timings_ms: Timings::default(),
            errors: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn error(&mut self, stage: &'static str, message: impl AsRef<str>) {
        self.errors.push(StageError {
            stage,
            message: cap(message.as_ref(), CAP_LONG),
        });
    }

    pub fn note(&mut self, message: impl AsRef<str>) {
        self.notes.push(cap(message.as_ref(), CAP_LONG));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_marks_truncation_on_a_char_boundary() {
        assert_eq!(cap("abc", 3), "abc");
        assert_eq!(cap("abcd", 3), "abc...[truncated]");
        let s = "a\u{e9}\u{e9}";
        assert_eq!(cap(s, 2), "a...[truncated]");
        assert_eq!(cap(s, 3), "a\u{e9}...[truncated]");
        assert_eq!(cap("\u{1F600}", 0), "...[truncated]");
    }
}
