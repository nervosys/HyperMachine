use std::net::Ipv4Addr;

pub const MAX_URL_LEN: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

fn validate_host(host: &str) -> Result<String, String> {
    if host.is_empty() {
        return Err("empty host".into());
    }
    let host = host.to_ascii_lowercase();
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return match host.parse::<Ipv4Addr>() {
            Ok(_) => Ok(host),
            Err(_) => Err(format!("{host:?} is not a valid IPv4 literal")),
        };
    }
    if host.len() > 253 {
        return Err("host name longer than 253 bytes".into());
    }
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("host has an empty or overlong label".into());
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("host contains a character that is not allowed in a DNS name".into());
        }
    }
    Ok(host)
}

fn scheme_of(s: &str) -> Option<&str> {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return None,
    }
    for (i, c) in chars {
        if c == ':' {
            return s.get(..i);
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return None;
        }
    }
    None
}

fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.trim().bytes() {
        if b <= 0x20 || b >= 0x7f {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(char::from(b));
        }
    }
    out
}

fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut trailing = false;
    let mut segments = path.split('/');
    segments.next();
    for seg in segments {
        match seg {
            "." => trailing = true,
            ".." => {
                out.pop();
                trailing = true;
            }
            s => {
                out.push(s);
                trailing = false;
            }
        }
    }
    let mut result = String::from("/");
    result.push_str(&out.join("/"));
    if trailing && !out.is_empty() {
        result.push('/');
    }
    result
}

impl ParsedUrl {
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            return Err("empty URL".into());
        }
        if input.len() > MAX_URL_LEN {
            return Err(format!(
                "URL is {} bytes, the limit is {MAX_URL_LEN}",
                input.len()
            ));
        }
        if let Some(b) = input.bytes().find(|b| *b <= 0x20 || *b >= 0x7f) {
            return Err(format!("URL contains a forbidden byte 0x{b:02x}"));
        }
        let (https, rest) = if let Some(r) = strip_prefix_ci(input, "https://") {
            (true, r)
        } else if let Some(r) = strip_prefix_ci(input, "http://") {
            (false, r)
        } else {
            return Err("scheme must be http or https".into());
        };
        let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(auth_end);
        if authority.contains('@') {
            return Err("userinfo in the URL is not supported".into());
        }
        if authority.contains('[') || authority.contains(']') {
            return Err("IPv6 literals are not supported".into());
        }
        let default_port = if https { 443 } else { 80 };
        let (host, port) = match authority.rfind(':') {
            Some(i) => {
                let port_text = authority.get(i + 1..).unwrap_or("");
                if port_text.is_empty()
                    || port_text.len() > 5
                    || !port_text.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(format!("invalid port {port_text:?}"));
                }
                let port: u16 = port_text
                    .parse()
                    .map_err(|_| format!("port {port_text:?} is out of range"))?;
                if port == 0 {
                    return Err("port 0 is not allowed".into());
                }
                (authority.get(..i).unwrap_or(""), port)
            }
            None => (authority, default_port),
        };
        let host = validate_host(host)?;
        let tail = tail.split('#').next().unwrap_or("");
        let path_and_query = if tail.is_empty() {
            "/".to_owned()
        } else if tail.starts_with('?') {
            format!("/{tail}")
        } else {
            tail.to_owned()
        };
        Ok(Self {
            https,
            host,
            port,
            path_and_query,
        })
    }

    pub fn scheme(&self) -> &'static str {
        if self.https {
            "https"
        } else {
            "http"
        }
    }

    pub fn default_port(&self) -> u16 {
        if self.https {
            443
        } else {
            80
        }
    }

    pub fn host_header(&self) -> String {
        if self.port == self.default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn origin(&self) -> String {
        format!("{}://{}", self.scheme(), self.host_header())
    }

    pub fn to_url_string(&self) -> String {
        format!("{}{}", self.origin(), self.path_and_query)
    }

    pub fn resolve(&self, location: &str) -> Result<ParsedUrl, String> {
        let loc = sanitize(location);
        if loc.is_empty() {
            return Err("empty Location".into());
        }
        if let Some(scheme) = scheme_of(&loc) {
            let lower = scheme.to_ascii_lowercase();
            if lower == "http" || lower == "https" {
                let after = loc.get(scheme.len() + 1..).unwrap_or("");
                if after.starts_with("//") {
                    return Self::parse(&loc);
                }
                return Err(format!("{lower}: URL without // is not supported"));
            }
            return Err(format!("scheme {lower:?} is not http or https"));
        }
        let loc = loc.split('#').next().unwrap_or("");
        if loc.is_empty() {
            return Ok(self.clone());
        }
        if let Some(rest) = loc.strip_prefix("//") {
            return Self::parse(&format!("{}://{rest}", self.scheme()));
        }
        let (path_part, query_part) = match loc.find('?') {
            Some(i) => (loc.get(..i).unwrap_or(""), loc.get(i..).unwrap_or("")),
            None => (loc, ""),
        };
        let path = if path_part.starts_with('/') {
            remove_dot_segments(path_part)
        } else if path_part.is_empty() {
            self.path_and_query
                .split('?')
                .next()
                .unwrap_or("/")
                .to_owned()
        } else {
            let base_path = self.path_and_query.split('?').next().unwrap_or("/");
            let dir = base_path
                .rfind('/')
                .and_then(|i| base_path.get(..=i))
                .unwrap_or("/");
            remove_dot_segments(&format!("{dir}{path_part}"))
        };
        Self::parse(&format!("{}{path}{query_part}", self.origin()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> ParsedUrl {
        ParsedUrl::parse(s).unwrap()
    }

    #[test]
    fn accepts_plain_forms() {
        let u = p("http://Example.COM");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path_and_query, "/");
        assert!(!u.https);
        let u = p("HTTPS://a.b:8443/x/y?q=1#frag");
        assert!(u.https);
        assert_eq!(u.port, 8443);
        assert_eq!(u.path_and_query, "/x/y?q=1");
        assert_eq!(p("http://1.2.3.4/").host, "1.2.3.4");
        assert_eq!(p("http://h?x=1").path_and_query, "/?x=1");
        assert_eq!(p("http://h#f").path_and_query, "/");
    }

    #[test]
    fn rejects_bad_urls() {
        for bad in [
            "",
            "example.com",
            "ftp://example.com/",
            "javascript:alert(1)",
            "http://",
            "http:///path",
            "http://user@host/",
            "http://user:pw@host/",
            "http://[::1]/",
            "http://host:/",
            "http://host:0/",
            "http://host:65536/",
            "http://host:12ab/",
            "http://host:123456/",
            "http://exa mple.com/",
            "http://example.com/\n",
            "http://1.2.3/",
            "http://256.1.1.1/",
            "http://a..b/",
            "http://-bad!.com/",
            "http://caf\u{e9}.com/",
        ] {
            assert!(ParsedUrl::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn rejects_overlong() {
        let long = format!("http://a.com/{}", "x".repeat(MAX_URL_LEN));
        assert!(ParsedUrl::parse(&long).unwrap_err().contains("limit"));
        let edge = format!("http://a.com/{}", "x".repeat(MAX_URL_LEN - 13));
        assert_eq!(edge.len(), MAX_URL_LEN);
        assert!(ParsedUrl::parse(&edge).is_ok());
        let label = format!("http://{}.com/", "a".repeat(64));
        assert!(ParsedUrl::parse(&label).is_err());
    }

    #[test]
    fn host_header_omits_default_port() {
        assert_eq!(p("http://a.com:80/").host_header(), "a.com");
        assert_eq!(p("https://a.com:443/").host_header(), "a.com");
        assert_eq!(p("http://a.com:8080/").host_header(), "a.com:8080");
    }

    #[test]
    fn resolves_locations() {
        let base = p("https://a.example:8443/dir/sub/page.html?old=1");
        let r = |l: &str| base.resolve(l).map(|u| u.to_url_string());
        assert_eq!(r("http://other.test/x").unwrap(), "http://other.test/x");
        assert_eq!(r("HTTPS://Other.test").unwrap(), "https://other.test/");
        assert_eq!(r("//cdn.test/a?b=1").unwrap(), "https://cdn.test/a?b=1");
        assert_eq!(r("/abs?z=2#f").unwrap(), "https://a.example:8443/abs?z=2");
        assert_eq!(
            r("next.html").unwrap(),
            "https://a.example:8443/dir/sub/next.html"
        );
        assert_eq!(
            r("../up.html").unwrap(),
            "https://a.example:8443/dir/up.html"
        );
        assert_eq!(
            r("../../../../way/up").unwrap(),
            "https://a.example:8443/way/up"
        );
        assert_eq!(
            r("./a/./b/../c").unwrap(),
            "https://a.example:8443/dir/sub/a/c"
        );
        assert_eq!(
            r("?q=9").unwrap(),
            "https://a.example:8443/dir/sub/page.html?q=9"
        );
        assert_eq!(
            r("#only").unwrap(),
            "https://a.example:8443/dir/sub/page.html?old=1"
        );
        assert_eq!(
            r("sp ace").unwrap(),
            "https://a.example:8443/dir/sub/sp%20ace"
        );
        assert_eq!(r("..").unwrap(), "https://a.example:8443/dir/");
    }

    #[test]
    fn refuses_non_http_locations() {
        let base = p("http://a.test/");
        for loc in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "JavaScript:void(0)",
            "ftp://x/",
            "mailto:a@b",
            "http:nohost",
            "",
            "//",
            "//user@host/",
            "http://[::1]/",
        ] {
            assert!(base.resolve(loc).is_err(), "accepted {loc:?}");
        }
        assert!(base
            .resolve("javascript:alert(1)")
            .unwrap_err()
            .contains("not http or https"));
    }
}
