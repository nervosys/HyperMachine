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
}

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

/// The variables and files `given` describes, or why they cannot be.
fn checked(given: Given) -> Result<(BTreeMap<String, String>, Vec<GuestFile>), String> {
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
        });
    }
    if file_bytes > MOST_FILE_BYTES {
        return Err(format!(
            "the files are at most {} MiB together",
            MOST_FILE_BYTES >> 20
        ));
    }
    Ok((given.env_vars, files))
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
    let (env_vars, files) = match checked(given) {
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
        checked(serde_json::from_value(body).map_err(|e| e.to_string())?)
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
