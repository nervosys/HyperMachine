//! Operator connection profiles; schedule specs contain only profile names.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::sandbox_vm::Api;
use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profiles {
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    endpoint: String,
    api_key_env: Option<String>,
    ca: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    request_timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

fn selected_profile<'a>(profiles: &'a Profiles, name: &str) -> Result<&'a Profile> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        bail!("invalid connection profile name");
    }
    profiles
        .profiles
        .get(name)
        .context("connection profile not found")
}

fn client(
    profile: &Profile,
    directory: &Path,
    lookup: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<Api> {
    if !(1..=86_401).contains(&profile.request_timeout_secs) {
        bail!("profile request timeout must be 1-86401 seconds");
    }
    let key = if let Some(variable) = &profile.api_key_env {
        let mut chars = variable.bytes();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            bail!("invalid API key environment variable name");
        }
        Some(
            lookup(variable)?
                .filter(|value| !value.is_empty())
                .context("profile API key environment variable is unset or empty")?,
        )
    } else {
        None
    };
    let ca = profile.ca.as_ref().map(|path| directory.join(path));
    Api::with_ca(
        &profile.endpoint,
        profile.request_timeout_secs,
        key,
        ca.as_deref(),
    )
}

/// Validate operator configuration, resolve the selected environment variable
/// and construct the shared authenticated TLS client. No network request is
/// sent, and successful validation does not establish endpoint reachability.
pub fn validate_connection_profile(path: &Path, name: &str) -> Result<()> {
    let profiles: Profiles = serde_json::from_slice(
        &std::fs::read(path).context("reading operator connection profiles")?,
    )
    .context("parsing operator connection profiles")?;
    let profile = selected_profile(&profiles, name)?;
    client(
        profile,
        path.parent().unwrap_or_else(|| Path::new(".")),
        |variable| match std::env::var(variable) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("profile API key environment variable is not UTF-8")
            }
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_use_shared_endpoint_validation_and_require_configured_keys() {
        let mut profile: Profile = serde_json::from_value(
            serde_json::json!({"endpoint":"https://example.test","api_key_env":"JOB_API_KEY"}),
        )
        .unwrap();
        assert!(client(&profile, Path::new("."), |_| Ok(Some("test-key".into()))).is_ok());
        assert!(client(&profile, Path::new("."), |_| Ok(None)).is_err());
        assert!(client(&profile, Path::new("."), |_| Ok(Some(String::new()))).is_err());
        profile.endpoint = "https://user:secret@example.test".into();
        assert!(client(&profile, Path::new("."), |_| Ok(Some("test-key".into()))).is_err());
        profile.endpoint = "https://example.test?token=secret".into();
        assert!(client(&profile, Path::new("."), |_| Ok(Some("test-key".into()))).is_err());
        profile.endpoint = "https://example.test".into();
        profile.request_timeout_secs = 0;
        assert!(client(&profile, Path::new("."), |_| Ok(Some("test-key".into()))).is_err());
    }
}
