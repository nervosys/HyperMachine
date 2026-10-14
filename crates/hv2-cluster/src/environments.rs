//! Environments: a named set of variables and files a team gives the
//! sandboxes it creates.
//!
//! - `PUT    /environments/{name}` `{"envVars": {...}, "files": [{"path",
//!   "content" | "contentBase64"}]}` makes or replaces one.
//! - `GET    /environments` lists the caller's team's.
//! - `GET    /environments/{name}` is one.
//! - `DELETE /environments/{name}` removes it.
//!
//! A sandbox created with `"environment": "{name}"` starts with them: the
//! variables under its own `envVars`, which win where both name one, and the
//! files written into its guest, owned by the template's user, before the
//! create answers. Served by a node and by a control plane alike, from the
//! cluster store, so every node finds the same environment.
//!
//! # What is kept, and what is returned
//!
//! An environment is kept in the cluster store as given: variable values
//! and file contents included, as the store keeps everything, unencrypted.
//! A secret put in one is as safe as the store. What is returned is the
//! variables' names and the files' paths and sizes, never a value or a
//! file's contents.
//!
//! A change applies to sandboxes created after it. One already running keeps
//! what it started with.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::{now_ms, rfc3339};
use crate::ownership::{RequestTeam, TeamId};
use crate::store::ClusterStore;

/// The most files in one environment.
pub const MOST_FILES: usize = 64;
/// The most bytes of all of its files together.
pub const MOST_FILE_BYTES: usize = 4 << 20;
/// The most variables, and bytes of their names and values: a sandbox's own.
pub const MOST_VARS: usize = 1024;
pub const MOST_VAR_BYTES: usize = 128 * 1024;
/// The most environments a team keeps.
pub const MOST: usize = 256;

/// A file an environment puts in a guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestFile {
    pub path: String,
    /// Its contents, base64 (standard, padded) as stored.
    #[serde(rename = "contentBase64")]
    pub content_base64: String,
    /// A tar archive to unpack into this directory and then remove: how a
    /// cloned repository goes into a guest. Never stored with an
    /// environment; a node makes these when it clones.
    #[serde(
        rename = "unpackInto",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unpack_into: Option<String>,
}

/// Which program clones a repository.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryKind {
    /// A `lit server`'s repository, cloned with `lit`.
    #[default]
    Lit,
    /// A Git repository over `http` or `https`, cloned with `git`.
    Git,
}

impl RepositoryKind {
    fn is_lit(&self) -> bool {
        *self == Self::Lit
    }
}

/// A repository an environment's sandboxes start with: cloned by the node
/// from `remote` into `path` in the guest, its history and all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// The program that clones it: `lit`, the default, or `git`.
    #[serde(default, skip_serializing_if = "RepositoryKind::is_lit")]
    pub kind: RepositoryKind,
    /// A `lit server`'s URL, or a Git remote's. The node clones only from
    /// remotes its operator allows (`--lit-remote`, `--git-remote`).
    pub remote: String,
    /// Where in the guest: absolute, made if it is not there.
    pub path: String,
    /// Checked out after the clone; the remote's default branch unless given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// An account on the remote, logged in to for each clone. Its password
    /// is kept as the rest of an environment is and never returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// The most repositories in one environment.
pub const MOST_REPOSITORIES: usize = 8;

impl GuestFile {
    /// Its bytes.
    ///
    /// # Errors
    ///
    /// Stored contents that are not base64, which only a store written by
    /// something else could hold.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.content_base64)
            .map_err(|e| format!("{}: its stored contents are not base64: {e}", self.path))
    }
}

/// An environment as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    #[serde(rename = "teamID", default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamId>,
    #[serde(rename = "envVars", default)]
    pub env_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub files: Vec<GuestFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<Repository>,
    #[serde(rename = "createdAt")]
    pub created_ms: u64,
    #[serde(rename = "updatedAt")]
    pub updated_ms: u64,
}

impl Environment {
    /// Where the store keeps it: one name per team.
    #[must_use]
    pub fn key(team: Option<&TeamId>, name: &str) -> String {
        format!("{}/{name}", team.map_or("-", TeamId::as_str))
    }

    /// As it is described: names and sizes, no values and no contents.
    #[must_use]
    pub fn describe(&self) -> Value {
        json!({
            "name": self.name,
            "teamID": self.team,
            "envVarNames": self.env_vars.keys().collect::<Vec<_>>(),
            "files": self.files.iter().map(|file| json!({
                "path": file.path,
                "bytes": file.bytes().map_or(0, |b| b.len()),
            })).collect::<Vec<_>>(),
            // Where each comes from and goes, and whose account; never a
            // password.
            "repositories": self.repositories.iter().map(|repo| json!({
                "remote": repo.remote,
                "path": repo.path,
                "branch": repo.branch,
                "username": repo.username,
            })).collect::<Vec<_>>(),
            "createdAt": rfc3339(self.created_ms),
            "updatedAt": rfc3339(self.updated_ms),
        })
    }
}

/// Whether `name` may name an environment.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with('.')
}

/// What a caller sends.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Given {
    #[serde(rename = "envVars", default)]
    env_vars: BTreeMap<String, String>,
    #[serde(default)]
    files: Vec<GivenFile>,
    #[serde(default)]
    repositories: Vec<Repository>,
}

/// Why `repo` cannot be one, if it cannot. `paths` are the files' and the
/// other repositories' paths so far.
fn refused_repository(repo: &Repository, paths: &[&str]) -> Option<String> {
    let remote = &repo.remote;
    let scheme_ok = reqwest::Url::parse(remote).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https" | "lit")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if !scheme_ok {
        return Some(format!(
            "{remote:?}: a repository's remote is an http, https or lit URL with no credentials in it"
        ));
    }
    let path = &repo.path;
    if !path.starts_with('/') || path == "/" || path.contains('\0') || path.len() > 4096 {
        return Some(format!(
            "{path:?}: a repository's path is absolute and not the root"
        ));
    }
    if path.split('/').any(|part| part == "..") {
        return Some(format!("{path:?}: a repository's path has no `..` in it"));
    }
    if paths.iter().any(|other| *other == path) {
        return Some(format!("{path:?} is given twice"));
    }
    let plain = |text: &str| {
        !text.is_empty()
            && text.len() <= 256
            && text
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'\'' && b != b'"' && b != b'\\')
    };
    if repo
        .branch
        .as_deref()
        // A node reads the branch's ref by its name: no `..`, no `/` at
        // either end, nothing that leaves the refs directory.
        .is_some_and(|b| {
            !plain(b)
                || b.starts_with('-')
                || b.starts_with('/')
                || b.ends_with('/')
                || b.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        })
    {
        return Some("a branch is a plain name".to_string());
    }
    if repo.kind == RepositoryKind::Git && (repo.username.is_some() || repo.password.is_some()) {
        return Some(
            "a Git repository is cloned without credentials: give it neither username nor password"
                .to_string(),
        );
    }
    match (&repo.username, &repo.password) {
        (Some(user), Some(password)) if plain(user) && !password.is_empty() => None,
        (None, None) => None,
        _ => Some("give a repository both username and password, or neither".to_string()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GivenFile {
    path: String,
    /// Text.
    content: Option<String>,
    /// Bytes, base64.
    #[serde(rename = "contentBase64")]
    content_base64: Option<String>,
}

/// The variables, files and repositories `given` describes, or why they
/// cannot be.
fn checked(
    given: Given,
) -> Result<(BTreeMap<String, String>, Vec<GuestFile>, Vec<Repository>), String> {
    if given.env_vars.len() > MOST_VARS {
        return Err(format!("at most {MOST_VARS} variables"));
    }
    let mut var_bytes = 0;
    for (name, value) in &given.env_vars {
        let first = name.bytes().next();
        let valid = first.is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid {
            return Err(format!("{name:?} is not a variable name"));
        }
        if value.contains('\0') {
            return Err(format!("the value of {name} holds a NUL byte"));
        }
        var_bytes += name.len() + value.len();
    }
    if var_bytes > MOST_VAR_BYTES {
        return Err(format!("the variables are at most {MOST_VAR_BYTES} bytes"));
    }
    if given.files.len() > MOST_FILES {
        return Err(format!("at most {MOST_FILES} files"));
    }
    let mut files: Vec<GuestFile> = Vec::with_capacity(given.files.len());
    let mut file_bytes = 0;
    for file in given.files {
        let path = file.path;
        if !path.starts_with('/') || path.ends_with('/') || path.contains('\0') || path.len() > 4096
        {
            return Err(format!(
                "{path:?}: a file's path is absolute and names the file"
            ));
        }
        if path.split('/').any(|part| part == "..") {
            return Err(format!("{path:?}: a file's path has no `..` in it"));
        }
        if files.iter().any(|f| f.path == path) {
            return Err(format!("{path:?} is given twice"));
        }
        let bytes = match (file.content, file.content_base64) {
            (Some(text), None) => text.into_bytes(),
            (None, Some(encoded)) => base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|e| format!("{path}: contentBase64 is not base64: {e}"))?,
            _ => {
                return Err(format!(
                    "{path}: give content or contentBase64, one of them"
                ))
            }
        };
        file_bytes += bytes.len();
        files.push(GuestFile {
            path,
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            unpack_into: None,
        });
    }
    if file_bytes > MOST_FILE_BYTES {
        return Err(format!(
            "the files are at most {} MiB together",
            MOST_FILE_BYTES >> 20
        ));
    }
    if given.repositories.len() > MOST_REPOSITORIES {
        return Err(format!("at most {MOST_REPOSITORIES} repositories"));
    }
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    for repo in &given.repositories {
        if let Some(why) = refused_repository(repo, &paths) {
            return Err(why);
        }
        paths.push(&repo.path);
    }
    let repositories = given.repositories.clone();
    Ok((given.env_vars, files, repositories))
}

/// The environment `name` of `team`, if there is one.
///
/// # Errors
///
/// The store could not be read.
pub async fn find(
    store: &dyn ClusterStore,
    team: Option<&TeamId>,
    name: &str,
) -> crate::store::Result<Option<Environment>> {
    Ok(store
        .environments()
        .await?
        .into_iter()
        .find(|env| env.team.as_ref() == team && env.name == name))
}

fn api_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "code": status.as_u16(), "message": message.to_string() })),
    )
        .into_response()
}

fn team_of(team: Option<Extension<RequestTeam>>) -> Option<TeamId> {
    team.and_then(|Extension(team)| team.0)
}

type Store = State<Arc<dyn ClusterStore>>;

/// The routes, over `store`.
pub fn router<S: Clone + Send + Sync + 'static>(store: Arc<dyn ClusterStore>) -> Router<S> {
    Router::<Arc<dyn ClusterStore>>::new()
        .route("/environments", get(list))
        .route(
            "/environments/{name}",
            get(one)
                .put(put)
                .delete(delete)
                // Files are in the body, base64, up to MOST_FILE_BYTES.
                .layer(axum::extract::DefaultBodyLimit::max(
                    MOST_FILE_BYTES * 2 + MOST_VAR_BYTES * 2,
                )),
        )
        .with_state(store)
}

async fn list(State(store): Store, team: Option<Extension<RequestTeam>>) -> Response {
    let team = team_of(team);
    match store.environments().await {
        Ok(all) => Json(
            all.iter()
                .filter(|env| env.team == team)
                .map(Environment::describe)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn one(
    State(store): Store,
    team: Option<Extension<RequestTeam>>,
    Path(name): Path<String>,
) -> Response {
    match find(store.as_ref(), team_of(team).as_ref(), &name).await {
        Ok(Some(env)) => Json(env.describe()).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, format!("no environment {name}")),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn put(
    State(store): Store,
    team: Option<Extension<RequestTeam>>,
    Path(name): Path<String>,
    Json(given): Json<Given>,
) -> Response {
    if !valid_name(&name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "an environment's name is 1 to 64 letters, digits, '-', '_' and '.', not starting with '.'",
        );
    }
    let (env_vars, files, repositories) = match checked(given) {
        Ok(checked) => checked,
        Err(why) => return api_error(StatusCode::BAD_REQUEST, why),
    };
    let team = team_of(team);
    let all = match store.environments().await {
        Ok(all) => all,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let before = all
        .iter()
        .find(|env| env.team == team && env.name == name)
        .cloned();
    if before.is_none() && all.iter().filter(|env| env.team == team).count() >= MOST {
        return api_error(
            StatusCode::CONFLICT,
            format!("a team keeps at most {MOST} environments"),
        );
    }
    let now = now_ms();
    let env = Environment {
        name,
        team,
        env_vars,
        files,
        repositories,
        created_ms: before.as_ref().map_or(now, |b| b.created_ms),
        updated_ms: now,
    };
    match store.put_environment(&env).await {
        Ok(()) => {
            let status = if before.is_some() {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(env.describe())).into_response()
        }
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn delete(
    State(store): Store,
    team: Option<Extension<RequestTeam>>,
    Path(name): Path<String>,
) -> Response {
    let team = team_of(team);
    match store
        .delete_environment(&Environment::key(team.as_ref(), &name))
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => api_error(StatusCode::NOT_FOUND, format!("no environment {name}")),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    fn given(body: Value) -> Result<(BTreeMap<String, String>, Vec<GuestFile>), String> {
        checked(serde_json::from_value(body).map_err(|e| e.to_string())?).map(|(v, f, _)| (v, f))
    }

    #[test]
    fn a_repository_is_a_remote_and_a_place_and_its_password_is_never_returned() {
        let body = json!({"repositories": [
            {"remote": "https://lit.example/team/app", "path": "/srv/app", "branch": "main",
             "username": "ci", "password": "s3cret-pw"},
            {"remote": "lit://10.0.0.5:9418/tools", "path": "/opt/tools"},
        ]});
        let (_, _, repos) = checked(serde_json::from_value(body).unwrap()).unwrap();
        assert_eq!(repos.len(), 2);
        let env = Environment {
            name: "x".into(),
            team: None,
            env_vars: BTreeMap::new(),
            files: Vec::new(),
            repositories: repos,
            created_ms: 0,
            updated_ms: 0,
        };
        let described = env.describe().to_string();
        assert!(
            described.contains("lit.example") && described.contains("\"ci\""),
            "{described}"
        );
        assert!(!described.contains("s3cret-pw"), "{described}");
        for (repo, expect) in [
            (
                json!({"remote": "ftp://x/y", "path": "/a"}),
                "http, https or lit",
            ),
            (
                json!({"remote": "https://u:p@lit.example/a", "path": "/a"}),
                "no credentials",
            ),
            (
                json!({"remote": "not a url", "path": "/a"}),
                "http, https or lit",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "rel"}),
                "absolute",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/"}),
                "absolute",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a/../etc"}),
                "`..`",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "branch": "--upload-pack=x"}),
                "plain name",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "branch": "../../../etc/passwd"}),
                "plain name",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "branch": "/abs"}),
                "plain name",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "branch": "a b"}),
                "plain name",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "username": "ci"}),
                "both",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "password": "x"}),
                "both",
            ),
            (
                json!({"remote": "https://lit.example/a", "path": "/a", "token": "x"}),
                "unknown field",
            ),
        ] {
            let refused = given(json!({"repositories": [repo.clone()]})).unwrap_err();
            assert!(refused.contains(expect), "{repo}: {refused}");
        }
        // A file and a repository in one place.
        let refused = given(json!({"files": [{"path": "/a", "content": "x"}],
                                   "repositories": [{"remote": "https://l/a", "path": "/a"}]}))
        .unwrap_err();
        assert!(refused.contains("twice"), "{refused}");
    }

    #[test]
    fn variables_and_files_are_checked_and_files_kept_as_bytes() {
        let (vars, files) = given(json!({
            "envVars": {"API_URL": "https://x", "_T": ""},
            "files": [
                {"path": "/home/user/.npmrc", "content": "registry=x\n"},
                {"path": "/etc/app/key.bin", "contentBase64": "AAH/"},
            ],
        }))
        .unwrap();
        assert_eq!(vars.len(), 2);
        assert_eq!(files[0].bytes().unwrap(), b"registry=x\n");
        assert_eq!(files[1].bytes().unwrap(), [0, 1, 255]);
        for (body, expect) in [
            (json!({"envVars": {"1X": "a"}}), "not a variable name"),
            (json!({"envVars": {"A-B": "a"}}), "not a variable name"),
            (json!({"envVars": {"A": "a\u{0}"}}), "NUL"),
            (
                json!({"files": [{"path": "rel", "content": "x"}]}),
                "absolute",
            ),
            (
                json!({"files": [{"path": "/d/", "content": "x"}]}),
                "absolute",
            ),
            (
                json!({"files": [{"path": "/a/../etc/shadow", "content": "x"}]}),
                "`..`",
            ),
            (
                json!({"files": [{"path": "/a", "content": "x"}, {"path": "/a", "content": "y"}]}),
                "twice",
            ),
            (json!({"files": [{"path": "/a"}]}), "one of them"),
            (
                json!({"files": [{"path": "/a", "content": "x", "contentBase64": "eA=="}]}),
                "one of them",
            ),
            (
                json!({"files": [{"path": "/a", "contentBase64": "%%%"}]}),
                "not base64",
            ),
            (
                json!({"files": [{"path": "/a", "content": "x".repeat(MOST_FILE_BYTES + 1)}]}),
                "MiB",
            ),
        ] {
            let refused = given(body.clone()).unwrap_err();
            assert!(refused.contains(expect), "{body}: {refused}");
        }
        assert!(given(json!({"secrets": {}})).is_err(), "an unknown field");
    }

    #[test]
    fn a_name_is_plain() {
        for name in ["node-20", "py3.12", "a_b", "A"] {
            assert!(valid_name(name), "{name}");
        }
        for name in ["", ".hidden", "a/b", "a b", "..", &"x".repeat(65)] {
            assert!(!valid_name(name), "{name}");
        }
    }

    /// The routes served, with each request's team taken from a header as
    /// a daemon's authentication would set it.
    async fn serve(store: Arc<dyn ClusterStore>) -> String {
        let app: Router = router(store).layer(axum::middleware::from_fn(
            |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                let team = request
                    .headers()
                    .get("x-test-team")
                    .and_then(|v| v.to_str().ok())
                    .map(|t| TeamId::parse(t).unwrap());
                request.extensions_mut().insert(RequestTeam(team));
                next.run(request).await
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    async fn call(
        base: &str,
        method: &str,
        path: &str,
        team: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = reqwest::Client::new().request(
            reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
            format!("{base}{path}"),
        );
        if let Some(team) = team {
            request = request.header("x-test-team", team);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn a_team_keeps_its_own_and_sees_no_values() {
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let app = serve(Arc::clone(&store)).await;
        let body =
            json!({"envVars": {"TOKEN": "s3cret"}, "files": [{"path": "/a", "content": "hidden"}]});
        let (status, made) = call(
            &app,
            "PUT",
            "/environments/dev",
            Some("team-a"),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        assert_eq!(made["envVarNames"], json!(["TOKEN"]));
        assert_eq!(made["files"], json!([{"path": "/a", "bytes": 6}]));
        assert!(!made.to_string().contains("s3cret") && !made.to_string().contains("hidden"));
        // Replaced, it keeps when it was made.
        let (status, again) = call(
            &app,
            "PUT",
            "/environments/dev",
            Some("team-a"),
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(again["createdAt"], made["createdAt"]);
        assert_eq!(again["envVarNames"], json!([]));
        // Another team has its own of the same name, and does not see A's.
        let (status, _) = call(&app, "PUT", "/environments/dev", Some("team-b"), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED);
        let (_, listed) = call(&app, "GET", "/environments", Some("team-b"), None).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["envVarNames"], json!(["TOKEN"]));
        assert_eq!(
            find(
                store.as_ref(),
                Some(&TeamId::parse("team-a").unwrap()),
                "dev"
            )
            .await
            .unwrap()
            .unwrap()
            .env_vars
            .len(),
            0
        );
        let (status, _) = call(&app, "GET", "/environments/dev", Some("team-c"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(&app, "DELETE", "/environments/dev", Some("team-c"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(&app, "DELETE", "/environments/dev", Some("team-a"), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, listed) = call(&app, "GET", "/environments", Some("team-a"), None).await;
        assert_eq!(listed, json!([]));
        let (status, _) = call(
            &app,
            "PUT",
            "/environments/.x",
            Some("team-a"),
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
