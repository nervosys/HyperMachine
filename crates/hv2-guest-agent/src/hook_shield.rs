//! Hook shields on the wire: what the host tells the in-guest agent to refuse
//! at a Linux security (BPF LSM) hook, and what the agent reports it matched.
//!
//! Self-contained, like [`crate::syscall_shield`]: this crate is linked into the
//! guest binary and must not depend on `hv2-shield`. The host translates
//! (`hv2-sandboxd`); the semantics are the same as the node's:
//!
//! - rules arrive as one flat list, in the order shields are in force;
//! - a hook rule tests nothing but the hook: every call that reaches it,
//!   from any process in the guest except the agent, matches;
//! - across shields naming the same hook, the first with `enforce` set
//!   decides (its `errno` is the one returned); a shield without it only has
//!   its match counted.
//!
//! The kernel counts, not the agent: a hook can be asked millions of times,
//! so a hit is "N matches of this hook since the last read", not one record
//! per call. The list REPLACES the agent's previous list: it is a state, not
//! a delta.

use serde::{Deserialize, Serialize};

use crate::syscall_shield::MAX_RULE_TEXT;

/// Most rules one [`crate::Operation::SetHookShields`] may carry: the sum over
/// every shield in force.
pub const MAX_HOOK_RULES: usize = 4096;

/// How many hits the agent keeps between two [`crate::Operation::HookShieldHits`]
/// reads. Beyond it, hits are counted in `dropped` and lost.
pub const HOOK_HIT_LIMIT: usize = 1024;

/// One hook rule, as the agent applies it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuestHookRule {
    /// The shield's name, for the hit.
    pub shield: String,
    /// The shield's digest. Rules of one shield are contiguous in the list.
    pub digest: String,
    /// The hook's name, one of the agent's own list (`hv2_shield_bpf::HOOKS`).
    /// The wire does not know that list; the agent checks it.
    pub hook: String,
    /// The operation fails with this errno, 1 to 4095.
    pub errno: i32,
    /// Whether the shield is enforced. If not, a match is counted and the
    /// operation runs.
    #[serde(default)]
    pub enforce: bool,
    /// Unix seconds, guest clock, after which the agent drops this rule on
    /// its own: a backstop for a host that stops talking. 0: no backstop.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub expires_at: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// Matches of one hook for one shield, since the last read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuestHookHit {
    /// Guest clock, unix seconds, when the agent read the count. Advisory:
    /// the host stamps its own time.
    pub at: u64,
    pub shield: String,
    pub digest: String,
    pub hook: String,
    /// Refused (true) or only counted (false).
    pub enforced: bool,
    /// How many matches this hit stands for, at least 1.
    pub count: u64,
}

/// Check a rule list before any of it is applied.
///
/// # Errors
///
/// Says why the whole list is refused: too many rules, an empty or oversized
/// string, an errno outside 1 to 4095. Whether a hook is one the agent can
/// attach is the agent's to check.
pub fn validate_hook_rules(rules: &[GuestHookRule]) -> Result<(), String> {
    if rules.len() > MAX_HOOK_RULES {
        return Err(format!(
            "refusing {} hook rules: at most {MAX_HOOK_RULES}",
            rules.len()
        ));
    }
    for (i, rule) in rules.iter().enumerate() {
        for (field, text) in [
            ("shield", &rule.shield),
            ("digest", &rule.digest),
            ("hook", &rule.hook),
        ] {
            if text.is_empty() || text.len() > MAX_RULE_TEXT {
                return Err(format!(
                    "hook rule {i}: {field} must be 1 to {MAX_RULE_TEXT} bytes"
                ));
            }
        }
        if !(1..=4095).contains(&rule.errno) {
            return Err(format!(
                "hook rule {i}: errno {} is not 1 to 4095",
                rule.errno
            ));
        }
    }
    Ok(())
}

/// Credit the matches the kernel counted for `hook` since the last read to the
/// shields that name it, at guest time `now`. Pure; the agent and the tests
/// share it.
///
/// `rules` are the ones that were in force while the matches happened. Those
/// expired at `now` are skipped. Among the rest naming `hook`, the decider is
/// the first with `enforce` set: it takes the refusals as `enforced: true`.
/// Every other shield naming the hook records the same refusals as not
/// enforced, and every shield naming it records the monitored matches as not
/// enforced: as with syscalls, every matching shield records, and only the
/// decider records a refusal. A refusal with no decider (the only enforcing
/// rule expired since the engine was set) is credited to nobody.
///
/// One hit per shield digest and `enforced`; a shield with several rules
/// naming the hook is counted once.
#[must_use]
pub fn attribute(
    rules: &[GuestHookRule],
    hook: &str,
    new_monitored: u64,
    new_refused: u64,
    now: u64,
) -> Vec<GuestHookHit> {
    if new_monitored == 0 && new_refused == 0 {
        return Vec::new();
    }
    let naming: Vec<&GuestHookRule> = rules
        .iter()
        .filter(|r| r.hook == hook && (r.expires_at == 0 || r.expires_at > now))
        .collect();
    let decider = naming.iter().find(|r| r.enforce).map(|r| r.digest.as_str());
    let mut hits: Vec<GuestHookHit> = Vec::new();
    let mut add = |rule: &GuestHookRule, enforced: bool, count: u64| {
        if count == 0 {
            return;
        }
        match hits
            .iter_mut()
            .find(|h| h.digest == rule.digest && h.enforced == enforced)
        {
            Some(hit) => hit.count = hit.count.saturating_add(count),
            None => hits.push(GuestHookHit {
                at: now,
                shield: rule.shield.clone(),
                digest: rule.digest.clone(),
                hook: hook.to_string(),
                enforced,
                count,
            }),
        }
    };
    // A shield with two rules naming the hook is credited once: the decider's
    // own second rule must not add a second "not enforced" refusal.
    let mut seen: Vec<&str> = Vec::new();
    for rule in naming {
        if seen.contains(&rule.digest.as_str()) {
            continue;
        }
        seen.push(rule.digest.as_str());
        if decider.is_some() {
            add(rule, decider == Some(rule.digest.as_str()), new_refused);
        }
        add(rule, false, new_monitored);
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(digest: &str, hook: &str, errno: i32, enforce: bool) -> GuestHookRule {
        GuestHookRule {
            shield: format!("shield-{digest}"),
            digest: digest.into(),
            hook: hook.into(),
            errno,
            enforce,
            expires_at: 0,
        }
    }

    fn summary(hits: &[GuestHookHit]) -> Vec<(&str, bool, u64)> {
        hits.iter()
            .map(|h| (h.digest.as_str(), h.enforced, h.count))
            .collect()
    }

    #[test]
    fn only_the_decider_records_a_refusal_and_every_shield_records_a_match() {
        let rules = [
            rule("a", "sb_mount", 1, false),
            rule("b", "sb_mount", 13, true),
            rule("c", "sb_mount", 99, true),
            rule("d", "socket_create", 1, true),
        ];
        let hits = attribute(&rules, "sb_mount", 0, 4, 10);
        assert_eq!(
            summary(&hits),
            [("a", false, 4), ("b", true, 4), ("c", false, 4)]
        );
        assert!(hits.iter().all(|h| h.hook == "sb_mount" && h.at == 10));
        assert_eq!(hits[1].shield, "shield-b");

        let hits = attribute(&rules, "sb_mount", 3, 0, 10);
        assert_eq!(
            summary(&hits),
            [("a", false, 3), ("b", false, 3), ("c", false, 3)]
        );

        // Both at once: a shield that is not the decider adds them up.
        let hits = attribute(&rules, "sb_mount", 3, 4, 10);
        assert_eq!(
            summary(&hits),
            [
                ("a", false, 7),
                ("b", true, 4),
                ("b", false, 3),
                ("c", false, 7)
            ]
        );
    }

    #[test]
    fn nothing_new_is_nothing_and_an_unnamed_hook_is_nobody() {
        let rules = [rule("a", "sb_mount", 1, true)];
        assert!(attribute(&rules, "sb_mount", 0, 0, 1).is_empty());
        assert!(attribute(&rules, "socket_create", 5, 5, 1).is_empty());
    }

    #[test]
    fn a_shield_with_two_rules_on_one_hook_is_counted_once() {
        let rules = [
            rule("a", "sb_mount", 1, true),
            rule("a", "sb_mount", 2, true),
        ];
        assert_eq!(
            summary(&attribute(&rules, "sb_mount", 0, 2, 1)),
            [("a", true, 2)]
        );
    }

    #[test]
    fn an_expired_rule_neither_decides_nor_records() {
        let mut first = rule("a", "sb_mount", 1, true);
        first.expires_at = 100;
        let rules = [first, rule("b", "sb_mount", 1, true)];
        assert_eq!(
            summary(&attribute(&rules, "sb_mount", 0, 1, 99)),
            [("a", true, 1), ("b", false, 1)]
        );
        assert_eq!(
            summary(&attribute(&rules, "sb_mount", 0, 1, 100)),
            [("b", true, 1)]
        );
        // No enforcing rule is left: a refusal is credited to nobody.
        let mut only = rule("a", "sb_mount", 1, true);
        only.expires_at = 5;
        assert!(attribute(&[only], "sb_mount", 0, 1, 5).is_empty());
    }

    #[test]
    fn validation_refuses_what_the_agent_could_not_apply() {
        assert!(validate_hook_rules(&[rule("a", "sb_mount", 1, true)]).is_ok());
        assert!(validate_hook_rules(&[rule("a", "sb_mount", 0, true)]).is_err());
        assert!(validate_hook_rules(&[rule("a", "sb_mount", 4096, true)]).is_err());
        assert!(validate_hook_rules(&[rule("", "sb_mount", 1, true)]).is_err());
        assert!(validate_hook_rules(&[rule("a", "", 1, true)]).is_err());
        let long = "h".repeat(MAX_RULE_TEXT + 1);
        assert!(validate_hook_rules(&[rule("a", &long, 1, true)]).is_err());
        let many = vec![rule("a", "sb_mount", 1, true); MAX_HOOK_RULES + 1];
        assert!(validate_hook_rules(&many).is_err());
    }
}
