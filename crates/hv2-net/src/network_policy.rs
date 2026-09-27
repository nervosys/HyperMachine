//! Per-sandbox egress policy, in E2B's terms.
//!
//! E2B's `NewSandbox` carries `allow_internet_access` and a `network` object
//! with `allowOut`, `denyOut` and per-domain `rules` (see `spec/openapi.yml`,
//! `SandboxNetworkConfig`). This is that configuration as a decision the
//! [`gateway`](crate::gateway) can ask, per connection:
//!
//! - `allowOut` entries are CIDRs, bare addresses, or names (`example.com`,
//!   `*.example.com`). They take precedence over `denyOut`.
//! - `denyOut` entries are CIDRs or bare addresses only; the spec does not
//!   allow names there, and neither does this.
//! - `allow_internet_access: false` is `denyOut: ["0.0.0.0/0"]`, as the spec
//!   says -- plus `::/0`, so that it means what it says on a dual-stack host.
//! - `rules` map a name pattern to headers injected into matching HTTPS
//!   requests. A rule grants no access on its own.
//!
//! # Where this deliberately differs from E2B
//!
//! **What "nothing configured" means is the operator's call.** E2B allows
//! everything by default. A server here is started with a default of its own
//! ([`Verdict::Deny`] unless told otherwise), because a sandbox runs code an
//! agent was told to run by something it read, and "nobody decided" should
//! not read as "everything is allowed".
//!
//! **Reserved addresses need an address rule.** Loopback, link-local
//! (including `169.254.169.254`, every cloud's metadata service), RFC 1918,
//! CGNAT, multicast and the gateway's own subnet are refused unless an
//! `allowOut` *CIDR* covers them. A name rule never does: if `example.com`
//! is allowed and its DNS answer is `10.0.0.5`, that is DNS rebinding into
//! the host's network, not a trip to example.com.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::egress::within;

/// Allowed or not, and why -- the reason is what the audit log records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

/// What can be decided from an address alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressVerdict {
    Allow(&'static str),
    Deny(&'static str),
    /// The address alone would be refused, but a name rule could still allow
    /// it. The gateway has to see the name the client asks for -- TLS SNI or
    /// an HTTP `Host` -- before deciding.
    NeedsName,
}

/// A name, or every name under a suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePattern {
    Exact(String),
    /// `*.example.com`: any depth of subdomain, and not `example.com` itself.
    /// The spec says so for `rules`; `allowOut` is read the same way so that
    /// one pattern means one thing wherever it is written.
    Subdomains(String),
}

impl NamePattern {
    /// Parse `example.com` or `*.example.com`.
    ///
    /// # Errors
    ///
    /// A bare `*`, an empty name, or a wildcard anywhere but the front.
    pub fn parse(pattern: &str) -> Result<Self, PolicyError> {
        let lowered = normalise(pattern);
        if let Some(suffix) = lowered.strip_prefix("*.") {
            if suffix.is_empty() || suffix.contains('*') {
                return Err(PolicyError::BadName(pattern.to_string()));
            }
            return Ok(Self::Subdomains(suffix.to_string()));
        }
        if lowered.is_empty() || lowered.contains('*') || lowered.contains('/') {
            return Err(PolicyError::BadName(pattern.to_string()));
        }
        Ok(Self::Exact(lowered))
    }

    /// Does this pattern cover `name`? `name` must already be normalised.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact(exact) => exact == name,
            Self::Subdomains(suffix) => name
                .strip_suffix(suffix.as_str())
                .is_some_and(|head| head.len() > 1 && head.ends_with('.')),
        }
    }

    /// How specific this pattern is, for "longest matching wildcard wins".
    fn specificity(&self) -> (bool, usize) {
        match self {
            Self::Exact(name) => (true, name.len()),
            Self::Subdomains(suffix) => (false, suffix.len()),
        }
    }
}

/// Lowercase, without a trailing root dot.
#[must_use]
pub fn normalise(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// An address and prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    pub address: IpAddr,
    pub prefix: u8,
}

impl Cidr {
    /// Parse `10.0.0.0/8`, `2001:db8::/32`, or a bare address.
    ///
    /// # Errors
    ///
    /// Anything that is not one of those, or a prefix longer than the family.
    pub fn parse(text: &str) -> Result<Self, PolicyError> {
        let text = text.trim();
        let (address, prefix) = match text.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (text, None),
        };
        let address: IpAddr = address
            .parse()
            .map_err(|_| PolicyError::BadAddress(text.to_string()))?;
        let bits = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => bits,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= bits)
                .ok_or_else(|| PolicyError::BadAddress(text.to_string()))?,
        };
        Ok(Self { address, prefix })
    }

    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        within(address, self.address, self.prefix)
    }
}

/// Why a configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("'{0}' is not a CIDR block or an IP address")]
    BadAddress(String),
    #[error("'{0}' is not a usable name pattern (a bare '*' is not allowed)")]
    BadName(String),
    #[error("denyOut takes addresses only, and '{0}' is a name")]
    NameInDeny(String),
    #[error("'{0}' is not a valid HTTP header name or value")]
    BadHeader(String),
}

/// Headers injected into HTTPS requests to a matching name.
pub type Headers = BTreeMap<String, String>;

/// A sandbox's egress policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPolicy {
    allow_addresses: Vec<Cidr>,
    allow_names: Vec<NamePattern>,
    deny_addresses: Vec<Cidr>,
    transforms: Vec<(NamePattern, Headers)>,
    /// For a destination no rule speaks to.
    default: Verdict,
}

impl NetworkPolicy {
    /// A policy with no rules, answering `default` for everything that is not
    /// a reserved address.
    #[must_use]
    pub fn new(default: Verdict) -> Self {
        Self {
            allow_addresses: Vec::new(),
            allow_names: Vec::new(),
            deny_addresses: Vec::new(),
            transforms: Vec::new(),
            default,
        }
    }

    /// Build from E2B's fields.
    ///
    /// `operator_default` applies only when the request configured nothing at
    /// all -- neither `allow_internet_access` nor any list. Once a caller has
    /// said anything, E2B's semantics hold: what is not denied is allowed.
    ///
    /// # Errors
    ///
    /// The first entry that does not parse. A policy is all or nothing: a
    /// sandbox created with half its deny list would be worse than one that
    /// was refused.
    pub fn from_e2b(
        allow_internet_access: Option<bool>,
        allow_out: &[String],
        deny_out: &[String],
        rules: &[(String, Headers)],
        operator_default: Verdict,
    ) -> Result<Self, PolicyError> {
        let configured =
            allow_internet_access.is_some() || !allow_out.is_empty() || !deny_out.is_empty();
        let mut policy = Self::new(if configured {
            Verdict::Allow
        } else {
            operator_default
        });

        for entry in allow_out {
            match Cidr::parse(entry) {
                Ok(cidr) => policy.allow_addresses.push(cidr),
                Err(_) => policy.allow_names.push(NamePattern::parse(entry)?),
            }
        }
        for entry in deny_out {
            match Cidr::parse(entry) {
                Ok(cidr) => policy.deny_addresses.push(cidr),
                Err(_) => {
                    // Distinguish "a name, which deny does not take" from
                    // garbage, because the fix is different.
                    return Err(match NamePattern::parse(entry) {
                        Ok(_) => PolicyError::NameInDeny(entry.clone()),
                        Err(e) => e,
                    });
                }
            }
        }
        if allow_internet_access == Some(false) {
            policy.deny_addresses.push(Cidr {
                address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                prefix: 0,
            });
            policy.deny_addresses.push(Cidr {
                address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                prefix: 0,
            });
        }
        for (pattern, headers) in rules {
            for (name, value) in headers {
                let valid_name = !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
                if !valid_name || value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
                    return Err(PolicyError::BadHeader(format!("{name}: {value}")));
                }
            }
            policy
                .transforms
                .push((NamePattern::parse(pattern)?, headers.clone()));
        }
        Ok(policy)
    }

    /// Everything a guest is not allowed to reach no matter what it asks,
    /// unless an address rule says otherwise.
    #[must_use]
    pub fn is_reserved(address: IpAddr) -> bool {
        match address {
            IpAddr::V4(v4) => {
                v4.is_unspecified()
                    || v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_multicast()
                    || v4.is_broadcast()
                    // 100.64.0.0/10, carrier-grade NAT: somebody's internal network.
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
                    // 0.0.0.0/8 and 240.0.0.0/4.
                    || v4.octets()[0] == 0
                    || v4.octets()[0] >= 240
            }
            IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return Self::is_reserved(IpAddr::V4(v4));
                }
                let first = v6.segments()[0];
                v6.is_unspecified()
                    || v6.is_loopback()
                    || v6.is_multicast()
                    || (first & 0xfe00) == 0xfc00 // unique local
                    || (first & 0xffc0) == 0xfe80 // link local
            }
        }
    }

    fn address_allowed(&self, address: IpAddr) -> bool {
        self.allow_addresses.iter().any(|c| c.contains(address))
    }

    fn address_denied(&self, address: IpAddr) -> bool {
        self.deny_addresses.iter().any(|c| c.contains(address))
    }

    /// Does any `allowOut` name cover `name`?
    #[must_use]
    pub fn name_allowed(&self, name: &str) -> bool {
        let name = normalise(name);
        self.allow_names.iter().any(|p| p.matches(&name))
    }

    /// May the gateway resolve `name` for the guest?
    ///
    /// A query is itself egress -- its question reaches whoever runs the zone
    /// -- so it is answered only if the name is allowed, or if the policy
    /// would let unnamed traffic to a public address out anyway, in which case
    /// refusing the lookup protects nothing. A policy that allows only
    /// addresses gets no resolution for other names: resolving them would
    /// open the one channel the policy otherwise closed.
    #[must_use]
    pub fn may_resolve(&self, name: &str) -> bool {
        self.name_allowed(name)
            || (self.default == Verdict::Allow
                && !self.deny_addresses.iter().any(|c| c.prefix == 0))
    }

    /// Decide from the address alone.
    #[must_use]
    pub fn decide_address(&self, address: IpAddr) -> AddressVerdict {
        if self.address_allowed(address) {
            return AddressVerdict::Allow("allowOut address");
        }
        if Self::is_reserved(address) {
            return AddressVerdict::Deny("reserved address");
        }
        let refused = if self.address_denied(address) {
            Some("denyOut address")
        } else if self.default == Verdict::Deny {
            Some("default deny")
        } else {
            None
        };
        match refused {
            None => AddressVerdict::Allow("default allow"),
            Some(_) if !self.allow_names.is_empty() => AddressVerdict::NeedsName,
            Some(reason) => AddressVerdict::Deny(reason),
        }
    }

    /// Decide knowing the name the client asked for.
    ///
    /// The caller is responsible for having established that `address` is
    /// really where `name` points; a guest that can choose both can otherwise
    /// send an allowed name to any address it likes.
    #[must_use]
    pub fn decide(&self, address: IpAddr, name: &str) -> (Verdict, &'static str) {
        if self.address_allowed(address) {
            return (Verdict::Allow, "allowOut address");
        }
        if Self::is_reserved(address) {
            return (Verdict::Deny, "reserved address");
        }
        if self.name_allowed(name) {
            return (Verdict::Allow, "allowOut name");
        }
        if self.address_denied(address) {
            return (Verdict::Deny, "denyOut address");
        }
        match self.default {
            Verdict::Allow => (Verdict::Allow, "default allow"),
            Verdict::Deny => (Verdict::Deny, "default deny"),
        }
    }

    /// The headers to inject into HTTPS requests for `name`, if any.
    ///
    /// Exact rules first, then the longest matching wildcard suffix; matching
    /// sets are not merged. All three are the spec's words.
    #[must_use]
    pub fn transform_for(&self, name: &str) -> Option<&Headers> {
        let name = normalise(name);
        self.transforms
            .iter()
            .filter(|(p, _)| p.matches(&name))
            .max_by_key(|(p, _)| p.specificity())
            .map(|(_, h)| h)
    }

    /// Are there any transform rules at all? Deciding whether to look inside a
    /// connection is cheaper when the answer is no.
    #[must_use]
    pub fn has_transforms(&self) -> bool {
        !self.transforms.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn policy(allow: &[&str], deny: &[&str]) -> NetworkPolicy {
        NetworkPolicy::from_e2b(
            None,
            &allow.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
            &deny.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
            &[],
            Verdict::Deny,
        )
        .unwrap()
    }

    #[test]
    fn nothing_configured_takes_the_operators_default() {
        let deny = NetworkPolicy::from_e2b(None, &[], &[], &[], Verdict::Deny).unwrap();
        assert_eq!(
            deny.decide_address(ip("93.184.216.34")),
            AddressVerdict::Deny("default deny")
        );
        let allow = NetworkPolicy::from_e2b(None, &[], &[], &[], Verdict::Allow).unwrap();
        assert_eq!(
            allow.decide_address(ip("93.184.216.34")),
            AddressVerdict::Allow("default allow")
        );
    }

    /// Once the caller says anything, E2B's reading applies: what is not
    /// denied is allowed, even on a server whose own default is deny.
    #[test]
    fn saying_anything_switches_to_e2b_semantics() {
        let p = policy(&[], &["8.8.8.8"]);
        assert_eq!(
            p.decide_address(ip("1.1.1.1")),
            AddressVerdict::Allow("default allow")
        );
        assert_eq!(
            p.decide_address(ip("8.8.8.8")),
            AddressVerdict::Deny("denyOut address")
        );
    }

    #[test]
    fn allow_internet_access_false_denies_both_families() {
        let p = NetworkPolicy::from_e2b(Some(false), &[], &[], &[], Verdict::Allow).unwrap();
        assert!(matches!(
            p.decide_address(ip("1.1.1.1")),
            AddressVerdict::Deny(_)
        ));
        assert!(matches!(
            p.decide_address(ip("2606:4700::1111")),
            AddressVerdict::Deny(_)
        ));
    }

    #[test]
    fn allow_beats_deny() {
        let p = policy(&["1.1.1.1"], &["0.0.0.0/0"]);
        assert_eq!(
            p.decide_address(ip("1.1.1.1")),
            AddressVerdict::Allow("allowOut address")
        );
        assert!(matches!(
            p.decide_address(ip("1.0.0.1")),
            AddressVerdict::Deny(_)
        ));
    }

    #[test]
    fn a_name_rule_makes_a_denied_address_a_question() {
        let p = policy(&["example.com"], &["0.0.0.0/0"]);
        assert_eq!(
            p.decide_address(ip("93.184.216.34")),
            AddressVerdict::NeedsName
        );
        assert_eq!(
            p.decide(ip("93.184.216.34"), "example.com"),
            (Verdict::Allow, "allowOut name")
        );
        assert_eq!(
            p.decide(ip("93.184.216.34"), "evil.com"),
            (Verdict::Deny, "denyOut address")
        );
    }

    /// DNS rebinding: an allowed name answering with a private address does
    /// not open the host's network.
    #[test]
    fn a_name_never_opens_a_reserved_address() {
        let p = policy(&["example.com"], &[]);
        for addr in [
            "10.0.0.5",
            "127.0.0.1",
            "169.254.169.254",
            "192.168.1.1",
            "100.64.0.1",
        ] {
            assert_eq!(
                p.decide(ip(addr), "example.com"),
                (Verdict::Deny, "reserved address"),
                "{addr}"
            );
        }
        let explicit = policy(&["10.0.0.0/8"], &[]);
        assert_eq!(
            explicit.decide_address(ip("10.0.0.5")),
            AddressVerdict::Allow("allowOut address"),
            "an address rule is how an operator says they meant it"
        );
    }

    #[test]
    fn wildcards_cover_subdomains_and_not_the_apex() {
        let p = NamePattern::parse("*.Example.COM.").unwrap();
        assert!(p.matches("api.example.com"));
        assert!(p.matches("a.b.example.com"));
        assert!(!p.matches("example.com"));
        assert!(
            !p.matches("notexample.com"),
            "a suffix is a label boundary, not a string"
        );
        assert!(NamePattern::parse("*").is_err());
        assert!(NamePattern::parse("a.*.com").is_err());
    }

    #[test]
    fn deny_refuses_names_with_a_reason_that_says_so() {
        let err =
            NetworkPolicy::from_e2b(None, &[], &["example.com".to_string()], &[], Verdict::Deny)
                .unwrap_err();
        assert_eq!(err, PolicyError::NameInDeny("example.com".to_string()));
    }

    #[test]
    fn transforms_pick_exact_then_longest_suffix_and_never_merge() {
        let mut exact = Headers::new();
        exact.insert("Authorization".into(), "exact".into());
        let mut short = Headers::new();
        short.insert("Authorization".into(), "short".into());
        let mut long = Headers::new();
        long.insert("Authorization".into(), "long".into());
        let p = NetworkPolicy::from_e2b(
            None,
            &[],
            &[],
            &[
                ("*.com".into(), short),
                ("*.example.com".into(), long),
                ("api.example.com".into(), exact),
            ],
            Verdict::Deny,
        )
        .unwrap();
        let get = |n: &str| p.transform_for(n).map(|h| h["Authorization"].clone());
        assert_eq!(get("api.example.com").as_deref(), Some("exact"));
        assert_eq!(get("x.example.com").as_deref(), Some("long"));
        assert_eq!(get("other.com").as_deref(), Some("short"));
        assert_eq!(get("example.org"), None);
    }

    /// A header value is written into a request line by line; a newline in
    /// one is a second header the caller did not ask for.
    #[test]
    fn a_header_that_would_split_a_request_is_refused() {
        let mut bad = Headers::new();
        bad.insert("X-Key".into(), "v\r\nHost: evil".into());
        assert!(matches!(
            NetworkPolicy::from_e2b(None, &[], &[], &[("a.com".into(), bad)], Verdict::Deny),
            Err(PolicyError::BadHeader(_))
        ));
    }

    #[test]
    fn cidrs_parse_bare_addresses_and_refuse_nonsense() {
        assert_eq!(Cidr::parse("8.8.8.8").unwrap().prefix, 32);
        assert_eq!(Cidr::parse("2001:db8::/32").unwrap().prefix, 32);
        assert!(Cidr::parse("10.0.0.0/33").is_err());
        assert!(Cidr::parse("example.com").is_err());
    }
}
