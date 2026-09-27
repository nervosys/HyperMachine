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

/// A pulled image: its filesystem, and its `ENV`.
#[derive(Debug, Default)]
pub struct Image {
    /// Paths without a leading slash (`usr/bin/python3`).
    pub entries: BTreeMap<String, Entry>,
    pub env: Vec<String>,
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
    http: reqwest::Client,
    base: String,
    repository: String,
    auth: Auth,
    credentials: Option<Credentials>,
}

impl Registry {
    async fn get(&mut self, path: &str, accept: &str) -> Result<reqwest::Response, String> {
        let url = format!("{}/v2/{}/{path}", self.base, self.repository);
        for _ in 0..2 {
            let mut request = self.http.get(&url).header("Accept", accept);
            match (&self.auth, &self.credentials) {
                (Auth::Bearer(token), _) => request = request.bearer_auth(token),
                (Auth::Basic, Some(c)) => {
                    request = request.basic_auth(&c.username, Some(&c.password));
                }
                _ => {}
            }
            let response = request.send().await.map_err(|e| format!("{url}: {e}"))?;
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
        let mut request = self.http.get(&realm).query(&query);
        if let Some(c) = &self.credentials {
            request = request.basic_auth(&c.username, Some(&c.password));
        }
        let answer: Value = request
            .send()
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

    /// A blob, verified against its digest.
    async fn blob(&mut self, digest: &str, limit: u64) -> Result<Vec<u8>, String> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| format!("{digest}: only sha256 digests are verified here"))?
            .to_string();
        let response = self.get(&format!("blobs/{digest}"), "*/*").await?;
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(format!("{digest} is larger than {limit} bytes"));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("{digest}: {e}"))?;
        if bytes.len() as u64 > limit {
            return Err(format!("{digest} is larger than {limit} bytes"));
        }
        let got: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != hex {
            return Err(format!(
                "{digest}: the registry sent content hashing to sha256:{got}"
            ));
        }
        Ok(bytes.to_vec())
    }
}

/// Pull `image` for `linux/amd64` and assemble its filesystem.
pub async fn pull(
    http: &reqwest::Client,
    image: &str,
    credentials: Option<Credentials>,
) -> Result<Image, String> {
    let reference = Reference::parse(image)?;
    let mut registry = Registry {
        http: http.clone(),
        base: format!("{}://{}", scheme(&reference.registry), reference.registry),
        repository: reference.repository.clone(),
        auth: Auth::None,
        credentials,
    };

    let mut manifest: Value = registry
        .get(
            &format!("manifests/{}", reference.reference),
            MANIFEST_TYPES,
        )
        .await?
        .json()
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
            .get(&format!("manifests/{digest}"), MANIFEST_TYPES)
            .await?
            .json()
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

    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| format!("{image}: a manifest without layers"))?
        .clone();
    let mut pulled = Image {
        env,
        digest,
        ..Image::default()
    };
    let mut budget = MAX_IMAGE_BYTES;
    for layer in layers {
        let digest = layer["digest"].as_str().ok_or("a layer without a digest")?;
        let blob = registry.blob(digest, budget).await?;
        budget = budget.saturating_sub(blob.len() as u64);
        apply_layer(&mut pulled.entries, decompressed(&blob)?)
            .map_err(|e| format!("{image}: layer {digest}: {e}"))?;
    }
    Ok(pulled)
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

/// Apply one layer: its entries replace what is below them, and its
/// whiteouts delete -- `.wh.NAME` one path, `.wh..wh..opq` everything under
/// its directory from lower layers.
fn apply_layer(entries: &mut BTreeMap<String, Entry>, layer: impl Read) -> Result<(), String> {
    let mut archive = tar::Archive::new(layer);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
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
            entries.retain(|p, _| !p.starts_with(&prefix));
            continue;
        }
        if let Some(hidden) = name.strip_prefix(".wh.") {
            let gone = dir.map_or(hidden.to_string(), |d| format!("{d}/{hidden}"));
            let under = format!("{gone}/");
            entries.retain(|p, _| p != &gone && !p.starts_with(&under));
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
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
                Entry::File { mode, data }
            }
            // Devices and FIFOs: the guest's /dev is devtmpfs.
            _ => continue,
        };
        // A path replaced by something that is not a directory takes what
        // was under it with it.
        if !matches!(new, Entry::Dir { .. }) {
            let under = format!("{path}/");
            entries.retain(|p, _| !p.starts_with(&under));
        }
        entries.insert(path, new);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
            apply_layer(&mut entries, decompressed(blob).unwrap()).unwrap();
            assert!(
                matches!(entries.get("etc/hostname"), Some(Entry::File { data, .. }) if data == b"sandbox\n")
            );
        }
    }

    #[test]
    fn paths_that_leave_the_root_are_dropped() {
        assert_eq!(clean("../etc/passwd"), None);
        assert_eq!(clean("./usr//bin/x"), Some("usr/bin/x".into()));
        assert_eq!(clean("/"), None);
    }
}
