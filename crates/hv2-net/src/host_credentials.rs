//! Credentials the host holds for HTTPS remotes, sent as `Authorization` on a
//! guest's requests to them.
//!
//! The guest never has the credential: the gateway, which already terminates
//! its TLS, sets the header on a request whose scheme, host, port and path
//! prefix match, replacing any the guest wrote. A credential grants no network
//! access -- the sandbox's egress policy still has to allow the destination --
//! and is sent only to what its URL names: `https://git.example/team/` matches
//! `/team/` and below on that host and port, not `/teammate/`, and not a path
//! that climbs out with `..` or an encoded dot or slash.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use hyper::header::HeaderValue;
use std::io;
use zeroize::Zeroizing;

/// One credential and the place it is for.
struct Entry {
    host: String,
    port: u16,
    /// Always starts and ends with `/`.
    prefix: String,
    authorization: Zeroizing<String>,
}

/// Credentials by URL prefix; the first that matches a request is used.
#[derive(Default)]
pub struct HostCredentials(Vec<Entry>);

impl std::fmt::Debug for HostCredentials {
    // The values are secret, and so is how many there are for which place.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostCredentials(..)")
    }
}

impl HostCredentials {
    /// Add `user_token` (`user:token`, sent as HTTP Basic) for `url`.
    ///
    /// # Errors
    ///
    /// `url` is not `https://host[:port][/path]` with no user, query or
    /// fragment, or `user_token` has no `user:token` shape.
    pub fn add(&mut self, url: &str, user_token: &str) -> io::Result<()> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_string());
        let rest = url
            .strip_prefix("https://")
            .ok_or_else(|| bad("a host credential is for an https URL"))?;
        if rest.contains(['?', '#', '@', '\\']) {
            return Err(bad(
                "a host credential's URL has no user, query or fragment",
            ));
        }
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| bad("the port is not a number"))?,
            ),
            None => (authority, 443),
        };
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(bad("the host is not a plain name"));
        }
        let mut prefix = format!("/{path}");
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        if unsafe_path(&prefix) {
            return Err(bad("the path has a dot segment or an encoded separator"));
        }
        match user_token.split_once(':') {
            Some((user, token)) if !user.is_empty() && !token.is_empty() => {}
            _ => return Err(bad("a credential is user:token")),
        }
        self.0.push(Entry {
            host: host.to_ascii_lowercase(),
            port,
            prefix,
            authorization: Zeroizing::new(format!("Basic {}", STANDARD.encode(user_token))),
        });
        Ok(())
    }

    /// Whether any credential is for this host and port.
    #[must_use]
    pub fn has_target(&self, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();
        self.0.iter().any(|e| e.host == host && e.port == port)
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `Authorization` value for a request to `path` on `host:port`.
    #[must_use]
    pub fn authorization_for(&self, host: &str, port: u16, path: &str) -> Option<HeaderValue> {
        if unsafe_path(path) {
            return None;
        }
        let host = host.to_ascii_lowercase();
        // `/team` is the directory itself, as `/team/` is.
        let with_slash;
        let path = if path.ends_with('/') {
            path
        } else {
            with_slash = format!("{path}/");
            // Only to compare the prefix: a file `/team/x.git` is under
            // `/team/`, and `/team` alone is the prefix's own directory.
            &with_slash
        };
        let entry = self
            .0
            .iter()
            .find(|e| e.host == host && e.port == port && path.starts_with(&e.prefix))?;
        let mut value = HeaderValue::from_str(&entry.authorization).ok()?;
        value.set_sensitive(true);
        Some(value)
    }
}

/// A path that could mean something other than where it appears to be.
fn unsafe_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
        || lower.contains('\\')
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> HostCredentials {
        let mut c = HostCredentials::default();
        c.add("https://git.example/team/", "bot:s3cret").unwrap();
        c
    }

    #[test]
    fn a_request_under_the_prefix_gets_basic_authorization() {
        let c = one();
        let v = c
            .authorization_for("git.example", 443, "/team/repo.git/info/refs")
            .unwrap();
        assert_eq!(
            v.to_str().unwrap(),
            format!("Basic {}", STANDARD.encode("bot:s3cret"))
        );
        assert!(v.is_sensitive());
        assert!(c.authorization_for("GIT.example", 443, "/team").is_some());
    }

    #[test]
    fn only_where_the_url_names() {
        let c = one();
        for (host, port, path) in [
            ("git.example", 443, "/teammate/x.git"),
            ("git.example", 443, "/other/x.git"),
            ("git.example", 443, "/"),
            ("git.example", 8443, "/team/x.git"),
            ("evil.git.example", 443, "/team/x.git"),
            ("git.example.evil", 443, "/team/x.git"),
        ] {
            assert!(
                c.authorization_for(host, port, path).is_none(),
                "{host}:{port}{path}"
            );
        }
    }

    #[test]
    fn a_path_that_climbs_out_or_hides_a_separator_gets_none() {
        let c = one();
        for path in [
            "/team/../other/x.git",
            "/team/%2e%2e/other",
            "/team/%2E%2E/other",
            "/team/a%2fb",
            "/team/./x",
            "/team\\..\\other",
        ] {
            assert!(
                c.authorization_for("git.example", 443, path).is_none(),
                "{path}"
            );
        }
    }

    #[test]
    fn what_is_not_a_credential_for_an_https_url_is_refused() {
        let mut c = HostCredentials::default();
        for (url, token) in [
            ("http://git.example/team/", "a:b"),
            ("https://u@git.example/team/", "a:b"),
            ("https://git.example/team/?x=1", "a:b"),
            ("https://git.example/a/../b/", "a:b"),
            ("https://git.example:xx/", "a:b"),
            ("https:///team/", "a:b"),
            ("https://git.example/team/", "nocolon"),
            ("https://git.example/team/", ":tok"),
            ("https://git.example/team/", "user:"),
        ] {
            assert!(c.add(url, token).is_err(), "{url} {token}");
        }
        assert!(c.is_empty());
    }

    #[test]
    fn the_first_matching_credential_wins_and_debug_hides_everything() {
        let mut c = HostCredentials::default();
        c.add("https://git.example/team/", "first:one").unwrap();
        c.add("https://git.example/", "second:two").unwrap();
        let v = c.authorization_for("git.example", 443, "/team/x").unwrap();
        assert_eq!(
            v.to_str().unwrap(),
            format!("Basic {}", STANDARD.encode("first:one"))
        );
        let v = c.authorization_for("git.example", 443, "/else/x").unwrap();
        assert_eq!(
            v.to_str().unwrap(),
            format!("Basic {}", STANDARD.encode("second:two"))
        );
        assert!(!format!("{c:?}").contains("one"));
        assert!(c.has_target("git.example", 443) && !c.has_target("git.example", 80));
    }
}
