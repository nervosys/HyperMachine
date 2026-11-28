//! Checking and merging a rule list: the part of `set` that touches no kernel.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::BTreeMap;

use crate::{HookRule, Scope, HOOKS};

/// The most rules one `set` takes.
pub(crate) const MAX_RULES: usize = 4096;

/// What a `(hook index, scope)` key does: `(errno, enforce)`.
pub(crate) type Merged = BTreeMap<(usize, Scope), (i32, bool)>;

/// Validate `rules` against the hooks this kernel has, then merge those that
/// name the same `(hook, scope)`: the key enforces if any of them does, with
/// the errno of the first enforcing rule in order; otherwise it monitors.
///
/// Nothing is changed by an error: this is the whole of what `set` checks
/// before it touches the kernel.
pub(crate) fn merge(rules: &[HookRule], available: &[&str]) -> Result<Merged, String> {
    if rules.len() > MAX_RULES {
        return Err(format!(
            "{} rules is more than the {MAX_RULES} one set takes",
            rules.len()
        ));
    }
    let mut out = Merged::new();
    for (n, rule) in rules.iter().enumerate() {
        let Some(index) = HOOKS.iter().position(|h| *h == rule.hook) else {
            return Err(format!(
                "rule {n}: {:?} is not a hook a shield may name",
                rule.hook
            ));
        };
        if !available.contains(&HOOKS[index]) {
            return Err(format!(
                "rule {n}: this kernel has no BPF LSM hook {:?}",
                rule.hook
            ));
        }
        if !(1..=4095).contains(&rule.errno) {
            return Err(format!(
                "rule {n}: errno {} is outside 1..=4095",
                rule.errno
            ));
        }
        if rule.scope == Scope::Cgroup(0) {
            return Err(format!(
                "rule {n}: cgroup id 0 is not a cgroup (use Scope::Everywhere)"
            ));
        }
        let entry = out
            .entry((index, rule.scope))
            .or_insert((rule.errno, false));
        if rule.enforce && !entry.1 {
            *entry = (rule.errno, true);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(hook: &str, scope: Scope, errno: i32, enforce: bool) -> HookRule {
        HookRule {
            hook: hook.into(),
            scope,
            errno,
            enforce,
        }
    }

    const ALL: &[&str] = HOOKS;

    #[test]
    fn rejects_without_a_partial_result() {
        let ok = rule("bpf", Scope::Everywhere, 1, true);
        for bad in [
            rule("no_such_hook", Scope::Everywhere, 1, true),
            rule("bpf", Scope::Everywhere, 0, true),
            rule("bpf", Scope::Everywhere, 4096, true),
            rule("bpf", Scope::Everywhere, -1, true),
            rule("bpf", Scope::Cgroup(0), 1, true),
        ] {
            assert!(merge(&[ok.clone(), bad], ALL).is_err());
        }
        assert!(merge(std::slice::from_ref(&ok), &["syslog"]).is_err());
        let many = vec![ok; MAX_RULES + 1];
        assert!(merge(&many, ALL).is_err());
        // The limit itself is fine.
        assert_eq!(merge(&many[..MAX_RULES], ALL).unwrap().len(), 1);
    }

    #[test]
    fn merging_follows_the_stated_order() {
        let c = Scope::Cgroup(9);
        let idx = HOOKS.iter().position(|h| *h == "socket_create").unwrap();
        // Monitor then enforce: enforces, with the enforcing rule's errno.
        let m = merge(
            &[
                rule("socket_create", c, 5, false),
                rule("socket_create", c, 13, true),
                rule("socket_create", c, 1, true),
            ],
            ALL,
        )
        .unwrap();
        assert_eq!(m[&(idx, c)], (13, true));
        // Only monitors: stays a monitor, with the first rule's errno.
        let m = merge(
            &[
                rule("socket_create", c, 5, false),
                rule("socket_create", c, 7, false),
            ],
            ALL,
        )
        .unwrap();
        assert_eq!(m[&(idx, c)], (5, false));
        // Different scopes are different keys.
        let m = merge(
            &[
                rule("socket_create", c, 5, true),
                rule("socket_create", Scope::Everywhere, 6, false),
            ],
            ALL,
        )
        .unwrap();
        assert_eq!(m.len(), 2);
    }
}
