//! Bounded host-bound replacement for operator-held secrets.
//!
//! This component grants no network access. Its caller must authenticate the
//! upstream hostname before applying it. It is not yet wired to the gateway.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

const LIMIT: usize = 1024 * 1024;
const PREFIX: &[u8] = b"hms_";
const TOKEN_LENGTH: usize = 68;

/// Operator input; deliberately has no Debug implementation.
pub struct Binding {
    pub placeholder: String,
    pub value: Vec<u8>,
    pub hosts: Vec<String>,
}

impl Drop for Binding {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// A validated policy replaced atomically between substitution operations.
/// Rotation preserves placeholders supplied by the operator; an empty policy
/// revokes every binding. No network or API authorization is implied.
pub struct Store(RwLock<Bindings>);

/// Operator-managed exact sandbox scopes. No implicit fork inheritance.
/// Removed scopes wipe retained stores, including handles in open relays.
pub struct ScopedStores(RwLock<BTreeMap<String, Arc<Store>>>);

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedDocument {
    version: u32,
    sandboxes: Vec<SandboxPolicy>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxPolicy {
    sandbox_id: String,
    bindings: Vec<PolicyBinding>,
}

fn scoped_policy(input: &[u8]) -> io::Result<BTreeMap<String, Bindings>> {
    if input.len() > LIMIT {
        return Err(invalid());
    }
    let document: ScopedDocument = serde_json::from_slice(input).map_err(|_| invalid())?;
    if document.version != 1 || document.sandboxes.len() > 64 {
        return Err(invalid());
    }
    let mut policies = BTreeMap::new();
    for sandbox in document.sandboxes {
        let id = sandbox.sandbox_id;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(invalid());
        }
        let bindings = sandbox
            .bindings
            .into_iter()
            .map(|mut binding| Binding {
                placeholder: std::mem::take(&mut binding.placeholder),
                value: std::mem::take(&mut binding.value).into_bytes(),
                hosts: std::mem::take(&mut binding.hosts),
            })
            .collect();
        if policies.insert(id, Bindings::new(bindings)?).is_some() {
            return Err(invalid());
        }
    }
    Ok(policies)
}

impl ScopedStores {
    /// Linux private-file loading; the immediate directory must be owned 0700,
    /// and the file owned 0600, regular, single-linked and not a symlink.
    #[cfg(target_os = "linux")]
    pub fn from_file(path: &std::path::Path) -> io::Result<Self> {
        Self::from_json(&private_policy_file(path)?)
    }

    #[cfg(target_os = "linux")]
    pub fn rotate_file(&self, path: &std::path::Path) -> io::Result<()> {
        self.rotate_json(&private_policy_file(path)?)
    }
    pub fn from_json(input: &[u8]) -> io::Result<Self> {
        let stores = scoped_policy(input)?
            .into_iter()
            .map(|(id, bindings)| (id, Arc::new(Store(RwLock::new(bindings)))))
            .collect();
        Ok(Self(RwLock::new(stores)))
    }

    pub fn get(&self, sandbox_id: &str) -> Option<Arc<Store>> {
        self.0.read().get(sandbox_id).cloned()
    }

    /// Validate all scopes first. Commit is atomic per sandbox/request, not
    /// across concurrent requests in different sandbox scopes.
    pub fn rotate_json(&self, input: &[u8]) -> io::Result<()> {
        let policies = scoped_policy(input)?;
        let mut stores = self.0.write();
        for (id, store) in stores.iter() {
            if !policies.contains_key(id) {
                *store.0.write() = Bindings(Vec::new());
            }
        }
        stores.retain(|id, _| policies.contains_key(id));
        for (id, bindings) in policies {
            if let Some(store) = stores.get(&id) {
                *store.0.write() = bindings;
            } else {
                stores.insert(id, Arc::new(Store(RwLock::new(bindings))));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn private_policy_file(path: &std::path::Path) -> io::Result<Zeroizing<Vec<u8>>> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if !path.is_absolute() {
        return Err(invalid());
    }
    let parent =
        std::fs::symlink_metadata(path.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let owner = unsafe { libc::geteuid() };
    if !parent.is_dir() || parent.uid() != owner || parent.mode() & 0o777 != 0o700 {
        return Err(invalid());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file()
        || metadata.uid() != owner
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() > LIMIT as u64
    {
        return Err(invalid());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    Ok(bytes)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    version: u32,
    bindings: Vec<PolicyBinding>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyBinding {
    placeholder: String,
    value: String,
    hosts: Vec<String>,
}

impl Drop for PolicyBinding {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

fn parse_policy(input: &[u8]) -> io::Result<Vec<Binding>> {
    if input.len() > LIMIT {
        return Err(invalid());
    }
    let document: PolicyDocument = serde_json::from_slice(input).map_err(|_| invalid())?;
    if document.version != 1 {
        return Err(invalid());
    }
    Ok(document
        .bindings
        .into_iter()
        .map(|mut binding| Binding {
            placeholder: std::mem::take(&mut binding.placeholder),
            value: std::mem::take(&mut binding.value).into_bytes(),
            hosts: std::mem::take(&mut binding.hosts),
        })
        .collect())
}

impl Store {
    /// Load version-1 operator JSON with strict fields and bounded UTF-8 values.
    /// Reading private files and selecting sandbox ownership are caller duties.
    pub fn from_json(input: &[u8]) -> io::Result<Self> {
        Self::new(parse_policy(input)?)
    }

    /// Parse and validate the entire document before replacing active policy.
    pub fn rotate_json(&self, input: &[u8]) -> io::Result<()> {
        self.rotate(parse_policy(input)?)
    }
    /// Whether this exact hostname has an active binding; grants no access.
    pub fn has_host(&self, hostname: &str) -> bool {
        let Ok(name) = host(hostname) else {
            return false;
        };
        self.0
            .read()
            .0
            .iter()
            .any(|binding| binding.hosts.contains(&name))
    }
    pub fn new(bindings: Vec<Binding>) -> io::Result<Self> {
        Ok(Self(RwLock::new(Bindings::new(bindings)?)))
    }

    pub fn rotate(&self, bindings: Vec<Binding>) -> io::Result<()> {
        let validated = Bindings::new(bindings)?;
        *self.0.write() = validated;
        Ok(())
    }

    pub fn replace(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.0.read().replace(authenticated_host, input)
    }

    pub fn replace_basic_auth(
        &self,
        authenticated_host: &str,
        input: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.0.read().replace_basic_auth(authenticated_host, input)
    }

    pub fn replace_query(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.0.read().replace_query(authenticated_host, input)
    }

    pub fn replace_form(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.0.read().replace_form(authenticated_host, input)
    }

    pub fn replace_json(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.0.read().replace_json(authenticated_host, input)
    }

    /// Rewrite a fully buffered, decoded HTTP request under one policy version.
    pub fn rewrite_request(
        &self,
        authenticated_host: &str,
        request: &mut hyper::Request<Vec<u8>>,
    ) -> io::Result<()> {
        self.0.read().rewrite_request(authenticated_host, request)
    }
}

/// Validated bindings. Diagnostics never print placeholders or secret values.
pub struct Bindings(Vec<Binding>);

impl std::fmt::Debug for Bindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bindings")
            .field("count", &self.0.len())
            .finish_non_exhaustive()
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid host-bound secret configuration",
    )
}

fn host(value: &str) -> io::Result<String> {
    if value.len() > 253
        || !value.is_ascii()
        || value.is_empty()
        || !value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err(invalid());
    }
    Ok(value.to_ascii_lowercase())
}

impl Bindings {
    /// Rewrite JSON string values, preserving keys and all other source bytes.
    /// RawValue validates the document without rounding its numeric literals.
    pub fn replace_json(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        host(authenticated_host)?;
        if input.len() > LIMIT {
            return Err(invalid());
        }
        let _: &serde_json::value::RawValue =
            serde_json::from_slice(input).map_err(|_| invalid())?;
        let mut output = Zeroizing::new(Vec::with_capacity(input.len()));
        let mut offset = 0;
        while offset < input.len() {
            if input[offset] != b'"' {
                append_bounded(&mut output, &input[offset..offset + 1])?;
                offset += 1;
                continue;
            }
            let start = offset;
            offset += 1;
            while offset < input.len() {
                if input[offset] == b'\\' {
                    offset += 2;
                } else if input[offset] == b'"' {
                    offset += 1;
                    break;
                } else {
                    offset += 1;
                }
            }
            let literal = &input[start..offset];
            let next = input[offset..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace());
            if next == Some(&b':') {
                append_bounded(&mut output, literal)?;
                continue;
            }
            let decoded =
                Zeroizing::new(serde_json::from_slice::<String>(literal).map_err(|_| invalid())?);
            let replaced = Zeroizing::new(self.replace(authenticated_host, decoded.as_bytes())?);
            if replaced.as_slice() == decoded.as_bytes() {
                append_bounded(&mut output, literal)?;
            } else {
                let text = std::str::from_utf8(&replaced).map_err(|_| invalid())?;
                let encoded = Zeroizing::new(serde_json::to_vec(text).map_err(|_| invalid())?);
                append_bounded(&mut output, &encoded)?;
            }
        }
        Ok(std::mem::take(&mut *output))
    }
    /// The caller has authenticated the upstream, decoded transfer framing and
    /// bounded body collection. Compressed bodies and trailers are refused.
    /// Host/routing and hop-by-hop headers are never substituted.
    pub fn rewrite_request(
        &self,
        authenticated_host: &str,
        request: &mut hyper::Request<Vec<u8>>,
    ) -> io::Result<()> {
        use hyper::header::{CONTENT_LENGTH, HOST, HeaderValue};
        let name = host(authenticated_host)?;
        let authority = request
            .headers()
            .get(HOST)
            .ok_or_else(invalid)?
            .to_str()
            .map_err(|_| invalid())?;
        let hostname = if let Some((hostname, port)) = authority.rsplit_once(':') {
            let port = port.parse::<u16>().map_err(|_| invalid())?;
            if port == 0 {
                return Err(invalid());
            }
            hostname
        } else {
            authority
        };
        if host(hostname)? != name
            || request
                .uri()
                .host()
                .is_some_and(|h| host(h).ok().as_deref() != Some(&name))
        {
            return Err(invalid());
        }
        if request.headers().contains_key("transfer-encoding")
            || request.headers().contains_key("trailer")
            || request
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(invalid());
        }
        let mut headers = request.headers().clone();
        let mut header_bytes = 0usize;
        for (key, value) in headers.iter_mut() {
            if matches!(
                key.as_str(),
                "host"
                    | "content-length"
                    | "connection"
                    | "upgrade"
                    | "te"
                    | "proxy-authorization"
                    | "proxy-connection"
            ) {
                continue;
            }
            let replaced = Zeroizing::new(
                if key.as_str() == "authorization"
                    && value
                        .as_bytes()
                        .get(..6)
                        .is_some_and(|v| v.eq_ignore_ascii_case(b"Basic "))
                {
                    self.replace_basic_auth(&name, value.as_bytes())?
                } else {
                    self.replace(&name, value.as_bytes())?
                },
            );
            header_bytes += key.as_str().len() + replaced.len();
            if header_bytes > LIMIT {
                return Err(invalid());
            }
            *value = HeaderValue::from_bytes(&replaced).map_err(|_| invalid())?;
        }
        let mut uri = request.uri().clone();
        if let Some(query) = uri.query() {
            let replaced = Zeroizing::new(self.replace_query(&name, query.as_bytes())?);
            let rewritten = format!(
                "{}?{}",
                uri.path(),
                std::str::from_utf8(&replaced).map_err(|_| invalid())?
            );
            let mut parts = uri.into_parts();
            parts.path_and_query = Some(rewritten.parse().map_err(|_| invalid())?);
            uri = hyper::Uri::from_parts(parts).map_err(|_| invalid())?;
        }
        let is_form = headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            });
        let body = if is_form {
            self.replace_form(&name, request.body())?
        } else if headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                let media = value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                media == "application/json"
                    || (media.starts_with("application/") && media.ends_with("+json"))
            })
        {
            self.replace_json(&name, request.body())?
        } else {
            self.replace(&name, request.body())?
        };
        headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
        // Commit only after all fields and new framing have validated.
        *request.headers_mut() = headers;
        *request.uri_mut() = uri;
        *request.body_mut() = body;
        Ok(())
    }
    /// Rewrite ampersand-separated query names/values, decoding percent escapes
    /// exactly once. '+' remains literal (this is not form-body decoding).
    /// Unchanged components retain their byte representation.
    pub fn replace_query(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.replace_parameters(authenticated_host, input, false)
    }

    /// Form encoding treats plus as space. Rewritten components percent-encode
    /// secret delimiters; unchanged components retain their original bytes.
    pub fn replace_form(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        self.replace_parameters(authenticated_host, input, true)
    }

    fn replace_parameters(
        &self,
        authenticated_host: &str,
        input: &[u8],
        form: bool,
    ) -> io::Result<Vec<u8>> {
        host(authenticated_host)?;
        if input.len() > LIMIT {
            return Err(invalid());
        }
        let mut output = Zeroizing::new(Vec::with_capacity(input.len()));
        for (index, parameter) in input.split(|c| *c == b'&').enumerate() {
            if index > 0 {
                append_bounded(&mut output, b"&")?;
            }
            if let Some(separator) = parameter.iter().position(|c| *c == b'=') {
                self.query_component(
                    authenticated_host,
                    &parameter[..separator],
                    &mut output,
                    form,
                )?;
                append_bounded(&mut output, b"=")?;
                self.query_component(
                    authenticated_host,
                    &parameter[separator + 1..],
                    &mut output,
                    form,
                )?;
            } else {
                self.query_component(authenticated_host, parameter, &mut output, form)?;
            }
        }
        Ok(std::mem::take(&mut *output))
    }

    fn query_component(
        &self,
        hostname: &str,
        input: &[u8],
        output: &mut Vec<u8>,
        form: bool,
    ) -> io::Result<()> {
        let mut decoded = Zeroizing::new(Vec::with_capacity(input.len()));
        let mut offset = 0;
        while offset < input.len() {
            if input[offset] == b'%' {
                let bytes = input.get(offset + 1..offset + 3).ok_or_else(invalid)?;
                let high = hex(bytes[0]).ok_or_else(invalid)?;
                let low = hex(bytes[1]).ok_or_else(invalid)?;
                decoded.push(high * 16 + low);
                offset += 3;
            } else {
                decoded.push(if form && input[offset] == b'+' {
                    b' '
                } else {
                    input[offset]
                });
                offset += 1;
            }
        }
        let replaced = Zeroizing::new(self.replace(hostname, &decoded)?);
        if *decoded == *replaced {
            return append_bounded(output, input);
        }
        const DIGITS: &[u8] = b"0123456789ABCDEF";
        for byte in replaced.iter().copied() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                append_bounded(output, &[byte])?;
            } else {
                append_bounded(
                    output,
                    &[
                        b'%',
                        DIGITS[(byte >> 4) as usize],
                        DIGITS[(byte & 15) as usize],
                    ],
                )?;
            }
        }
        Ok(())
    }
    /// Substitute within a Basic authorization value. Other schemes pass
    /// unchanged. Decoded temporary credentials are wiped on all return paths.
    pub fn replace_basic_auth(
        &self,
        authenticated_host: &str,
        input: &[u8],
    ) -> io::Result<Vec<u8>> {
        host(authenticated_host)?;
        if input.len() > LIMIT {
            return Err(invalid());
        }
        let Some(separator) = input.iter().position(|c| *c == b' ') else {
            return Ok(input.to_vec());
        };
        let (scheme, encoded) = (&input[..separator], &input[separator + 1..]);
        if !scheme.eq_ignore_ascii_case(b"Basic") {
            return Ok(input.to_vec());
        }
        let decoded = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| invalid())?);
        if !decoded.contains(&b':') {
            return Err(invalid());
        }
        let replaced = Zeroizing::new(self.replace(authenticated_host, &decoded)?);
        if *decoded == *replaced {
            return Ok(input.to_vec());
        }
        // Check encoded length before creating a potentially larger buffer.
        if replaced.len().div_ceil(3) * 4 + 6 > LIMIT {
            return Err(invalid());
        }
        let encoded = Zeroizing::new(STANDARD.encode(&*replaced));
        let mut output = Vec::with_capacity(encoded.len() + 6);
        output.extend_from_slice(b"Basic ");
        output.extend_from_slice(encoded.as_bytes());
        Ok(output)
    }
    /// Validate exact hostname scopes, unique opaque placeholders and bounds.
    pub fn new(mut bindings: Vec<Binding>) -> io::Result<Self> {
        if bindings.len() > 128 {
            return Err(invalid());
        }
        let mut tokens = HashSet::new();
        let mut total = 0usize;
        for binding in &mut bindings {
            let token = binding.placeholder.as_bytes();
            if token.len() != TOKEN_LENGTH
                || !token.starts_with(PREFIX)
                || !token[PREFIX.len()..]
                    .iter()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
                || !tokens.insert(binding.placeholder.clone())
                || binding.value.is_empty()
                || binding.value.len() > 65536
                || binding.hosts.is_empty()
                || binding.hosts.len() > 64
            {
                return Err(invalid());
            }
            total += binding.value.len();
            if total > LIMIT {
                return Err(invalid());
            }
            let mut names = HashSet::new();
            for name in &mut binding.hosts {
                *name = host(name)?;
                if !names.insert(name.clone()) {
                    return Err(invalid());
                }
            }
        }
        Ok(Self(bindings))
    }

    /// Replace raw placeholders once, only within the authenticated host scope.
    /// Encoded query/Basic-auth forms and streaming framing belong to the caller.
    pub fn replace(&self, authenticated_host: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        let name = host(authenticated_host)?;
        if input.len() > LIMIT {
            return Err(invalid());
        }
        let eligible: Vec<_> = self.0.iter().filter(|b| b.hosts.contains(&name)).collect();
        // Failed expansions must wipe any secret bytes already inserted.
        // Successful output transfers ownership to the HTTP framing caller.
        let mut output = Zeroizing::new(Vec::with_capacity(input.len()));
        let mut offset = 0;
        while offset < input.len() {
            let matching = if input[offset..].starts_with(PREFIX) {
                eligible
                    .iter()
                    .find(|b| input[offset..].starts_with(b.placeholder.as_bytes()))
            } else {
                None
            };
            if let Some(binding) = matching {
                if output.len() + binding.value.len() > LIMIT {
                    return Err(invalid());
                }
                output.extend_from_slice(&binding.value);
                offset += TOKEN_LENGTH;
            } else {
                if output.len() == LIMIT {
                    return Err(invalid());
                }
                output.push(input[offset]);
                offset += 1;
            }
        }
        Ok(std::mem::take(&mut *output))
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    if output.len() + bytes.len() > LIMIT {
        return Err(invalid());
    }
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn private_policy_file_refuses_links_and_unsafe_modes() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = std::env::temp_dir().join(format!(
            "hm-secret-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("policy.json");
        std::fs::write(&path, b"{\"version\":1,\"sandboxes\":[]}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(ScopedStores::from_file(&path).is_ok());
        let link = directory.join("symlink");
        symlink(&path, &link).unwrap();
        assert!(ScopedStores::from_file(&link).is_err());
        std::fs::remove_file(&link).unwrap();
        let hard = directory.join("hardlink");
        std::fs::hard_link(&path, &hard).unwrap();
        assert!(ScopedStores::from_file(&path).is_err());
        std::fs::remove_file(&hard).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ScopedStores::from_file(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn sandbox_scopes_revoke_retained_handles_and_refuse_invalid_reload() {
        let token = format!("hms_{}", "a".repeat(64));
        let policy = serde_json::json!({"version":1,"sandboxes":[{"sandbox_id":"parent","bindings":[{"placeholder":token,"value":"first","hosts":["api.example.test"]}]}]});
        let scopes = ScopedStores::from_json(policy.to_string().as_bytes()).unwrap();
        let retained = scopes.get("parent").unwrap();
        assert!(scopes.get("fork-child").is_none());
        let mut invalid = policy.clone();
        invalid["sandboxes"][0]["sandbox_id"] = "../invalid".into();
        assert!(scopes.rotate_json(invalid.to_string().as_bytes()).is_err());
        assert_eq!(
            retained
                .replace("api.example.test", token.as_bytes())
                .unwrap(),
            b"first"
        );
        let mut rotated = policy.clone();
        rotated["sandboxes"][0]["bindings"][0]["value"] = "rotated".into();
        scopes.rotate_json(rotated.to_string().as_bytes()).unwrap();
        assert!(Arc::ptr_eq(&retained, &scopes.get("parent").unwrap()));
        assert_eq!(
            retained
                .replace("api.example.test", token.as_bytes())
                .unwrap(),
            b"rotated"
        );
        let mut duplicates = policy.clone();
        duplicates["sandboxes"]
            .as_array_mut()
            .unwrap()
            .push(policy["sandboxes"][0].clone());
        assert!(
            scopes
                .rotate_json(duplicates.to_string().as_bytes())
                .is_err()
        );
        scopes
            .rotate_json(b"{\"version\":1,\"sandboxes\":[]}")
            .unwrap();
        assert!(scopes.get("parent").is_none());
        assert_eq!(
            retained
                .replace("api.example.test", token.as_bytes())
                .unwrap(),
            token.as_bytes()
        );
    }
    #[test]
    fn operator_json_is_strict_and_failed_reload_preserves_policy() {
        let token = format!("hms_{}", "a".repeat(64));
        let document = serde_json::json!({"version":1,"bindings":[{"placeholder":token,"value":"first","hosts":["api.example.test"]}]}).to_string();
        let store = Store::from_json(document.as_bytes()).unwrap();
        for invalid in [
            "{\"version\":2,\"bindings\":[]}",
            "{\"version\":1,\"bindings\":[],\"extra\":true}",
            "{\"version\":1,\"version\":1,\"bindings\":[]}",
            "{\"version\":true,\"bindings\":[]}",
        ] {
            assert!(store.rotate_json(invalid.as_bytes()).is_err());
            assert_eq!(
                store.replace("api.example.test", token.as_bytes()).unwrap(),
                b"first"
            );
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
        invalid["bindings"][0]["extra"] = true.into();
        assert!(store.rotate_json(invalid.to_string().as_bytes()).is_err());
        assert!(Store::from_json(&vec![b' '; LIMIT + 1]).is_err());
        store
            .rotate_json(b"{\"version\":1,\"bindings\":[]}")
            .unwrap();
        assert_eq!(
            store.replace("api.example.test", token.as_bytes()).unwrap(),
            token.as_bytes()
        );
    }
    #[test]
    fn json_values_escape_secrets_without_rewriting_numbers_or_keys() {
        let b = binding('a', b"quote\" slash\\ newline\n");
        let token = b.placeholder.clone();
        let store = Store::new(vec![b]).unwrap();
        let input = format!(
            "{{ \"{token}\": 123456789012345678901234567890, \"value\": \"\\u0068{}\", \"array\": [\"unchanged\"] }}",
            &token[1..]
        );
        let output = store
            .replace_json("api.example.test", input.as_bytes())
            .unwrap();
        let expected = format!(
            "{{ \"{token}\": 123456789012345678901234567890, \"value\": \"quote\\\" slash\\\\ newline\\n\", \"array\": [\"unchanged\"] }}"
        );
        assert_eq!(output, expected.as_bytes());
        assert_eq!(
            store.replace_json("other.test", input.as_bytes()).unwrap(),
            input.as_bytes()
        );
        assert!(
            store
                .replace_json("api.example.test", b"{invalid}")
                .is_err()
        );
        let mut request = hyper::Request::builder()
            .uri("/")
            .header("host", "api.example.test")
            .header("content-type", "application/problem+json")
            .body(input.into_bytes())
            .unwrap();
        store
            .rewrite_request("api.example.test", &mut request)
            .unwrap();
        assert_eq!(request.body(), expected.as_bytes());
    }
    #[test]
    fn form_replacement_preserves_fields_and_space_semantics() {
        let b = binding('a', b"&=+ space");
        let token = b.placeholder.clone();
        let store = Store::new(vec![b]).unwrap();
        let body = format!(
            "untouched=a+b&key=prefix+{token}+suffix&encoded=%68{}",
            &token[1..]
        );
        let expected =
            b"untouched=a+b&key=prefix%20%26%3D%2B%20space%20suffix&encoded=%26%3D%2B%20space";
        assert_eq!(
            store
                .replace_form("api.example.test", body.as_bytes())
                .unwrap(),
            expected
        );
        let mut request = hyper::Request::builder()
            .method("POST")
            .uri("/")
            .header("host", "api.example.test")
            .header(
                "content-type",
                "Application/X-Www-Form-Urlencoded; charset=UTF-8",
            )
            .body(body.into_bytes())
            .unwrap();
        store
            .rewrite_request("api.example.test", &mut request)
            .unwrap();
        assert_eq!(request.body(), expected);
        assert_eq!(
            request.headers()["content-length"],
            expected.len().to_string()
        );
    }
    #[test]
    fn request_fields_and_body_length_are_rewritten_together() {
        let b = binding('a', b"secret");
        let token = b.placeholder.clone();
        let store = Store::new(vec![b]).unwrap();
        let mut request = hyper::Request::builder()
            .method("POST")
            .uri(format!("/path?key={token}"))
            .header("host", "api.example.test:443")
            .header("x-key", &token)
            .header("content-length", token.len())
            .body(token.as_bytes().to_vec())
            .unwrap();
        store
            .rewrite_request("api.example.test", &mut request)
            .unwrap();
        assert_eq!(request.uri(), "/path?key=secret");
        assert_eq!(request.headers()["x-key"], "secret");
        assert_eq!(request.headers()["content-length"], "6");
        assert_eq!(request.body(), b"secret");
    }
    #[test]
    fn request_failure_preserves_original_fields() {
        let b = binding('a', b"secret\ninvalid-header");
        let token = b.placeholder.clone();
        let store = Store::new(vec![b]).unwrap();
        let mut request = hyper::Request::builder()
            .uri(format!("/path?key={token}"))
            .header("host", "api.example.test")
            .header("x-key", &token)
            .body(token.as_bytes().to_vec())
            .unwrap();
        let original_uri = request.uri().clone();
        assert!(
            store
                .rewrite_request("api.example.test", &mut request)
                .is_err()
        );
        assert_eq!(request.uri(), &original_uri);
        assert_eq!(request.headers()["x-key"], token);
        assert_eq!(request.body(), token.as_bytes());
        assert!(store.rewrite_request("other.test", &mut request).is_err());
        request
            .headers_mut()
            .insert("content-encoding", "gzip".parse().unwrap());
        assert!(
            store
                .rewrite_request("api.example.test", &mut request)
                .is_err()
        );
    }
    #[test]
    fn query_values_cannot_change_parameter_structure() {
        let b = binding('a', b"x&y=+/# %\xff");
        let token = b.placeholder.clone();
        let policy = Bindings::new(vec![b]).unwrap();
        let query = format!("keep=%2f+&key={token}&empty=&bare&&key=%68{}", &token[1..]);
        let result = policy
            .replace_query("api.example.test", query.as_bytes())
            .unwrap();
        assert_eq!(
            result,
            b"keep=%2f+&key=x%26y%3D%2B%2F%23%20%25%FF&empty=&bare&&key=x%26y%3D%2B%2F%23%20%25%FF"
        );
        assert_eq!(
            policy
                .replace_query("other.test", query.as_bytes())
                .unwrap(),
            query.as_bytes()
        );
        let twice = format!("key=%2568{}", &token[1..]);
        assert_eq!(
            policy
                .replace_query("api.example.test", twice.as_bytes())
                .unwrap(),
            twice.as_bytes()
        );
        for malformed in [b"key=%".as_slice(), b"key=%0", b"key=%GG"] {
            assert!(policy.replace_query("api.example.test", malformed).is_err());
        }
    }
    #[test]
    fn basic_auth_substitution_respects_scope_and_scheme() {
        let b = binding('a', b"secret");
        let token = b.placeholder.clone();
        let policy = Bindings::new(vec![b]).unwrap();
        let input = format!("bAsIc {}", STANDARD.encode(format!("user:{token}")));
        let expected = format!("Basic {}", STANDARD.encode(b"user:secret"));
        assert_eq!(
            policy
                .replace_basic_auth("api.example.test", input.as_bytes())
                .unwrap(),
            expected.as_bytes()
        );
        assert_eq!(
            policy
                .replace_basic_auth("other.example.test", input.as_bytes())
                .unwrap(),
            input.as_bytes()
        );
        assert_eq!(
            policy
                .replace_basic_auth("api.example.test", b"Bearer unchanged")
                .unwrap(),
            b"Bearer unchanged"
        );
        assert!(
            policy
                .replace_basic_auth("api.example.test", b"Basic invalid")
                .is_err()
        );
        let no_separator = format!("Basic {}", STANDARD.encode(b"not-a-credential"));
        assert!(
            policy
                .replace_basic_auth("api.example.test", no_separator.as_bytes())
                .is_err()
        );
    }
    #[test]
    fn rotation_is_validated_before_commit_and_revocation_removes_values() {
        let first = binding('a', b"old");
        let token = first.placeholder.clone();
        let store = Store::new(vec![first]).unwrap();
        assert!(
            store
                .rotate(vec![binding('a', b"bad"), binding('a', b"duplicate")])
                .is_err()
        );
        assert_eq!(
            store.replace("api.example.test", token.as_bytes()).unwrap(),
            b"old"
        );
        store.rotate(vec![binding('a', b"new")]).unwrap();
        assert_eq!(
            store.replace("api.example.test", token.as_bytes()).unwrap(),
            b"new"
        );
        store.rotate(vec![]).unwrap();
        assert_eq!(
            store.replace("api.example.test", token.as_bytes()).unwrap(),
            token.as_bytes()
        );
    }
    fn binding(c: char, value: &[u8]) -> Binding {
        Binding {
            placeholder: format!("hms_{}", c.to_string().repeat(64)),
            value: value.to_vec(),
            hosts: vec!["API.example.test".into()],
        }
    }
    #[test]
    fn exact_host_scope_and_binary_input() {
        let b = binding('a', b"secret");
        let token = b.placeholder.clone();
        let policy = Bindings::new(vec![b]).unwrap();
        let input = [b"\0key=".as_slice(), token.as_bytes(), b"\xff"].concat();
        assert_eq!(
            policy.replace("api.example.test", &input).unwrap(),
            b"\0key=secret\xff"
        );
        assert_eq!(
            policy.replace("api.example.test.evil", &input).unwrap(),
            input
        );
        assert!(policy.replace("api.example.test:443", &input).is_err());
    }
    #[test]
    fn inserted_values_are_never_reprocessed_or_logged() {
        let second = binding('b', b"actual-secret");
        let first = binding('a', second.placeholder.as_bytes());
        let token = first.placeholder.clone();
        let policy = Bindings::new(vec![first, second]).unwrap();
        assert_eq!(
            policy
                .replace("api.example.test", token.as_bytes())
                .unwrap(),
            binding('b', b"x").placeholder.as_bytes()
        );
        assert!(!format!("{policy:?}").contains("actual-secret"));
    }
    #[test]
    fn invalid_configuration_and_duplicate_scopes_are_refused() {
        assert!(Bindings::new(vec![binding('a', b"x"), binding('a', b"y")]).is_err());
        let mut b = binding('a', b"x");
        b.hosts.push("api.example.test".into());
        assert!(Bindings::new(vec![b]).is_err());
        for name in ["*.example.test", "bad..test", "-bad.test", "host/path"] {
            let mut b = binding('a', b"x");
            b.hosts = vec![name.into()];
            assert!(Bindings::new(vec![b]).is_err());
        }
    }
    #[test]
    fn expansion_and_input_bounds_are_enforced() {
        let binary = binding('a', &vec![0xff; 65536]);
        let query = format!("key={}", binary.placeholder.repeat(6));
        let binary_policy = Bindings::new(vec![binary]).unwrap();
        assert!(
            binary_policy
                .replace_query("api.example.test", query.as_bytes())
                .is_err()
        );
        let b = binding('a', &vec![b'x'; 65536]);
        let token = b.placeholder.clone();
        let policy = Bindings::new(vec![b]).unwrap();
        assert!(
            policy
                .replace("api.example.test", token.repeat(17).as_bytes())
                .is_err()
        );
        assert!(
            policy
                .replace("api.example.test", &vec![0; LIMIT + 1])
                .is_err()
        );
    }
}
