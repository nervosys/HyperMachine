//! URLs an operator allows a daemon to call on a user's behalf.
//!
//! A node clones the Git and Lit remotes its operator lists, and a control
//! plane pushes a team's telemetry to the collectors its operator lists. In
//! both, a user names a URL and the daemon, which sits inside the operator's
//! network, makes the request. The operator's list is what stops a user
//! from pointing it at whatever they like, so what counts as "under" an
//! entry is settled here, once.

/// Whether `url` is one of `allowed`, or under one: the same scheme, host
/// and port, and a path at or below the allowed one's.
///
/// Both are parsed first, so a URL's dot segments, the case of its scheme
/// and host, and a port left out or given as the scheme's own are settled
/// before they are compared: `https://h/a/../b` is `https://h/b`. A URL that
/// does not parse is never allowed, and neither is one carrying credentials,
/// which would be sent to the host as given.
#[must_use]
pub fn allowed(url: &str, allowed: &[String]) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    allowed.iter().any(|prefix| {
        let Ok(prefix) = reqwest::Url::parse(prefix) else {
            return false;
        };
        let base = prefix.path().trim_end_matches('/');
        url.scheme() == prefix.scheme()
            && url.host_str() == prefix.host_str()
            && url.port_or_known_default() == prefix.port_or_known_default()
            && (url.path().trim_end_matches('/') == base
                || url.path().starts_with(&format!("{base}/")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| (*e).to_string()).collect()
    }

    #[test]
    fn a_url_is_allowed_at_or_below_an_entry() {
        let entries = list(&["https://collector.example/teams", "http://127.0.0.1:4318"]);
        for url in [
            "https://collector.example/teams",
            "https://collector.example/teams/",
            "https://collector.example/teams/red/v1/metrics",
            "https://COLLECTOR.example:443/teams/red",
            "http://127.0.0.1:4318",
            "http://127.0.0.1:4318/v1/metrics",
        ] {
            assert!(allowed(url, &entries), "{url}");
        }
    }

    #[test]
    fn nothing_else_is_allowed() {
        let entries = list(&["https://collector.example/teams", "http://127.0.0.1:4318"]);
        for url in [
            "https://collector.example/teammate",
            "https://collector.example/other",
            "https://collector.example/",
            "http://collector.example/teams",
            "https://collector.example.evil/teams",
            "https://evil.example/teams",
            "http://127.0.0.1:4319",
            "http://169.254.169.254/teams",
            "not a url",
            "",
        ] {
            assert!(!allowed(url, &entries), "{url}");
        }
        assert!(!allowed("https://collector.example/teams", &[]));
    }

    #[test]
    fn dot_segments_and_credentials_do_not_get_past_an_entry() {
        let entries = list(&["https://collector.example/teams"]);
        // Settled by parsing: this is /other, outside the entry.
        assert!(!allowed(
            "https://collector.example/teams/../other",
            &entries
        ));
        assert!(allowed(
            "https://collector.example/other/../teams/x",
            &entries
        ));
        // Credentials in a URL would be sent as given.
        assert!(!allowed(
            "https://user:pw@collector.example/teams/red",
            &entries
        ));
        assert!(!allowed(
            "https://tok@collector.example/teams/red",
            &entries
        ));
    }
}
