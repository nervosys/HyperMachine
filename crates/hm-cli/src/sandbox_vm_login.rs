//! `hm sandbox vm login`: a single sign-on session for the CLI.
//!
//! The control plane's browser sign-in, ending at this machine instead of a
//! browser session: the CLI listens on `127.0.0.1`, opens
//! `/auth/login?cliPort=…&cliChallenge=…`, and the control plane, once the
//! provider has signed the person in, sends the browser on to that listener
//! with a one-time code. The CLI trades code and PKCE verifier for a session
//! token at `/auth/cli-token` -- a code read on its way here is useless
//! without the verifier, which never leaves this process.
//!
//! Tokens are kept per control plane in `sessions.json` beside the CLI's
//! other state (mode 0600 on Unix) and sent as `Authorization: Bearer` when
//! `HV2_API_KEY` is not set. `logout` forgets one; the token itself lasts
//! until it expires, or until its person leaves the control plane's members.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A stored session.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    token: String,
    email: String,
    #[serde(rename = "expiresAt")]
    expires_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Sessions {
    /// By control plane origin.
    sessions: BTreeMap<String, Stored>,
}

fn sessions_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Application Support"))
    } else {
        dirs::home_dir().map(|h| h.join(".config"))
    }?;
    Some(base.join("hypermachine").join("sessions.json"))
}

fn origin(endpoint: &str) -> Result<String> {
    let url = Url::parse(endpoint).context("invalid sandbox endpoint")?;
    Ok(url.origin().ascii_serialization())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn load(path: &std::path::Path) -> Sessions {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Written beside, then renamed over, readable by this user alone.
fn save(path: &std::path::Path, sessions: &Sessions) -> Result<()> {
    let dir = path.parent().context("no directory for the session file")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let temporary = dir.join(format!(".sessions.{}.json", std::process::id()));
    let bytes = serde_json::to_vec_pretty(sessions)?;
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("creating {}", temporary.display()))?;
        std::io::Write::write_all(&mut file, &bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))
}

/// The unexpired session stored for `endpoint`, if any.
pub(crate) fn stored_token(endpoint: &str) -> Option<String> {
    let path = sessions_path()?;
    let origin = origin(endpoint).ok()?;
    load(&path)
        .sessions
        .remove(&origin)
        .filter(|stored| stored.expires_at > now())
        .map(|stored| stored.token)
}

/// `logout`: forget `endpoint`'s session.
pub(crate) fn logout(endpoint: &str) -> Result<Value> {
    let path = sessions_path().context("no home directory to keep sessions in")?;
    let origin = origin(endpoint)?;
    let mut sessions = load(&path);
    let removed = sessions.sessions.remove(&origin).is_some();
    if removed {
        save(&path, &sessions)?;
    }
    Ok(json!({ "endpoint": origin, "signedOut": removed }))
}

fn random_verifier() -> Result<String> {
    let mut bytes = [0u8; 32];
    hv2_core::crypto::FipsCrypto::new(hv2_core::crypto::FipsMode::Enabled)
        .and_then(|crypto| crypto.random_bytes(&mut bytes))
        .map_err(|e| anyhow::anyhow!("no randomness for the sign-in: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn challenge_for(verifier: &str) -> Result<String> {
    let digest = hv2_core::crypto::FipsCrypto::new(hv2_core::crypto::FipsMode::Enabled)
        .and_then(|crypto| crypto.sha256(verifier.as_bytes()))
        .map_err(|e| anyhow::anyhow!("hashing the verifier: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(digest))
}

/// Try to open `url` in a browser; the URL is printed either way.
fn open_browser(url: &str) {
    let opened = if cfg!(windows) {
        // Not `cmd /c start`: cmd would read the URL's `&` as a separator.
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if opened.is_err() {
        eprintln!("(could not open a browser; open the URL above yourself)");
    }
}

/// The `code` a request line carries for `/callback`, if it is one.
fn callback_code(request_line: &str) -> Option<String> {
    let target = request_line.strip_prefix("GET ")?.split(' ').next()?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
}

/// Wait for the browser to arrive at the listener with a code.
async fn wait_for_code(listener: tokio::net::TcpListener, deadline: Instant) -> Result<String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for the browser sign-in");
        }
        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .context("timed out waiting for the browser sign-in")??;
        let mut buffer = vec![0u8; 8192];
        let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let line = request.lines().next().unwrap_or_default();
        let (status, body, code) = match callback_code(line) {
            Some(code) => (
                "200 OK",
                "Signed in. You can close this tab and return to the terminal.",
                Some(code),
            ),
            None => ("404 Not Found", "Not found.", None),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\n\
             Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(code) = code {
            return Ok(code);
        }
    }
}

/// `login`: sign in through the control plane's provider and keep the
/// session for `endpoint`.
pub(crate) async fn login(
    endpoint: &str,
    ca: Option<&std::path::Path>,
    wait: Duration,
    open: bool,
) -> Result<Value> {
    let base = Url::parse(endpoint).context("invalid sandbox endpoint")?;
    let verifier = random_verifier()?;
    let challenge = challenge_for(&verifier)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("listening for the browser on 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let mut url = base.join("/auth/login")?;
    url.query_pairs_mut()
        .append_pair("cliPort", &port.to_string())
        .append_pair("cliChallenge", &challenge);
    eprintln!("Sign in at:\n\n  {url}\n");
    if open {
        open_browser(url.as_str());
    }
    let code = wait_for_code(listener, Instant::now() + wait).await?;

    let ca_pem = ca
        .map(|path| std::fs::read(path).context("could not read API CA certificate"))
        .transpose()?;
    let response = hv2_tls::http_client(ca_pem.as_deref())
        .map_err(|e| anyhow::anyhow!("invalid API CA certificate: {e}"))?
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?
        .post(base.join("/auth/cli-token")?)
        .json(&json!({ "code": code, "verifier": verifier }))
        .send()
        .await
        .context("reaching the control plane")?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        bail!(
            "the control plane refused the sign-in ({status}): {}",
            body["message"].as_str().unwrap_or("no reason given")
        );
    }
    let stored: Stored =
        serde_json::from_value(body).context("the control plane's session answer")?;
    let path = sessions_path().context("no home directory to keep sessions in")?;
    let origin = origin(endpoint)?;
    let mut sessions = load(&path);
    sessions.sessions.insert(origin.clone(), stored.clone());
    save(&path, &sessions)?;
    Ok(json!({
        "endpoint": origin,
        "email": stored.email,
        "expiresAt": stored.expires_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_callback_path_yields_a_code() {
        assert_eq!(
            callback_code("GET /callback?code=hmc1.abc.def HTTP/1.1").as_deref(),
            Some("hmc1.abc.def")
        );
        assert_eq!(callback_code("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(callback_code("POST /callback?code=x HTTP/1.1"), None);
        assert_eq!(callback_code("GET /callback HTTP/1.1"), None);
        assert_eq!(callback_code(""), None);
    }

    #[test]
    fn the_challenge_is_the_verifiers_s256() {
        // RFC 7636 Appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let verifier = random_verifier().unwrap();
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, random_verifier().unwrap());
    }

    #[test]
    fn sessions_are_kept_by_origin_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hypermachine").join("sessions.json");
        let mut sessions = Sessions::default();
        sessions.sessions.insert(
            origin("https://cp.example:8443/some/path").unwrap(),
            Stored {
                token: "hms1.a.b".into(),
                email: "a@example.com".into(),
                expires_at: now() + 60,
            },
        );
        save(&path, &sessions).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.sessions["https://cp.example:8443"].token, "hms1.a.b");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// The whole loop on this machine: a browser stand-in arrives at the
    /// listener, and the code is what the CLI keeps.
    #[tokio::test]
    async fn the_listener_takes_the_code_from_the_callback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(wait_for_code(
            listener,
            Instant::now() + Duration::from_secs(10),
        ));
        let client = reqwest::Client::new();
        let stray = client
            .get(format!("http://127.0.0.1:{port}/favicon.ico"))
            .send()
            .await
            .unwrap();
        assert_eq!(stray.status(), 404);
        let landed = client
            .get(format!("http://127.0.0.1:{port}/callback?code=hmc1.x.y"))
            .send()
            .await
            .unwrap();
        assert_eq!(landed.status(), 200);
        assert_eq!(waiting.await.unwrap().unwrap(), "hmc1.x.y");
    }
}
