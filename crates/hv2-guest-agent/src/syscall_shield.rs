//! Syscall shields on the wire: what the host tells the in-guest agent to
//! refuse, and what the agent reports it matched.
//!
//! These are self-contained copies of the shape in `hv2-shield`'s
//! `SyscallRule`/`ArgMatch`, on purpose: this crate is linked into the guest
//! binary and must not depend on `hv2-shield`, which pulls in the node's
//! crypto. The host translates (`hv2-sandboxd`); the semantics are the same:
//!
//! - rules arrive as one flat list, in the order shields are in force;
//! - within one shield (one `digest`), the FIRST rule whose syscall and
//!   argument tests match is the shield's answer for that call;
//! - across shields, the first shield with `enforce` set whose answer exists
//!   decides the call; a shield without it only has its match recorded;
//! - arguments are registers; memory they point to is never read.
//!
//! The list REPLACES the agent's previous list: it is a state, not a delta.

use serde::{Deserialize, Serialize};

/// Most rules one [`crate::Operation::SetSyscallShields`] may carry. A shield
/// has at most 64; this bounds the sum over every shield in force.
pub const MAX_SYSCALL_RULES: usize = 4096;

/// Longest string field in a rule, in bytes.
pub const MAX_RULE_TEXT: usize = 256;

/// How many hits the agent keeps between two [`crate::Operation::SyscallShieldHits`]
/// reads. Beyond it, hits are counted in `dropped` and lost.
pub const HIT_RING: usize = 1024;

/// One syscall rule, as the agent applies it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuestSyscallRule {
    /// The shield's name, for the hit.
    pub shield: String,
    /// The shield's digest. Rules of one shield are contiguous in the list.
    pub digest: String,
    /// The call's name, one the agent can trap (see
    /// `hv2_sandbox::seccomp_notify::trappable`).
    pub syscall: String,
    /// Every one must hold. None: every call to `syscall` matches.
    #[serde(default)]
    pub args: Vec<GuestArgMatch>,
    pub action: GuestSyscallAction,
    /// Whether the shield is enforced. If not, a match is recorded and the
    /// call runs.
    #[serde(default)]
    pub enforce: bool,
    /// Unix seconds, guest clock, after which the agent ignores this rule on
    /// its own: a backstop for a host that stops talking. 0: no backstop.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub expires_at: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

fn all_bits() -> u64 {
    u64::MAX
}

/// `(arg[index] & mask) <op> value`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuestArgMatch {
    /// 0 to 5.
    pub index: u8,
    pub op: GuestArgOp,
    pub value: u64,
    #[serde(default = "all_bits")]
    pub mask: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuestArgOp {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GuestSyscallAction {
    /// The call fails with this errno, 1 to 4095.
    Errno { errno: i32 },
    /// The calling process is killed.
    Kill,
}

/// One match the agent recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuestHit {
    /// Guest clock, unix seconds. Advisory: the host stamps its own time.
    pub at: u64,
    pub shield: String,
    pub digest: String,
    pub syscall: String,
    pub args: [u64; 6],
    /// Refused (true) or only recorded (false).
    pub enforced: bool,
    /// The calling thread, as the guest numbers it.
    pub pid: u32,
}

impl GuestArgMatch {
    /// Whether `args` passes this test. Same arithmetic as `hv2-shield`'s
    /// `ArgMatch::holds`.
    #[must_use]
    pub fn holds(&self, args: &[u64; 6]) -> bool {
        let Some(&arg) = args.get(usize::from(self.index)) else {
            return false;
        };
        let arg = arg & self.mask;
        match self.op {
            GuestArgOp::Eq => arg == self.value,
            GuestArgOp::Ne => arg != self.value,
            GuestArgOp::Lt => arg < self.value,
            GuestArgOp::Le => arg <= self.value,
            GuestArgOp::Gt => arg > self.value,
            GuestArgOp::Ge => arg >= self.value,
            GuestArgOp::AnyBits => arg & self.value != 0,
            GuestArgOp::AllBits => arg & self.value == self.value,
        }
    }
}

impl GuestSyscallRule {
    /// Whether a call to `syscall` with `args` matches this rule.
    #[must_use]
    pub fn matches(&self, syscall: &str, args: &[u64; 6]) -> bool {
        self.syscall == syscall && self.args.iter().all(|t| t.holds(args))
    }
}

/// What the agent does with a trapped call: the result of
/// [`evaluate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// The action of the first enforced shield that answers, or none.
    pub verdict: Option<GuestSyscallAction>,
    /// One entry per shield that matched: `(shield, digest, enforced)`.
    /// `enforced` is true only for the shield whose action is `verdict`:
    /// a second enforced shield behind it is recorded as not enforced,
    /// exactly as the node's own `Live::syscall` does.
    pub matched: Vec<(String, String, bool)>,
}

/// Evaluate a trapped call against `rules` at guest time `now` (unix
/// seconds). Pure; the agent and the tests share it.
#[must_use]
pub fn evaluate(
    rules: &[GuestSyscallRule],
    syscall: &str,
    args: &[u64; 6],
    now: u64,
) -> Evaluation {
    let mut verdict = None;
    let mut matched: Vec<(String, String, bool)> = Vec::new();
    for rule in rules {
        if rule.expires_at != 0 && rule.expires_at <= now {
            continue;
        }
        // A shield answers once: its first matching rule.
        if matched.iter().any(|(_, digest, _)| *digest == rule.digest) {
            continue;
        }
        if !rule.matches(syscall, args) {
            continue;
        }
        let enforced = rule.enforce && verdict.is_none();
        matched.push((rule.shield.clone(), rule.digest.clone(), enforced));
        if enforced {
            verdict = Some(rule.action);
        }
    }
    Evaluation { verdict, matched }
}

/// Check a rule list before any of it is applied.
///
/// # Errors
///
/// Says why the whole list is refused: too many rules, an empty or oversized
/// string, an argument index above 5, an errno outside 1 to 4095.
pub fn validate_rules(rules: &[GuestSyscallRule]) -> Result<(), String> {
    if rules.len() > MAX_SYSCALL_RULES {
        return Err(format!(
            "refusing {} syscall rules: at most {MAX_SYSCALL_RULES}",
            rules.len()
        ));
    }
    for (i, rule) in rules.iter().enumerate() {
        for (field, text) in [
            ("shield", &rule.shield),
            ("digest", &rule.digest),
            ("syscall", &rule.syscall),
        ] {
            if text.is_empty() || text.len() > MAX_RULE_TEXT {
                return Err(format!(
                    "rule {i}: {field} must be 1 to {MAX_RULE_TEXT} bytes"
                ));
            }
        }
        if rule.args.len() > 6 || rule.args.iter().any(|a| a.index > 5) {
            return Err(format!("rule {i}: argument indexes run 0 to 5"));
        }
        if let GuestSyscallAction::Errno { errno } = rule.action {
            if !(1..=4095).contains(&errno) {
                return Err(format!("rule {i}: errno {errno} is not 1 to 4095"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(digest: &str, syscall: &str, errno: i32, enforce: bool) -> GuestSyscallRule {
        GuestSyscallRule {
            shield: format!("shield-{digest}"),
            digest: digest.into(),
            syscall: syscall.into(),
            args: Vec::new(),
            action: GuestSyscallAction::Errno { errno },
            enforce,
            expires_at: 0,
        }
    }

    #[test]
    fn the_first_enforced_shield_decides_and_monitored_ones_only_record() {
        let rules = [
            rule("a", "ptrace", 1, false),
            rule("b", "ptrace", 13, true),
            rule("c", "ptrace", 99, true),
            rule("d", "bpf", 1, true),
        ];
        let e = evaluate(&rules, "ptrace", &[0; 6], 10);
        assert_eq!(e.verdict, Some(GuestSyscallAction::Errno { errno: 13 }));
        assert_eq!(
            e.matched,
            vec![
                ("shield-a".into(), "a".into(), false),
                ("shield-b".into(), "b".into(), true),
                ("shield-c".into(), "c".into(), false),
            ]
        );
        assert_eq!(evaluate(&rules, "mount", &[0; 6], 10).matched, vec![]);
    }

    #[test]
    fn a_shield_answers_once_with_its_first_matching_rule() {
        let mut first = rule("a", "unshare", 1, true);
        first.args = vec![GuestArgMatch {
            index: 0,
            op: GuestArgOp::AnyBits,
            value: 0x1000_0000,
            mask: u64::MAX,
        }];
        let second = rule("a", "unshare", 2, true);
        let rules = [first, second];
        let e = evaluate(&rules, "unshare", &[0x1000_0000, 0, 0, 0, 0, 0], 1);
        assert_eq!(e.verdict, Some(GuestSyscallAction::Errno { errno: 1 }));
        assert_eq!(e.matched.len(), 1);
        let e = evaluate(&rules, "unshare", &[0; 6], 1);
        assert_eq!(e.verdict, Some(GuestSyscallAction::Errno { errno: 2 }));
    }

    #[test]
    fn an_expired_rule_is_ignored_by_the_agent_on_its_own() {
        let mut r = rule("a", "ptrace", 1, true);
        r.expires_at = 100;
        assert!(evaluate(&[r.clone()], "ptrace", &[0; 6], 99)
            .verdict
            .is_some());
        assert!(evaluate(&[r], "ptrace", &[0; 6], 100).verdict.is_none());
    }

    #[test]
    fn masks_and_ops_match_hv2_shield() {
        let test = |op, value, mask| GuestArgMatch {
            index: 1,
            op,
            value,
            mask,
        };
        let args = [0, 0x1_0000_0030, 0, 0, 0, 0];
        assert!(test(GuestArgOp::Eq, 0x30, 0xffff_ffff).holds(&args));
        assert!(!test(GuestArgOp::Eq, 0x30, u64::MAX).holds(&args));
        assert!(test(GuestArgOp::AllBits, 0x30, u64::MAX).holds(&args));
        assert!(!test(GuestArgOp::AllBits, 0x31, u64::MAX).holds(&args));
        assert!(test(GuestArgOp::Gt, 0x2f, 0xff).holds(&args));
    }

    #[test]
    fn validation_refuses_what_the_agent_could_not_apply() {
        assert!(validate_rules(&[rule("a", "ptrace", 1, true)]).is_ok());
        assert!(validate_rules(&[rule("a", "ptrace", 0, true)]).is_err());
        assert!(validate_rules(&[rule("a", "ptrace", 4096, true)]).is_err());
        assert!(validate_rules(&[rule("", "ptrace", 1, true)]).is_err());
        let mut r = rule("a", "ptrace", 1, true);
        r.args = vec![GuestArgMatch {
            index: 6,
            op: GuestArgOp::Eq,
            value: 0,
            mask: u64::MAX,
        }];
        assert!(validate_rules(&[r]).is_err());
        let many = vec![rule("a", "ptrace", 1, true); MAX_SYSCALL_RULES + 1];
        assert!(validate_rules(&many).is_err());
    }
}
