//! Pulling an OCI image straight from its registry, with no Docker daemon:
//! what building a template from an image reference needs on a node that
//! has only this process.
//!
//! The distribution API, as far as a public image needs it: the manifest
//! (or an index, from which the `linux/amd64` manifest is chosen), the
//! config blob for the image's environment, and the layers, each verified
//! against its sha256 digest before it is used and applied in order with
//! the OCI whiteout rules. Bearer-token auth, which Docker Hub and most
//! registries serve, is followed when challenged -- anonymously, or with a
//! username and password for a private image, which a registry asking for
//! basic auth is sent directly. Layers are gzip, zstd, or uncompressed.
//!
//! The filesystem is assembled in memory, as the path-ordered set of
//! entries an initramfs needs -- not unpacked to disk, so a layer's paths
//! never touch the host's filesystem at all.

use std::collections::BTreeMap;
use std::io::Read;

use serde_json::Value;
use sha2::Digest;

/// The most a pulled image may hold, compressed layers summed: an initramfs
/// lives in guest memory, so this is a bound on what a template can cost.
pub const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The most a pulled image may unpack to, summed over its layers and counting
/// tar headers: the assembled filesystem is held in memory, so this bounds
/// what a registry's content (a gzip or zstd bomb above all) can cost the node.
/// Real images -- ubuntu, python, node, a CUDA runtime -- are well under it.
pub const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The most one file of an image may hold.
pub const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The most entries (files, directories, links) an image may hold, per layer
/// and assembled. The largest real images have a few hundred thousand.
pub const MAX_ENTRIES: usize = 1_000_000;

/// How long one blob may take to arrive, and one manifest or token.
/// The most a manifest or index may be.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const BLOB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One entry of the assembled filesystem.
#[derive(Debug, Clone)]
pub enum Entry {
    Dir {
        mode: u32,
    },
    File {
        mode: u32,
        data: Vec<u8>,
    },
    Symlink {
        target: String,
    },
    /// A hard link to the file at `target`: kept as a link, not a copy --
    /// busybox's image is some 400 links to one binary, and copies made its
    /// initramfs 270 MiB.
    Link {
        target: String,
    },
}

/// A pulled image: its filesystem, and its `ENV`, `ENTRYPOINT`, `CMD`,
/// `WORKDIR` and `USER`.
#[derive(Debug, Default)]
pub struct Image {
    /// Paths without a leading slash (`usr/bin/python3`).
    pub entries: BTreeMap<String, Entry>,
    pub env: Vec<String>,
    /// The image's `ENTRYPOINT`, as an argument vector.
    pub entrypoint: Vec<String>,
    /// The image's `CMD`: the entrypoint's arguments, or the command itself.
    pub cmd: Vec<String>,
    /// The image's `WORKDIR`, when it sets one.
    pub workdir: Option<String>,
    /// The image's `USER`, when it sets one.
    pub user: Option<String>,
    /// The manifest's digest, which names exactly what was pulled.
    pub digest: String,
}

/// `[registry/]repository[:tag|@digest]`, Docker's rules for the defaults.
#[derive(Debug, PartialEq, Eq)]
pub struct Reference {
    pub registry: String,
    pub repository: String,
    pub reference: String,
}

impl Reference {
    pub fn parse(image: &str) -> Result<Self, String> {
        if image.is_empty() || image.chars().any(char::is_whitespace) {
            return Err(format!("{image:?} is not an image reference"));
        }
        let (name, reference) = match image.split_once('@') {
            Some((name, digest)) => (name, digest.to_string()),
            None => match image.rsplit_once(':') {
                // A colon before the last slash is a registry port.
                Some((name, tag)) if !tag.contains('/') => (name, tag.to_string()),
                _ => (image, "latest".to_string()),
            },
        };
        let (registry, repository) = match name.split_once('/') {
            Some((first, rest))
                if first.contains('.') || first.contains(':') || first == "localhost" =>
            {
                (first.to_string(), rest.to_string())
            }
            _ => ("registry-1.docker.io".to_string(), name.to_string()),
        };
        let repository = if registry == "registry-1.docker.io" && !repository.contains('/') {
            format!("library/{repository}")
        } else {
            repository
        };
        Ok(Self {
            registry,
            repository,
            reference,
        })
    }
}

const MANIFEST_TYPES: &str = "application/vnd.oci.image.index.v1+json, \
    application/vnd.docker.distribution.manifest.list.v2+json, \
    application/vnd.oci.image.manifest.v1+json, \
    application/vnd.docker.distribution.manifest.v2+json";

/// A private registry's login.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credentials({}, <redacted>)", self.username)
    }
}

/// How a registry session proves itself, once challenged.
enum Auth {
    None,
    Bearer(String),
    Basic,
}

/// A registry session: the client and whatever a challenge earned.
struct Registry {
    policy: Policy,
    base: String,
    repository: String,
    auth: Auth,
    credentials: Option<Credentials>,
}

impl Registry {
    async fn get(
        &mut self,
        path: &str,
        accept: &str,
        timeout: std::time::Duration,
    ) -> Result<reqwest::Response, String> {
        let url = format!("{}/v2/{}/{path}", self.base, self.repository);
        for _ in 0..2 {
            // Credentials and the token go to the registry itself, never to
            // wherever a redirect sends the request on.
            let response = self
                .policy
                .get(&url, true, timeout, |request, same_origin| {
                    let request = request.header("Accept", accept);
                    match (&self.auth, &self.credentials) {
                        (Auth::Bearer(token), _) if same_origin => request.bearer_auth(token),
                        (Auth::Basic, Some(c)) if same_origin => {
                            request.basic_auth(&c.username, Some(&c.password))
                        }
                        _ => request,
                    }
                })
                .await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED
                && matches!(self.auth, Auth::None)
            {
                let challenge = response
                    .headers()
                    .get("www-authenticate")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                self.auth = if challenge.starts_with("Basic") {
                    if self.credentials.is_none() {
                        return Err(format!("{url}: the registry wants a username and password"));
                    }
                    Auth::Basic
                } else {
                    Auth::Bearer(self.authenticate(&challenge).await?)
                };
                continue;
            }
            if !response.status().is_success() {
                return Err(format!("{url}: {}", response.status()));
            }
            return Ok(response);
        }
        Err(format!("{url}: still unauthorized with a token"))
    }

    /// Follow a `Bearer realm=..., service=..., scope=...` challenge for a
    /// pull token: anonymous, or for this session's credentials.
    async fn authenticate(&self, challenge: &str) -> Result<String, String> {
        let params = challenge.strip_prefix("Bearer ").ok_or_else(|| {
            format!("the registry asks for {challenge:?}, which is not bearer auth")
        })?;
        let mut realm = None;
        let mut query = Vec::new();
        for part in params.split(',') {
            let Some((key, value)) = part.trim().split_once('=') else {
                continue;
            };
            let value = value.trim_matches('"').to_string();
            if key == "realm" {
                realm = Some(value);
            } else {
                query.push((key.to_string(), value));
            }
        }
        if !query.iter().any(|(k, _)| k == "scope") {
            query.push((
                "scope".into(),
                format!("repository:{}:pull", self.repository),
            ));
        }
        let realm = realm.ok_or("a bearer challenge without a realm")?;
        // The realm is named by the registry and receives the client's
        // credentials: it must be a scheme, an address and (when a list is
        // set) a host the registry's own would be allowed.
        realm_scheme_ok(&self.base, &realm)?;
        let mut realm_url = reqwest::Url::parse(&realm).map_err(|e| format!("{realm}: {e}"))?;
        realm_url.query_pairs_mut().extend_pairs(&query);
        let answer: Value = self
            .policy
            .get(
                realm_url.as_str(),
                true,
                API_TIMEOUT,
                |request, same_origin| match (&self.credentials, same_origin) {
                    (Some(c), true) => request.basic_auth(&c.username, Some(&c.password)),
                    _ => request,
                },
            )
            .await
            .map_err(|e| format!("{realm}: {e}"))?
            .error_for_status()
            .map_err(|e| format!("{realm}: {e}"))?
            .json()
            .await
            .map_err(|e| format!("{realm}: {e}"))?;
        answer["token"]
            .as_str()
            .or_else(|| answer["access_token"].as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("{realm} answered without a token"))
    }

    /// A manifest or index, read with a size cap and parsed.
    async fn json(&mut self, path: &str) -> Result<Value, String> {
        let mut response = self.get(path, MANIFEST_TYPES, API_TIMEOUT).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > MAX_MANIFEST_BYTES {
                return Err(format!("larger than {MAX_MANIFEST_BYTES} bytes"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    /// A blob, verified against its digest.
    async fn blob(&mut self, digest: &str, limit: u64) -> Result<Vec<u8>, String> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| format!("{digest}: only sha256 digests are verified here"))?
            .to_string();
        let mut response = self
            .get(&format!("blobs/{digest}"), "*/*", BLOB_TIMEOUT)
            .await?;
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(format!("{digest} is larger than {limit} bytes"));
        }
        // Streamed, and counted as it arrives: a chunked response states no
        // length, and must not be buffered before it is measured.
        let mut hasher = sha2::Sha256::new();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("{digest}: {e}"))?
        {
            if bytes.len() as u64 + chunk.len() as u64 > limit {
                return Err(format!("{digest} is larger than {limit} bytes"));
            }
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
        }
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != hex {
            return Err(format!(
                "{digest}: the registry sent content hashing to sha256:{got}"
            ));
        }
        Ok(bytes)
    }
}

/// Pull `image` for `linux/amd64` and assemble its filesystem.
pub async fn pull(
    policy: &Policy,
    image: &str,
    credentials: Option<Credentials>,
) -> Result<Image, String> {
    let reference = Reference::parse(image)?;
    let mut registry = Registry {
        policy: policy.clone(),
        base: format!("{}://{}", scheme(&reference.registry), reference.registry),
        repository: reference.repository.clone(),
        auth: Auth::None,
        credentials,
    };

    let mut manifest: Value = registry
        .json(&format!("manifests/{}", reference.reference))
        .await
        .map_err(|e| format!("{image}: the manifest did not parse: {e}"))?;
    let mut digest = reference.reference.clone();
    // An index names one manifest per platform.
    if let Some(manifests) = manifest["manifests"].as_array() {
        let chosen = manifests
            .iter()
            .find(|m| {
                m["platform"]["os"] == "linux"
                    && m["platform"]["architecture"] == "amd64"
                    && m["platform"]["variant"].is_null()
            })
            .or_else(|| {
                manifests.iter().find(|m| {
                    m["platform"]["os"] == "linux" && m["platform"]["architecture"] == "amd64"
                })
            })
            .ok_or_else(|| format!("{image} has no linux/amd64 image"))?;
        digest = chosen["digest"]
            .as_str()
            .ok_or("an index entry without a digest")?
            .to_string();
        manifest = registry
            .json(&format!("manifests/{digest}"))
            .await
            .map_err(|e| format!("{image}: the manifest did not parse: {e}"))?;
    }

    let config_digest = manifest["config"]["digest"]
        .as_str()
        .ok_or_else(|| format!("{image}: a manifest without a config"))?
        .to_string();
    let config: Value = serde_json::from_slice(&registry.blob(&config_digest, 16 << 20).await?)
        .map_err(|e| format!("{image}: the config did not parse: {e}"))?;
    let env = config["config"]["Env"]
        .as_array()
        .map(|vars| {
            vars.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let (entrypoint, cmd, workdir, user) = launch_of(&config);

    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| format!("{image}: a manifest without layers"))?
        .clone();
    let mut pulled = Image {
        env,
        entrypoint,
        cmd,
        workdir,
        user,
        digest,
        ..Image::default()
    };
    let mut budget = MAX_IMAGE_BYTES;
    let mut unpacked = MAX_UNPACKED_BYTES;
    for layer in layers {
        let digest = layer["digest"].as_str().ok_or("a layer without a digest")?;
        let blob = registry.blob(digest, budget).await?;
        budget = budget.saturating_sub(blob.len() as u64);
        apply_layer(&mut pulled.entries, decompressed(&blob)?, &mut unpacked)
            .map_err(|e| format!("{image}: layer {digest}: {e}"))?;
    }
    Ok(pulled)
}

/// The image config's `Entrypoint`, `Cmd`, `WorkingDir` and `User`.
fn launch_of(config: &Value) -> (Vec<String>, Vec<String>, Option<String>, Option<String>) {
    let words = |key: &str| -> Vec<String> {
        config["config"][key]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let text = |key: &str| -> Option<String> {
        config["config"][key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    (
        words("Entrypoint"),
        words("Cmd"),
        text("WorkingDir"),
        text("User"),
    )
}

/// Where a pull may connect: the registry, the bearer realm it names, and any
/// redirect either answers with.
///
/// By default every address must be a public one
/// (`hv2_cluster::events::is_global`: not loopback, private, link-local -- the
/// cloud metadata service --, shared CGNAT, unique-local, multicast,
/// documentation or unspecified), because the registry host comes from a
/// client's image reference and the realm from the registry's reply, and each
/// is a request the node makes from inside its network. The name is resolved
/// once, here, and the connection is pinned to the address that passed, so it
/// cannot resolve somewhere else a moment later.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// `--allow-private-registries`: skip the address check.
    pub allow_private: bool,
    /// `--registry-allow HOST`: when non-empty, the registry's host and its
    /// realm's host must be one of these (a bare host, or `host:port`).
    pub allow_hosts: Vec<String>,
    /// Test only: one address exempt from the address check, for a server on
    /// loopback that stands in for the registry.
    #[cfg(test)]
    pub(crate) exempt: Option<std::net::SocketAddr>,
}

/// Redirects followed per request.
const MAX_REDIRECTS: usize = 5;

impl Policy {
    /// The operator's settings: `--allow-private-registries` and `--registry-allow`.
    pub fn new(allow_private: bool, allow_hosts: Vec<String>) -> Self {
        Self {
            allow_private,
            allow_hosts,
            #[cfg(test)]
            exempt: None,
        }
    }

    /// Whether `host` (with `port`) is on the operator's list, if there is one.
    fn listed(&self, host: &str, port: Option<u16>) -> Result<(), String> {
        if self.allow_hosts.is_empty() {
            return Ok(());
        }
        let host = host
            .trim_matches(|c| c == '[' || c == ']')
            .to_ascii_lowercase();
        let found = self.allow_hosts.iter().any(|allowed| {
            let allowed = allowed.to_ascii_lowercase();
            allowed == host || port.is_some_and(|p| allowed == format!("{host}:{p}"))
        });
        if found {
            Ok(())
        } else {
            Err(format!(
                "{host} is not one of this node's --registry-allow hosts"
            ))
        }
    }

    /// The first of `addrs` a pull may connect to.
    fn pick(&self, addrs: &[std::net::SocketAddr]) -> Option<std::net::SocketAddr> {
        addrs.iter().copied().find(|a| {
            self.allow_private || hv2_cluster::events::is_global(a.ip()) || self.is_exempt(a)
        })
    }

    #[cfg(test)]
    fn is_exempt(&self, a: &std::net::SocketAddr) -> bool {
        self.exempt == Some(*a)
    }

    #[cfg(not(test))]
    fn is_exempt(&self, _: &std::net::SocketAddr) -> bool {
        false
    }

    /// A client for `url` that can connect only to the address that passed.
    async fn client_for(
        &self,
        url: &reqwest::Url,
        listed: bool,
    ) -> Result<reqwest::Client, String> {
        // An IPv6 literal comes bracketed; the lookup wants it bare.
        let host = url
            .host_str()
            .ok_or_else(|| format!("{url}: no host"))?
            .trim_matches(|c| c == '[' || c == ']')
            .to_string();
        let port = url.port_or_known_default().unwrap_or(443);
        if listed {
            self.listed(&host, url.port())?;
        }
        let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| format!("{host}: {e}"))?
            .collect();
        let addr = self.pick(&addrs).ok_or_else(|| {
            format!(
                "{host} resolves to no public address; a registry on a private network \
                 needs --allow-private-registries"
            )
        })?;
        hv2_tls::http_client(None)
            .unwrap_or_default()
            .resolve(&host, addr)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("{host}: {e}"))
    }

    /// `GET url`, each hop checked: the first against the host list (when
    /// `listed`), every hop against the address check and never from https
    /// down to http. `build` adds headers; it is told whether the request is
    /// still on the origin it began at, so credentials do not follow a
    /// redirect elsewhere.
    async fn get(
        &self,
        url: &str,
        listed: bool,
        timeout: std::time::Duration,
        build: impl Fn(reqwest::RequestBuilder, bool) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        let mut url = reqwest::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
        let origin = url.origin();
        for hop in 0..=MAX_REDIRECTS {
            let client = self.client_for(&url, listed && hop == 0).await?;
            let request = build(
                client.get(url.clone()).timeout(timeout),
                url.origin() == origin,
            );
            let response = request.send().await.map_err(|e| format!("{url}: {e}"))?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
            else {
                return Ok(response);
            };
            let next = url
                .join(location)
                .map_err(|e| format!("{url}: redirect to {location:?}: {e}"))?;
            if !matches!(next.scheme(), "http" | "https")
                || (url.scheme() == "https" && next.scheme() != "https")
            {
                return Err(format!("{url}: refused a redirect to {next}"));
            }
            url = next;
        }
        Err(format!("{url}: more than {MAX_REDIRECTS} redirects"))
    }
}

/// A bearer realm must be https when the registry is: a challenge in the
/// clear would hand the client's credentials to the network.
fn realm_scheme_ok(registry_base: &str, realm: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(realm).map_err(|e| format!("realm {realm}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("realm {realm}: not an http(s) URL"));
    }
    if registry_base.starts_with("https://") && url.scheme() != "https" {
        return Err(format!(
            "realm {realm}: the registry is https and its token server must be too"
        ));
    }
    Ok(())
}

/// HTTPS, but for a registry on this host -- as Docker, which lets
/// `localhost` registries be plain HTTP: nothing crosses a network.
fn scheme(registry: &str) -> &'static str {
    let host = registry.rsplit_once(':').map_or(registry, |(host, _)| host);
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        "http"
    } else {
        "https"
    }
}

/// A layer's tar, by what its bytes say it is rather than its media type:
/// registries have been seen to label a gzip layer as plain tar.
fn decompressed(blob: &[u8]) -> Result<Box<dyn Read + '_>, String> {
    Ok(if blob.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(blob))
    } else if blob.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Box::new(
            ruzstd::decoding::StreamingDecoder::new(blob)
                .map_err(|e| format!("a zstd layer that does not open: {e}"))?,
        )
    } else {
        Box::new(blob)
    })
}

/// Normalise a tar path to the form entries are kept in, refusing anything
/// that would leave the root.
fn clean(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A reader that fails, rather than ends, once `left` bytes have passed --
/// a layer cut short by a limit must not look like a layer that is whole --
/// and is charged for what it reads, so one budget spans an image's layers.
pub(crate) struct Capped<'a, R> {
    inner: R,
    left: &'a mut u64,
}

impl<'a, R> Capped<'a, R> {
    pub(crate) fn new(inner: R, left: &'a mut u64) -> Self {
        Self { inner, left }
    }
}

impl<R: Read> Read for Capped<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n as u64 > *self.left {
            *self.left = 0;
            return Err(std::io::Error::other(
                "the archive unpacks to more than the limit allows",
            ));
        }
        *self.left -= n as u64;
        Ok(n)
    }
}

/// Remove every entry whose path is `prefix` followed by more: in the map's
/// order they are adjacent, so this does not walk the rest.
fn remove_under(entries: &mut BTreeMap<String, Entry>, prefix: &str) {
    let keys: Vec<String> = entries
        .range::<str, _>((
            std::ops::Bound::Included(prefix),
            std::ops::Bound::Unbounded,
        ))
        .map(|(path, _)| path)
        .take_while(|path| path.starts_with(prefix))
        .cloned()
        .collect();
    for key in keys {
        entries.remove(&key);
    }
}

/// Apply one layer: its entries replace what is below them, and its
/// whiteouts delete -- `.wh.NAME` one path, `.wh..wh..opq` everything under
/// its directory from lower layers.
///
/// The layer is untrusted and may be a bomb, so everything about it is
/// bounded: the bytes it unpacks to (against `unpacked`, shared by the image's
/// layers), the entries it holds, and the size of each file -- and memory is
/// only ever used for bytes actually read, never for a size a header claims.
fn apply_layer(
    entries: &mut BTreeMap<String, Entry>,
    layer: impl Read,
    unpacked: &mut u64,
) -> Result<(), String> {
    let mut archive = tar::Archive::new(Capped::new(layer, unpacked));
    let mut seen = 0usize;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        seen += 1;
        if seen > MAX_ENTRIES || entries.len() > MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries"));
        }
        let raw = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let Some(path) = clean(&raw) else {
            continue;
        };
        let (dir, name) = match path.rsplit_once('/') {
            Some((dir, name)) => (Some(dir), name),
            None => (None, path.as_str()),
        };
        if name == ".wh..wh..opq" {
            let prefix = dir.map_or(String::new(), |d| format!("{d}/"));
            remove_under(entries, &prefix);
            continue;
        }
        if let Some(hidden) = name.strip_prefix(".wh.") {
            let gone = dir.map_or(hidden.to_string(), |d| format!("{d}/{hidden}"));
            entries.remove(&gone);
            remove_under(entries, &format!("{gone}/"));
            continue;
        }
        let mode = entry.header().mode().unwrap_or(0o644) & 0o7777;
        let kind = entry.header().entry_type();
        let new = match kind {
            tar::EntryType::Directory => Entry::Dir { mode },
            tar::EntryType::Symlink => Entry::Symlink {
                target: entry
                    .link_name()
                    .map_err(|e| e.to_string())?
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            tar::EntryType::Link => {
                // A hard link: the same content under another name.
                let target = entry
                    .link_name()
                    .map_err(|e| e.to_string())?
                    .and_then(|t| clean(&t.to_string_lossy()));
                match target {
                    Some(target) => Entry::Link { target },
                    None => continue,
                }
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let size = entry.size();
                if size > MAX_ENTRY_BYTES {
                    return Err(format!(
                        "{path}: claims {size} bytes, more than the {MAX_ENTRY_BYTES} a file may hold"
                    ));
                }
                // Not sized from the header: the vector grows with what the
                // archive actually delivers.
                let mut data = Vec::new();
                (&mut entry)
                    .take(MAX_ENTRY_BYTES)
                    .read_to_end(&mut data)
                    .map_err(|e| format!("{path}: {e}"))?;
                Entry::File { mode, data }
            }
            // Devices and FIFOs: the guest's /dev is devtmpfs.
            _ => continue,
        };
        // A path replaced by something that is not a directory takes what
        // was under it with it.
        if !matches!(new, Entry::Dir { .. }) {
            remove_under(entries, &format!("{path}/"));
        }
        entries.insert(path, new);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_images_launch_settings_are_read_from_its_config() {
        let config = serde_json::json!({"config": {
            "Entrypoint": ["python", "-m"], "Cmd": ["http.server"],
            "WorkingDir": "/app", "User": "nobody"}});
        assert_eq!(
            launch_of(&config),
            (
                vec!["python".to_string(), "-m".to_string()],
                vec!["http.server".to_string()],
                Some("/app".to_string()),
                Some("nobody".to_string())
            )
        );
        // An image that sets none (null Entrypoint, empty strings) gets none.
        let bare =
            serde_json::json!({"config": {"Entrypoint": null, "WorkingDir": "", "User": ""}});
        assert_eq!(launch_of(&bare), (vec![], vec![], None, None));
    }

    use super::*;

    #[test]
    fn references_take_dockers_defaults() {
        let r = Reference::parse("python:3.12-slim").unwrap();
        assert_eq!(
            (
                r.registry.as_str(),
                r.repository.as_str(),
                r.reference.as_str()
            ),
            ("registry-1.docker.io", "library/python", "3.12-slim")
        );
        let r = Reference::parse("ghcr.io/org/tool").unwrap();
        assert_eq!(
            (
                r.registry.as_str(),
                r.repository.as_str(),
                r.reference.as_str()
            ),
            ("ghcr.io", "org/tool", "latest")
        );
        let r = Reference::parse("localhost:5000/x/y@sha256:abc").unwrap();
        assert_eq!(
            (
                r.registry.as_str(),
                r.repository.as_str(),
                r.reference.as_str()
            ),
            ("localhost:5000", "x/y", "sha256:abc")
        );
        assert!(Reference::parse("has space").is_err());
    }

    fn layer(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    #[test]
    fn layers_apply_in_order_with_whiteouts() {
        let mut entries = BTreeMap::new();
        apply_layer(
            &mut entries,
            &layer(&[("etc/a", b"1"), ("etc/b", b"2"), ("opt/x/y", b"3")])[..],
            &mut MAX_UNPACKED_BYTES.clone(),
        )
        .unwrap();
        apply_layer(
            &mut entries,
            &layer(&[
                ("etc/.wh.a", b""),
                ("etc/b", b"22"),
                ("opt/x/.wh..wh..opq", b""),
                ("opt/x/z", b"4"),
            ])[..],
            &mut MAX_UNPACKED_BYTES.clone(),
        )
        .unwrap();
        assert!(!entries.contains_key("etc/a"), "a whiteout deletes");
        assert!(
            matches!(&entries["etc/b"], Entry::File { data, .. } if data == b"22"),
            "upper wins"
        );
        assert!(
            !entries.contains_key("opt/x/y"),
            "an opaque dir hides the lower one"
        );
        assert!(entries.contains_key("opt/x/z"), "but keeps its own layer's");
    }

    /// A layer is read as what its bytes are: gzip, zstd, or a bare tar.
    #[test]
    fn layers_decompress_by_their_magic() {
        let tar = layer(&[("etc/hostname", b"sandbox\n")]);
        let gzip = {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &tar).unwrap();
            gz.finish().unwrap()
        };
        let zstd = ruzstd::encoding::compress_to_vec(
            &tar[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        for blob in [&tar, &gzip, &zstd] {
            let mut entries = BTreeMap::new();
            apply_layer(
                &mut entries,
                decompressed(blob).unwrap(),
                &mut MAX_UNPACKED_BYTES.clone(),
            )
            .unwrap();
            assert!(
                matches!(entries.get("etc/hostname"), Some(Entry::File { data, .. }) if data == b"sandbox\n")
            );
        }
    }

    /// A header that claims a file of `claimed` bytes, followed by `data`
    /// (what the archive really holds). The size is base-256 encoded, as tar
    /// does for what octal cannot say, so it can claim a terabyte.
    fn lying_layer(claimed: u64, data: &[u8]) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_path("big").unwrap();
        header.set_mode(0o644);
        header.set_size(0);
        header.set_cksum();
        let mut block = header.as_bytes().to_vec();
        block[124] = 0x80;
        block[125..136].fill(0);
        block[128..136].copy_from_slice(&claimed.to_be_bytes());
        block[148..156].fill(b' ');
        let sum: u32 = block.iter().map(|b| u32::from(*b)).sum();
        block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        block.extend_from_slice(data);
        block
    }

    /// A header claiming 2^40 bytes used to be `Vec::with_capacity(2^40)`,
    /// which aborts the node. It is now refused before anything is read.
    #[test]
    fn a_header_claiming_a_huge_file_is_refused_not_allocated() {
        for claimed in [1u64 << 40, MAX_ENTRY_BYTES + 1] {
            let mut entries = BTreeMap::new();
            let err = apply_layer(
                &mut entries,
                &lying_layer(claimed, b"tiny")[..],
                &mut MAX_UNPACKED_BYTES.clone(),
            )
            .unwrap_err();
            assert!(err.contains("claims"), "{claimed}: {err}");
            assert!(entries.is_empty());
        }
        // A claim within the cap that the archive does not back is an error
        // too, and costs only what arrived.
        let mut entries = BTreeMap::new();
        let err = apply_layer(
            &mut entries,
            &lying_layer(MAX_ENTRY_BYTES - 1, b"tiny")[..],
            &mut MAX_UNPACKED_BYTES.clone(),
        )
        .unwrap_err();
        assert!(!err.is_empty(), "the pull fails, whatever was read");
    }

    /// A few KiB of gzip that unpack to far more than the image may hold stop
    /// at the cap, in every compression the layer may use, and are an error
    /// rather than a silently shortened layer.
    #[test]
    fn a_compression_bomb_stops_at_the_unpacked_cap() {
        let big = vec![0u8; 12 << 20];
        let tar = layer(&[("zeros", &big)]);
        let gzip = {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            std::io::Write::write_all(&mut gz, &tar).unwrap();
            gz.finish().unwrap()
        };
        let zstd = ruzstd::encoding::compress_to_vec(
            &tar[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        assert!(gzip.len() < 64 << 10, "the bomb is small: {}", gzip.len());
        for blob in [&gzip, &zstd] {
            let mut entries = BTreeMap::new();
            let mut budget = 1u64 << 20;
            let err =
                apply_layer(&mut entries, decompressed(blob).unwrap(), &mut budget).unwrap_err();
            assert!(err.contains("unpacks to more than"), "{err}");
            assert!(entries.is_empty(), "nothing of a failed layer is kept");
            assert_eq!(budget, 0);
        }
        // Within the budget the same layer is fine.
        let mut entries = BTreeMap::new();
        let mut budget = 16u64 << 20;
        apply_layer(&mut entries, decompressed(&gzip).unwrap(), &mut budget).unwrap();
        assert!(matches!(&entries["zeros"], Entry::File { data, .. } if data.len() == 12 << 20));
        assert!(budget < 16 << 20, "the budget is spent as layers are read");
    }

    #[test]
    fn removing_under_a_prefix_takes_only_what_is_below_it() {
        let file = || Entry::File {
            mode: 0o644,
            data: vec![],
        };
        let mut entries = BTreeMap::new();
        for path in ["a", "a/b", "a/b/c", "a-b", "a.x", "ab/c", "b"] {
            entries.insert(path.to_string(), file());
        }
        remove_under(&mut entries, "a/");
        let left: Vec<&str> = entries.keys().map(String::as_str).collect();
        assert_eq!(left, ["a", "a-b", "a.x", "ab/c", "b"]);
        remove_under(&mut entries, "");
        assert!(entries.is_empty());
    }

    #[test]
    fn paths_that_leave_the_root_are_dropped() {
        assert_eq!(clean("../etc/passwd"), None);
        assert_eq!(clean("./usr//bin/x"), Some("usr/bin/x".into()));
        assert_eq!(clean("/"), None);
    }

    // ---- where a pull may connect ----

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A server on loopback that answers every request with `reply` and
    /// counts the connections it took.
    async fn serve(reply: String) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (addr, seen)
    }

    fn url(s: &str) -> reqwest::Url {
        reqwest::Url::parse(s).unwrap()
    }

    #[tokio::test]
    async fn a_registry_on_a_non_public_address_is_refused() {
        let strict = Policy::default();
        for bad in [
            "https://127.0.0.1:5000/",
            "https://localhost/",
            "https://10.1.2.3/",
            "https://172.16.0.9/",
            "https://192.168.1.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://100.64.0.1/",
            "https://[::1]/",
            "https://[fd00::1]/",
            "https://[fe80::1]/",
            "https://0.0.0.0/",
        ] {
            let e = strict.client_for(&url(bad), true).await.unwrap_err();
            assert!(e.contains("no public address"), "{bad}: {e}");
        }
        // A public literal needs no lookup and passes.
        strict
            .client_for(&url("https://93.184.216.34/"), true)
            .await
            .unwrap();
        // The opt-out lets the private ones through.
        let open = Policy {
            allow_private: true,
            ..Policy::default()
        };
        for ok in ["https://127.0.0.1:5000/", "https://169.254.169.254/"] {
            open.client_for(&url(ok), true).await.unwrap();
        }
    }

    #[tokio::test]
    async fn with_a_list_only_listed_hosts_are_allowed() {
        let policy = Policy {
            allow_hosts: vec!["Registry.Example".into(), "93.184.216.34:8443".into()],
            ..Policy::default()
        };
        // A public address, but not listed.
        let e = policy
            .client_for(&url("https://93.184.216.35/"), true)
            .await
            .unwrap_err();
        assert!(e.contains("--registry-allow"), "{e}");
        policy
            .client_for(&url("https://93.184.216.34:8443/"), true)
            .await
            .unwrap();
        // The list does not lift the address check.
        let on_list = Policy {
            allow_hosts: vec!["127.0.0.1".into()],
            ..Policy::default()
        };
        let e = on_list
            .client_for(&url("https://127.0.0.1/"), true)
            .await
            .unwrap_err();
        assert!(e.contains("no public address"), "{e}");
    }

    #[test]
    fn a_realm_must_be_https_when_the_registry_is() {
        assert!(realm_scheme_ok("https://r.example", "https://auth.example/token").is_ok());
        assert!(realm_scheme_ok("https://r.example", "http://auth.example/token").is_err());
        assert!(realm_scheme_ok("https://r.example", "ftp://auth.example/token").is_err());
        assert!(realm_scheme_ok("http://localhost:5000", "http://localhost:5000/token").is_ok());
    }

    #[tokio::test]
    async fn pulling_from_a_loopback_registry_never_connects() {
        let (addr, seen) = serve("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}".into()).await;
        let e = pull(&Policy::default(), &format!("{addr}/library/x:1"), None)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(e.contains("no public address"), "{e}");
        assert_eq!(seen.load(Ordering::SeqCst), 0, "a connection was made");
    }

    #[tokio::test]
    async fn a_redirect_to_the_metadata_service_is_not_followed() {
        let (addr, seen) = serve(
            "HTTP/1.1 302 Found\r\nlocation: http://169.254.169.254/latest/meta-data/\r\n\
             content-length: 0\r\n\r\n"
                .into(),
        )
        .await;
        let policy = Policy {
            exempt: Some(addr),
            ..Policy::default()
        };
        let e = pull(&policy, &format!("{addr}/library/x:1"), None)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(e.contains("no public address"), "{e}");
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_bearer_realm_on_an_internal_address_gets_no_credentials() {
        let (addr, _) = serve(
            "HTTP/1.1 401 Unauthorized\r\nwww-authenticate: Bearer \
             realm=\"http://169.254.169.254/token\",service=\"r\"\r\ncontent-length: 0\r\n\r\n"
                .into(),
        )
        .await;
        let policy = Policy {
            exempt: Some(addr),
            ..Policy::default()
        };
        let creds = Credentials {
            username: "u".into(),
            password: "p".into(),
        };
        let e = pull(&policy, &format!("{addr}/library/x:1"), Some(creds))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(e.contains("169.254.169.254"), "{e}");
        assert!(e.contains("no public address"), "{e}");
    }
}
