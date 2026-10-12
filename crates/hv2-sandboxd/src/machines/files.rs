//! Files: bytes put into a machine's guest, and taken out of it.
//!
//! - `PUT /machines/{name}/files?path=/abs/path&user=NAME` writes the
//!   request's body to that path in the guest, making the directories above
//!   it. With `user`, the file and those directories are that guest user's.
//! - `GET /machines/{name}/files?path=/abs/path` answers with the file's
//!   bytes.
//!
//! Both go through the guest agent, as `exec` does, and so need one: a
//! machine from a template has it, and one from an image has it when
//! created with `guestAgent`.
//!
//! A file is held whole in the node's memory on its way through, so it is
//! at most [`MAX_FILE`]. Something larger belongs on a disk, or comes over
//! the machine's network.
//!
//! A write is in place, a piece at a time. One that fails part way leaves
//! what it had written: it is not swapped in whole at the end.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::Query;
use axum::http::{header, HeaderMap};

use super::super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Path, Response, State, StatusCode,
};
use super::running;

/// The most one file may be, either way.
pub(crate) const MAX_FILE: usize = 64 << 20;

/// How long the guest has for each piece of a file.
const PIECE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
pub(crate) struct FileQuery {
    /// Where in the guest. Absolute: a file sent from outside has no
    /// directory of its own to be relative to.
    path: String,
    /// Who owns what a write makes; root unless given. Not for a read.
    user: Option<String>,
}

/// Why `path` cannot name a file in the guest, when it cannot.
fn refused_path(path: &str) -> Option<&'static str> {
    if !path.starts_with('/') {
        Some("path must be absolute")
    } else if path.contains('\0') {
        Some("path cannot contain NUL")
    } else if path.ends_with('/') {
        Some("path names a directory; give the file's own name")
    } else if path.len() > 4096 {
        Some("path is at most 4096 bytes")
    } else {
        None
    }
}

/// What the guest's reason for refusing is to a caller: theirs to put right
/// where it says so, and otherwise a failure between the node and the guest.
fn status_of(reason: &str) -> StatusCode {
    // The guest's own errors, as its agent words them.
    if reason.contains("(os error 2)") {
        StatusCode::NOT_FOUND
    } else if reason.contains("(os error 13)") {
        StatusCode::FORBIDDEN
    } else if reason.contains("-byte limit") {
        StatusCode::PAYLOAD_TOO_LARGE
    } else if reason.contains("is a directory")
        // A directory to be made where a file is, and a file beneath one.
        || reason.contains("(os error 17)")
        || reason.contains("(os error 20)")
        || reason.contains("(os error 21)")
        || reason.contains("no user ")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::BAD_GATEWAY
    }
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn agent_of(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
) -> Result<Arc<super::AgentVM>, Response> {
    match running(state, headers, name)? {
        (vm, true) => Ok(vm),
        (_, false) => Err(api_error(
            StatusCode::CONFLICT,
            format!(
                "machine {name} has no guest agent answering, and files go through one; see \
                 its exec"
            ),
        )),
    }
}

/// `PUT /machines/{name}/files?path=`.
pub(crate) async fn put(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(query): Query<FileQuery>,
    body: Bytes,
) -> Response {
    if let Some(why) = refused_path(&query.path) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    if query.user.as_deref().is_some_and(|u| u.trim().is_empty()) {
        return api_error(StatusCode::BAD_REQUEST, "user is empty");
    }
    let vm = match agent_of(&state, &headers, &name) {
        Ok(vm) => vm,
        Err(response) => return response,
    };
    match vm
        .write_file_in_guest_as(
            &query.path,
            body.to_vec(),
            query.user.as_deref(),
            PIECE_TIMEOUT,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let reason = e.to_string();
            api_error(status_of(&reason), reason)
        }
    }
}

/// `GET /machines/{name}/files?path=`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(query): Query<FileQuery>,
) -> Response {
    if let Some(why) = refused_path(&query.path) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    if query.user.is_some() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "user is for a write: a file is read as root",
        );
    }
    let vm = match agent_of(&state, &headers, &name) {
        Ok(vm) => vm,
        Err(response) => return response,
    };
    match vm
        .read_file_in_guest(&query.path, MAX_FILE as u64, PIECE_TIMEOUT)
        .await
    {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        Err(e) => {
            let reason = e.to_string();
            api_error(status_of(&reason), reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_absolute_and_names_a_file() {
        for path in ["/etc/hosts", "/a", "/srv/app/with space/and-ü.txt"] {
            assert_eq!(refused_path(path), None, "{path}");
        }
        for (path, expect) in [
            ("", "absolute"),
            ("etc/hosts", "absolute"),
            ("./x", "absolute"),
            ("/etc/", "directory"),
            ("/", "directory"),
            ("/a\0b", "NUL"),
        ] {
            let why = refused_path(path).unwrap_or_else(|| panic!("{path:?} accepted"));
            assert!(why.contains(expect), "{path:?}: {why}");
        }
        assert!(refused_path(&format!("/{}", "x".repeat(4096))).is_some());
        assert_eq!(refused_path(&format!("/{}", "x".repeat(4095))), None);
    }

    #[test]
    fn the_guest_s_reason_decides_whose_failure_it_is() {
        for (reason, status) in [
            (
                "reading /nope: No such file or directory (os error 2)",
                StatusCode::NOT_FOUND,
            ),
            (
                "creating /proc/x: Permission denied (os error 13)",
                StatusCode::FORBIDDEN,
            ),
            (
                "/big is 99999999 bytes, over the 67108864-byte limit",
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            ("reading /etc: it is a directory", StatusCode::BAD_REQUEST),
            (
                "creating /etc/hosts/x: Not a directory (os error 20)",
                StatusCode::BAD_REQUEST,
            ),
            (
                "creating the directory of /tmp/f/below: File exists (os error 17)",
                StatusCode::BAD_REQUEST,
            ),
            (
                "owning /tmp/x: no user nobody-here in this sandbox",
                StatusCode::BAD_REQUEST,
            ),
            // Anything else is between the node and the guest.
            ("guest agent did not answer", StatusCode::BAD_GATEWAY),
            ("", StatusCode::BAD_GATEWAY),
        ] {
            assert_eq!(status_of(reason), status, "{reason}");
        }
    }
}
