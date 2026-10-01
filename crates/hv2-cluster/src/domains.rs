//! Validated custom-domain bindings, shared by storage and API routing.

use serde::{Deserialize, Serialize};

/// Canonical ASCII DNS hostname. IDNs use their DNS punycode representation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DomainName(String);

impl DomainName {
    /// Validate and canonicalize an ASCII DNS name.
    ///
    /// # Errors
    /// Reject invalid labels, IP addresses and reserved sandbox hostnames.
    pub fn parse(value: &str) -> Result<Self, String> {
        let host = value
            .strip_suffix('.')
            .unwrap_or(value)
            .to_ascii_lowercase();
        if host.len() > 253 || !host.contains('.') || !host.is_ascii() {
            return Err("domain must be an ASCII DNS hostname of at most 253 bytes".into());
        }
        for label in host.split('.') {
            if label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err("domain contains an invalid DNS label".into());
            }
        }
        if host.parse::<std::net::IpAddr>().is_ok()
            || host
                .rsplit('.')
                .next()
                .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err("domain must be a DNS name, not an IP address".into());
        }
        if hv2_api::sandbox_proxy::route_of(&host).is_some() {
            return Err("domain conflicts with the reserved sandbox hostname format".into());
        }
        Ok(Self(host))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DomainName {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<DomainName> for String {
    fn from(value: DomainName) -> Self {
        value.0
    }
}

/// One hostname names one sandbox port. Storage must claim ownership atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawBinding")]
pub struct DomainBinding {
    domain: DomainName,
    sandbox_id: String,
    port: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinding {
    domain: DomainName,
    sandbox_id: String,
    port: u16,
}

impl TryFrom<RawBinding> for DomainBinding {
    type Error = String;
    fn try_from(raw: RawBinding) -> Result<Self, Self::Error> {
        Self::new(raw.domain.as_str(), &raw.sandbox_id, raw.port)
    }
}

impl DomainBinding {
    /// Construct a validated domain, sandbox and port binding.
    ///
    /// # Errors
    /// Reject invalid hostnames, sandbox identifiers and port zero.
    pub fn new(domain: &str, sandbox_id: &str, port: u16) -> Result<Self, String> {
        if port == 0
            || sandbox_id.is_empty()
            || sandbox_id.len() > 128
            || !sandbox_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err("binding requires a sandbox identifier and a nonzero port".into());
        }
        Ok(Self {
            domain: DomainName::parse(domain)?,
            sandbox_id: sandbox_id.into(),
            port,
        })
    }
    #[must_use]
    pub fn domain(&self) -> &DomainName {
        &self.domain
    }
    #[must_use]
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_and_binding_fields_survive_round_trip() {
        let binding = DomainBinding::new("App.Example.COM.", "sbx-one", 8080).unwrap();
        assert_eq!(binding.domain().as_str(), "app.example.com");
        assert_eq!(binding.sandbox_id(), "sbx-one");
        assert_eq!(binding.port(), 8080);
        assert_eq!(
            serde_json::from_str::<DomainBinding>(&serde_json::to_string(&binding).unwrap())
                .unwrap(),
            binding
        );
        assert!(DomainName::parse("xn--bcher-kva.example").is_ok());
    }

    #[test]
    fn rejects_authorities_ips_bad_labels_and_reserved_routes() {
        for host in [
            "localhost",
            "127.0.0.1",
            "127.1",
            "https://app.example",
            "app.example:443",
            "app.example/path",
            "a..example",
            "-app.example",
            "app-.example",
            " app.example",
            "app.example..",
            "bücher.example",
            "8080-sbx-one.example",
        ] {
            assert!(DomainName::parse(host).is_err(), "{host}");
        }
        assert!(DomainName::parse(&format!("{}.example", "a".repeat(64))).is_err());
        assert!(DomainName::parse(&format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(63)
        ))
        .is_err());
    }

    #[test]
    fn stored_json_cannot_bypass_binding_validation() {
        for raw in [
            r#"{"domain":"app.example","sandbox_id":"sbx-one","port":0}"#,
            r#"{"domain":"app.example","sandbox_id":"../other","port":80}"#,
            r#"{"domain":"app.example","sandbox_id":"sbx-one","port":80,"extra":true}"#,
        ] {
            assert!(serde_json::from_str::<DomainBinding>(raw).is_err());
        }
    }
}
