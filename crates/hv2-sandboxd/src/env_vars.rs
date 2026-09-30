//! E2B's `envVars`: environment variables for every command a sandbox runs.
//!
//! They go where a template's own `ENV` already lives -- the guest's
//! [`TEMPLATE_DEFAULTS_PATH`], which the guest agent reads at every `Exec`
//! and `Start` -- layered over the template's variables and under a
//! command's own. Kept in the guest rather than on the host, so a pause, a
//! resume on another node, a fork and a snapshot all carry them with the
//! rest of the guest's memory, and nothing on the host can drift from it.
//!
//! Write-only, as in E2B: no response returns them, so a secret passed here
//! is readable inside its sandbox and nowhere else.

use std::collections::BTreeMap;
use std::time::Duration;

use hv2_agent::AgentVM;
use hv2_guest_agent::{TemplateDefaults, TEMPLATE_DEFAULTS_PATH};

/// How long the guest has to read or write its defaults.
const AGENT_TIMEOUT: Duration = Duration::from_secs(10);
/// At most this many variables.
pub(crate) const MAX_VARS: usize = 1024;
/// At most this many bytes of names and values together: what the guest
/// reads at every command start, so it is kept small.
pub(crate) const MAX_BYTES: usize = 128 * 1024;

/// Refuse what cannot be an environment variable, or is too much of one.
pub(crate) fn validate(env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > MAX_VARS {
        return Err(format!(
            "envVars has {} variables; at most {MAX_VARS}",
            env.len()
        ));
    }
    let mut bytes = 0usize;
    for (name, value) in env {
        if name.is_empty() || name.contains('=') || name.contains('\0') {
            return Err(format!(
                "envVars: {name:?} is not a variable name (empty, or holds '=' or NUL)"
            ));
        }
        if value.contains('\0') {
            return Err(format!("envVars: the value of {name} holds a NUL byte"));
        }
        bytes += name.len() + value.len();
    }
    if bytes > MAX_BYTES {
        return Err(format!("envVars is {bytes} bytes; at most {MAX_BYTES}"));
    }
    Ok(())
}

/// The defaults a guest should hold: `current`'s, with `env` over its
/// variables. `current` is `None` for a template not built by steps, which
/// has no defaults file.
pub(crate) fn merged(
    current: Option<&[u8]>,
    env: &BTreeMap<String, String>,
) -> Result<Vec<u8>, String> {
    let mut defaults: TemplateDefaults = match current {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|e| format!("the guest's {TEMPLATE_DEFAULTS_PATH} is not readable: {e}"))?,
        None => TemplateDefaults::default(),
    };
    defaults
        .env
        .extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
    serde_json::to_vec(&defaults).map_err(|e| e.to_string())
}

/// Whether a guest's read failure is its file not existing, which is the one
/// failure that means "no defaults" rather than "could not tell".
///
/// Anything else fails the create: taking an unreadable file for an absent
/// one would overwrite a template's `ENV`, `WORKDIR` and `USER` with nothing.
fn is_absent(error: &str) -> bool {
    error.contains("(os error 2)")
}

/// Give `vm`'s guest `env`, over its template's variables.
pub(crate) async fn apply(vm: &AgentVM, env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.is_empty() {
        return Ok(());
    }
    let current = match vm
        .read_file_in_guest(TEMPLATE_DEFAULTS_PATH, 1 << 20, AGENT_TIMEOUT)
        .await
    {
        Ok(bytes) => Some(bytes),
        Err(e) if is_absent(&e.to_string()) => None,
        Err(e) => return Err(format!("reading the template's defaults: {e}")),
    };
    let bytes = merged(current.as_deref(), env)?;
    vm.write_file_in_guest(TEMPLATE_DEFAULTS_PATH, bytes, AGENT_TIMEOUT)
        .await
        .map_err(|e| format!("setting envVars: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn sandbox_variables_go_over_the_templates_and_keep_the_rest() {
        let template = TemplateDefaults {
            env: env(&[("PATH", "/usr/bin"), ("MODE", "template")]),
            cwd: Some("/app".into()),
            user: Some("node".into()),
        };
        let bytes = serde_json::to_vec(&template).unwrap();
        let out = merged(Some(&bytes), &env(&[("MODE", "sandbox"), ("TOKEN", "t")])).unwrap();
        let out: TemplateDefaults = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            out.env,
            env(&[("MODE", "sandbox"), ("PATH", "/usr/bin"), ("TOKEN", "t")])
        );
        assert_eq!(out.cwd.as_deref(), Some("/app"));
        assert_eq!(out.user.as_deref(), Some("node"));
    }

    #[test]
    fn a_template_without_defaults_gets_only_the_sandboxs() {
        let out = merged(None, &env(&[("A", "1")])).unwrap();
        let out: TemplateDefaults = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            out,
            TemplateDefaults {
                env: env(&[("A", "1")]),
                ..TemplateDefaults::default()
            }
        );
    }

    #[test]
    fn an_unreadable_defaults_file_is_an_error_not_an_empty_one() {
        assert!(merged(Some(b"not json"), &env(&[("A", "1")])).is_err());
        assert!(is_absent(
            "guest refused: reading /etc/hv2/defaults.json: No such file or directory (os error 2)"
        ));
        assert!(!is_absent("guest did not answer within 10s"));
        assert!(!is_absent(
            "reading /etc/hv2/defaults.json: Permission denied (os error 13)"
        ));
    }

    #[test]
    fn names_and_sizes_are_checked() {
        assert!(validate(&env(&[("GOOD_NAME", "any value = fine")])).is_ok());
        assert!(validate(&env(&[("", "x")])).is_err());
        assert!(validate(&env(&[("A=B", "x")])).is_err());
        assert!(validate(&env(&[("A\0", "x")])).is_err());
        assert!(validate(&env(&[("A", "x\0y")])).is_err());

        let many: BTreeMap<String, String> = (0..=MAX_VARS)
            .map(|i| (format!("V{i}"), String::new()))
            .collect();
        assert!(validate(&many).is_err());
        let big = env(&[("BIG", &"x".repeat(MAX_BYTES))]);
        assert!(validate(&big).is_err());
    }
}
