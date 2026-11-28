//! What a shield says.
//!
//! A shield is JSON, and the bytes that were signed are the bytes that are
//! parsed: nothing is re-encoded before it is checked (see [`crate::sign`]).
//! Unknown fields are refused, so a field a later version adds cannot be
//! signed by one version and ignored by another.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::{Result, ShieldError};

/// The longest a shield may be in force: thirty days from when it is loaded.
/// A shield is a stopgap; one still needed after that is signed again.
pub const MAX_LIFETIME_SECS: u64 = 30 * 24 * 3600;
/// The most rules of each kind in one shield.
pub const MAX_RULES: usize = 64;
/// The most argument matches in one system-call rule.
pub const MAX_ARGS: usize = 6;
/// The longest a shield's JSON may be.
pub const MAX_SPEC_BYTES: usize = 64 * 1024;
/// The most sandboxes one shield may name.
pub const MAX_SCOPE_IDS: usize = 1024;

/// A shield: rules, where they apply, and when they stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShieldSpec {
    /// `[a-z0-9][a-z0-9-]*`, at most 63 characters. One shield of a name is
    /// in force on a node at a time.
    pub name: String,
    /// What it protects against, for the people who approve it.
    #[serde(default)]
    pub description: String,
    /// The advisory it answers, such as a CVE identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisory: Option<String>,
    /// The sandboxes it applies to.
    #[serde(default)]
    pub scope: Scope,
    /// System calls refused.
    #[serde(default)]
    pub syscalls: Vec<SyscallRule>,
    /// Connections refused.
    #[serde(default)]
    pub network: Vec<NetworkRule>,
    /// Kernel operations refused at a Linux security hook, whatever system
    /// call reaches them, by the eBPF backend (`hv2-shield-bpf`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookRule>,
    /// Unix seconds after which the shield is no longer in force, and is not
    /// loaded. At most [`MAX_LIFETIME_SECS`] after it is loaded.
    pub expires_at: u64,
}

/// The sandboxes a shield applies to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Scope {
    /// Every sandbox on the node, including those started later.
    #[default]
    All,
    /// These sandboxes, by ID.
    Sandboxes { ids: Vec<String> },
}

impl Scope {
    /// Whether the shield applies to the sandbox `id`.
    pub fn covers(&self, id: &str) -> bool {
        match self {
            Scope::All => true,
            Scope::Sandboxes { ids } => ids.iter().any(|i| i == id),
        }
    }
}

/// Refuse a system call, or a call whose arguments match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyscallRule {
    /// The call's name as in `<asm/unistd.h>`, such as `ptrace`. Each
    /// backend refuses a shield naming a call it cannot trap.
    pub syscall: String,
    /// Every one must hold for the rule to match. None: every call does.
    #[serde(default)]
    pub args: Vec<ArgMatch>,
    pub action: SyscallAction,
}

/// One test of one argument, as a register: `(arg & mask) <op> value`.
///
/// Only the argument's value is tested, never memory it points to: what a
/// pointer names can change between the test and the call, so a test of it
/// could be passed and then not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArgMatch {
    /// 0 to 5.
    pub index: u8,
    pub op: ArgOp,
    pub value: u64,
    /// Bits of the argument compared. An `int` argument should be masked to
    /// its 32 bits, since the rest of its register is not defined.
    #[serde(default = "all_bits")]
    pub mask: u64,
}

fn all_bits() -> u64 {
    u64::MAX
}

/// How an argument is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArgOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Any of `value`'s bits are set.
    AnyBits,
    /// All of `value`'s bits are set.
    AllBits,
}

impl ArgMatch {
    /// Whether `args` passes this test.
    pub fn holds(&self, args: &[u64; 6]) -> bool {
        let Some(&arg) = args.get(usize::from(self.index)) else {
            return false;
        };
        let arg = arg & self.mask;
        match self.op {
            ArgOp::Eq => arg == self.value,
            ArgOp::Ne => arg != self.value,
            ArgOp::Lt => arg < self.value,
            ArgOp::Le => arg <= self.value,
            ArgOp::Gt => arg > self.value,
            ArgOp::Ge => arg >= self.value,
            ArgOp::AnyBits => arg & self.value != 0,
            ArgOp::AllBits => arg & self.value == self.value,
        }
    }
}

impl SyscallRule {
    /// Whether a call with `args` matches.
    pub fn matches(&self, args: &[u64; 6]) -> bool {
        self.args.iter().all(|test| test.holds(args))
    }
}

/// Refuse a kernel operation at a Linux security (LSM) hook, such as
/// `socket_create` or `sb_mount`. The hook is asked whichever system call
/// leads to it, by every process in scope, and from the moment the rule is in
/// force: a hook rule needs nothing trapped in advance. It tests no arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HookRule {
    /// One of `hv2_shield_bpf::HOOKS`.
    pub hook: String,
    /// The operation fails with this `errno`, 1 to 4095. A hook cannot kill.
    pub errno: i32,
}

/// What happens to a refused call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SyscallAction {
    /// The call fails with this `errno`, 1 to 4095, without running.
    Errno { errno: i32 },
    /// The calling process is killed, without the call running.
    Kill,
}

/// Refuse connections. Every field given must match; at least an address
/// range or a name must be given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkRule {
    /// An address or a range, such as `203.0.113.0/24`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cidr: Option<String>,
    /// A name, or `*.` and a domain for any name under it. It matches where
    /// the gateway knows the name: a DNS query, or the name a connection
    /// sends (TLS SNI, HTTP Host). A connection straight to an address
    /// carries no name, so to close an address, give its range too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<PortRange>,
    #[serde(default)]
    pub protocol: Protocol,
}

/// Ports, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRange {
    pub from: u16,
    pub to: u16,
}

/// Which transport a network rule covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    #[default]
    Any,
    Tcp,
    Udp,
}

/// A connection as a network rule sees it.
#[derive(Debug, Clone, Copy)]
pub struct Connection<'a> {
    pub address: Option<IpAddr>,
    pub port: u16,
    pub name: Option<&'a str>,
    pub protocol: Protocol,
}

impl NetworkRule {
    /// Whether the rule refuses `connection`.
    pub fn matches(&self, connection: &Connection<'_>) -> bool {
        if let Some(cidr) = &self.cidr {
            let Some(address) = connection.address else {
                return false;
            };
            if !parse_cidr(cidr).is_some_and(|(net, len)| in_range(address, net, len)) {
                return false;
            }
        }
        if let Some(pattern) = &self.name {
            let Some(name) = connection.name else {
                return false;
            };
            if !name_matches(pattern, name) {
                return false;
            }
        }
        if let Some(ports) = self.ports {
            if !(ports.from..=ports.to).contains(&connection.port) {
                return false;
            }
        }
        match (self.protocol, connection.protocol) {
            (Protocol::Any, _) | (_, Protocol::Any) => true,
            (want, got) => want == got,
        }
    }
}

/// Whether `name` is `pattern`, or under it when `pattern` is `*.domain`.
/// Names compare without case and without a final dot.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(domain) => name
            .strip_suffix(domain)
            .is_some_and(|head| head.len() > 1 && head.ends_with('.')),
        None => name == pattern,
    }
}

/// `address/len`, or a bare address as its own range.
pub fn parse_cidr(text: &str) -> Option<(IpAddr, u8)> {
    let (address, len) = match text.split_once('/') {
        Some((address, len)) => (address, Some(len)),
        None => (text, None),
    };
    let address: IpAddr = address.parse().ok()?;
    let max = if address.is_ipv4() { 32 } else { 128 };
    let len = match len {
        Some(len) if !len.is_empty() && len.bytes().all(|b| b.is_ascii_digit()) => {
            len.parse::<u8>().ok().filter(|l| *l <= max)?
        }
        Some(_) => return None,
        None => max,
    };
    Some((address, len))
}

fn in_range(address: IpAddr, net: IpAddr, len: u8) -> bool {
    let (address, net, bits) = match (address, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => (u128::from(u32::from(a)), u128::from(u32::from(n)), 32),
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            // An IPv4 address seen as IPv6 is tested as itself.
            if let Some(v4) = a.to_ipv4_mapped() {
                return in_range(IpAddr::V4(v4), IpAddr::V6(n), len);
            }
            (u128::from(a), u128::from(n), 128)
        }
        (IpAddr::V4(a), IpAddr::V6(n)) => match n.to_ipv4_mapped() {
            Some(n4) if len >= 96 => {
                return in_range(IpAddr::V4(a), IpAddr::V4(n4), len - 96);
            }
            _ => return false,
        },
        (IpAddr::V6(a), IpAddr::V4(n)) => match a.to_ipv4_mapped() {
            Some(a4) => return in_range(IpAddr::V4(a4), IpAddr::V4(n), len),
            None => return false,
        },
    };
    let len = u32::from(len);
    if len == 0 {
        return true;
    }
    let shift = bits - len;
    (address >> shift) == (net >> shift)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.as_bytes()[0] != b'-'
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_host_pattern(pattern: &str) -> bool {
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

impl ShieldSpec {
    /// Parse a shield from its JSON, and check it.
    ///
    /// # Errors
    ///
    /// Says what is wrong with it.
    pub fn parse(json: &str) -> Result<Self> {
        if json.len() > MAX_SPEC_BYTES {
            return Err(ShieldError::Invalid(format!(
                "a shield is at most {MAX_SPEC_BYTES} bytes"
            )));
        }
        let spec: Self =
            serde_json::from_str(json).map_err(|e| ShieldError::Invalid(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    /// The checks every shield must pass, whatever enforces it. Whether it
    /// may be loaded *now* (its expiry) is the registry's to check.
    ///
    /// # Errors
    ///
    /// Says what is wrong with it.
    pub fn validate(&self) -> Result<()> {
        let bad = |why: String| Err(ShieldError::Invalid(why));
        if !valid_name(&self.name) {
            return bad("name is [a-z0-9-], at most 63 characters, not starting with -".into());
        }
        if self.description.len() > 4096 {
            return bad("description is at most 4096 bytes".into());
        }
        if let Some(advisory) = &self.advisory {
            if advisory.is_empty()
                || advisory.len() > 64
                || !advisory.bytes().all(|b| b.is_ascii_graphic())
            {
                return bad("advisory is 1 to 64 printable ASCII characters".into());
            }
        }
        if let Scope::Sandboxes { ids } = &self.scope {
            if ids.is_empty() || ids.len() > MAX_SCOPE_IDS {
                return bad(format!("a scope names 1 to {MAX_SCOPE_IDS} sandboxes"));
            }
            if ids.iter().any(|id| id.is_empty() || id.len() > 128) {
                return bad("a sandbox ID is 1 to 128 bytes".into());
            }
        }
        if self.syscalls.is_empty() && self.network.is_empty() && self.hooks.is_empty() {
            return bad("a shield has at least one rule".into());
        }
        if self.syscalls.len() > MAX_RULES
            || self.network.len() > MAX_RULES
            || self.hooks.len() > MAX_RULES
        {
            return bad(format!("at most {MAX_RULES} rules of each kind"));
        }
        for rule in &self.hooks {
            if !hv2_shield_bpf::HOOKS.contains(&rule.hook.as_str()) {
                return bad(format!(
                    "{:?} is not a hook a shield may name (see hv2_shield_bpf::HOOKS)",
                    rule.hook
                ));
            }
            if !(1..=4095).contains(&rule.errno) {
                return bad("errno is 1 to 4095".into());
            }
        }
        for rule in &self.syscalls {
            if rule.syscall.is_empty()
                || rule.syscall.len() > 32
                || !rule
                    .syscall
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                return bad(format!("{:?} is not a system call name", rule.syscall));
            }
            if rule.args.len() > MAX_ARGS {
                return bad(format!("at most {MAX_ARGS} argument tests in a rule"));
            }
            if rule.args.iter().any(|a| a.index > 5) {
                return bad("an argument index is 0 to 5".into());
            }
            if let SyscallAction::Errno { errno } = rule.action {
                if !(1..=4095).contains(&errno) {
                    return bad("errno is 1 to 4095".into());
                }
            }
        }
        for rule in &self.network {
            if rule.cidr.is_none() && rule.name.is_none() {
                return bad("a network rule gives an address range, a name, or both".into());
            }
            if let Some(cidr) = &rule.cidr {
                if parse_cidr(cidr).is_none() {
                    return bad(format!("{cidr:?} is not an address or a range"));
                }
            }
            if let Some(name) = &rule.name {
                if !valid_host_pattern(name) {
                    return bad(format!("{name:?} is not a name or *.domain"));
                }
            }
            if let Some(ports) = rule.ports {
                if ports.from > ports.to {
                    return bad("a port range runs from its lower end".into());
                }
            }
        }
        Ok(())
    }

    /// The system-call rules that match a call to `syscall` with `args`: the
    /// first to match decides.
    pub fn syscall_verdict(&self, syscall: &str, args: &[u64; 6]) -> Option<SyscallAction> {
        self.syscalls
            .iter()
            .find(|rule| rule.syscall == syscall && rule.matches(args))
            .map(|rule| rule.action)
    }

    /// Whether a network rule refuses `connection`.
    pub fn refuses(&self, connection: &Connection<'_>) -> bool {
        self.network.iter().any(|rule| rule.matches(connection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> Result<ShieldSpec> {
        ShieldSpec::parse(json)
    }

    #[test]
    fn a_shield_is_checked_before_anything_reads_it() {
        let good = r#"{"name":"cve-x","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":1}}],"expiresAt":10}"#;
        assert!(spec(good).is_ok());
        // A shield of hook rules alone is a shield.
        let hooks =
            spec(r#"{"name":"no-mount","hooks":[{"hook":"sb_mount","errno":1}],"expiresAt":10}"#)
                .unwrap();
        assert_eq!(hooks.hooks[0].hook, "sb_mount");
        for bad in [
            // No rule.
            r#"{"name":"a","expiresAt":10}"#,
            // A field no version knows.
            r#"{"name":"a","extra":1,"syscalls":[{"syscall":"ptrace","action":{"kind":"kill"}}],"expiresAt":10}"#,
            // Names.
            r#"{"name":"A","syscalls":[{"syscall":"ptrace","action":{"kind":"kill"}}],"expiresAt":10}"#,
            r#"{"name":"-a","syscalls":[{"syscall":"ptrace","action":{"kind":"kill"}}],"expiresAt":10}"#,
            r#"{"name":"a","syscalls":[{"syscall":"Ptrace","action":{"kind":"kill"}}],"expiresAt":10}"#,
            // Arguments and errno.
            r#"{"name":"a","syscalls":[{"syscall":"ptrace","args":[{"index":6,"op":"eq","value":0}],"action":{"kind":"kill"}}],"expiresAt":10}"#,
            r#"{"name":"a","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":0}}],"expiresAt":10}"#,
            r#"{"name":"a","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":4096}}],"expiresAt":10}"#,
            // Network rules.
            r#"{"name":"a","network":[{"ports":{"from":1,"to":2}}],"expiresAt":10}"#,
            r#"{"name":"a","network":[{"cidr":"10.0.0.0/33"}],"expiresAt":10}"#,
            r#"{"name":"a","hooks":[{"hook":"not_a_hook","errno":1}],"expiresAt":10}"#,
            r#"{"name":"a","hooks":[{"hook":"socket_create","errno":0}],"expiresAt":10}"#,
            r#"{"name":"a","hooks":[{"hook":"socket_create","errno":4096}],"expiresAt":10}"#,
            r#"{"name":"a","hooks":[{"hook":"socket_create","errno":1,"kill":true}],"expiresAt":10}"#,
            r#"{"name":"a","network":[{"cidr":"10.0.0.0/"}],"expiresAt":10}"#,
            r#"{"name":"a","network":[{"name":"a..b"}],"expiresAt":10}"#,
            r#"{"name":"a","network":[{"cidr":"10.0.0.1","ports":{"from":2,"to":1}}],"expiresAt":10}"#,
            // Scope.
            r#"{"name":"a","scope":{"kind":"sandboxes","ids":[]},"network":[{"cidr":"10.0.0.1"}],"expiresAt":10}"#,
        ] {
            assert!(spec(bad).is_err(), "{bad}");
        }
        let long = format!(
            r#"{{"name":"a","description":"{}"}}"#,
            "x".repeat(MAX_SPEC_BYTES)
        );
        assert!(spec(&long).is_err());
    }

    #[test]
    fn arguments_are_tested_as_registers() {
        let test = |op, value, mask| ArgMatch {
            index: 1,
            op,
            value,
            mask,
        };
        let args = [0, 0xffff_ffff_0000_0010, 0, 0, 0, 0];
        assert!(test(ArgOp::Eq, 0x10, 0xffff_ffff).holds(&args));
        assert!(!test(ArgOp::Eq, 0x10, u64::MAX).holds(&args));
        assert!(test(ArgOp::AnyBits, 0x30, u64::MAX).holds(&args));
        assert!(!test(ArgOp::AllBits, 0x30, u64::MAX).holds(&args));
        assert!(test(ArgOp::Gt, 0xf, 0xff).holds(&args));
        assert!(test(ArgOp::Le, 0x10, 0xff).holds(&args));
        assert!(!test(ArgOp::Lt, 0x10, 0xff).holds(&args));
        assert!(test(ArgOp::Ne, 0x11, 0xff).holds(&args));
        let rule = SyscallRule {
            syscall: "ptrace".into(),
            args: vec![test(ArgOp::Eq, 0x10, 0xff), test(ArgOp::Eq, 1, u64::MAX)],
            action: SyscallAction::Kill,
        };
        assert!(!rule.matches(&args), "every test must hold");
    }

    #[test]
    fn network_rules_match_what_they_name_and_nothing_else() {
        let conn = |address: &str, port, name| Connection {
            address: Some(address.parse().unwrap()),
            port,
            name,
            protocol: Protocol::Tcp,
        };
        let rule: NetworkRule = serde_json::from_str(
            r#"{"cidr":"203.0.113.0/24","ports":{"from":443,"to":443},"protocol":"tcp"}"#,
        )
        .unwrap();
        assert!(rule.matches(&conn("203.0.113.9", 443, None)));
        assert!(rule.matches(&conn("::ffff:203.0.113.9", 443, None)));
        assert!(!rule.matches(&conn("203.0.114.9", 443, None)));
        assert!(!rule.matches(&conn("203.0.113.9", 80, None)));
        let mut udp = conn("203.0.113.9", 443, None);
        udp.protocol = Protocol::Udp;
        assert!(!rule.matches(&udp));

        let named: NetworkRule = serde_json::from_str(r#"{"name":"*.evil.example"}"#).unwrap();
        assert!(named.matches(&conn("192.0.2.1", 1, Some("a.evil.example"))));
        assert!(named.matches(&conn("192.0.2.1", 1, Some("A.B.Evil.Example."))));
        assert!(!named.matches(&conn("192.0.2.1", 1, Some("evil.example"))));
        assert!(!named.matches(&conn("192.0.2.1", 1, Some("notevil.example"))));
        assert!(
            !named.matches(&conn("192.0.2.1", 1, None)),
            "no name, no match"
        );

        let everything: NetworkRule = serde_json::from_str(r#"{"cidr":"0.0.0.0/0"}"#).unwrap();
        assert!(everything.matches(&conn("198.51.100.1", 1, None)));
        let v6: NetworkRule = serde_json::from_str(r#"{"cidr":"2001:db8::/32"}"#).unwrap();
        assert!(v6.matches(&conn("2001:db8::1", 1, None)));
        assert!(!v6.matches(&conn("2001:db9::1", 1, None)));
    }

    #[test]
    fn the_first_matching_rule_decides() {
        let shield = spec(
            r#"{"name":"a","syscalls":[
                {"syscall":"ptrace","args":[{"index":0,"op":"eq","value":16}],"action":{"kind":"kill"}},
                {"syscall":"ptrace","action":{"kind":"errno","errno":1}}
            ],"expiresAt":10}"#,
        )
        .unwrap();
        assert_eq!(
            shield.syscall_verdict("ptrace", &[16, 0, 0, 0, 0, 0]),
            Some(SyscallAction::Kill)
        );
        assert_eq!(
            shield.syscall_verdict("ptrace", &[0; 6]),
            Some(SyscallAction::Errno { errno: 1 })
        );
        assert_eq!(shield.syscall_verdict("bpf", &[0; 6]), None);
        assert!(Scope::All.covers("any"));
        assert!(!Scope::Sandboxes {
            ids: vec!["a".into()]
        }
        .covers("b"));
    }
}
