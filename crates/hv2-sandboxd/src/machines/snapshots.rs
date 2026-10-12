//! Snapshots of a machine's disk: kept beside it, restored over it, and
//! downloaded whole.
//!
//! - `POST   /machines/{name}/snapshots` `{"name"}` takes one.
//! - `GET    /machines/{name}/snapshots` lists them, oldest first.
//! - `GET    /machines/{name}/snapshots/{id}` is one.
//! - `POST   /machines/{name}/snapshots/{id}/restore` puts it back.
//! - `GET    /machines/{name}/snapshots/{id}/disk` is its disk, raw.
//! - `DELETE /machines/{name}/snapshots/{id}` removes it.
//!
//! # What a snapshot is
//!
//! The machine's root disk as it was, copied while the machine is stopped:
//! files and nothing else. Its memory is not in it, and a machine restored
//! from one boots. Taking or restoring one while the machine runs is
//! refused, since a disk copied under a running guest is a disk caught
//! half-written.
//!
//! The copy is `cp --sparse=always --reflink=auto`: on a filesystem that
//! shares blocks (XFS, Btrfs) it takes no time and no room until the disks
//! differ, and elsewhere it costs what the disk holds, not its size. Each
//! snapshot's answer says how many bytes it holds on the node's disk.
//!
//! A machine's cloud-init seed is not in a snapshot: it is made once, at
//! creation, and restoring the root disk leaves it as it is.
//!
//! # Downloading
//!
//! The disk comes as one raw image, as long as the disk is, streamed. Its
//! holes come as zeros, so a download is as large as the disk; a client that
//! wants it sparse makes it so on its side.

use std::path::{Path as FsPath, PathBuf};

use axum::body::Body;
use axum::http::HeaderMap;
use serde::Serialize;
use serde_json::{json, Value};

use super::super::{
    api_error, now_ms, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State,
    StatusCode,
};
use super::{dir_of, live, named, save, Stored, OPS};

/// The most snapshots one machine may have.
pub(crate) const MOST: usize = 32;

/// What is kept of a snapshot beside its disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    #[serde(rename = "snapshotID")]
    id: String,
    name: String,
    #[serde(rename = "createdAt")]
    created_ms: u64,
    /// The machine's disk size when it was taken, which a restore gives it
    /// back.
    #[serde(rename = "diskGiB")]
    disk_gib: u64,
}

fn root_of(state: &AppState, machine: &Stored) -> PathBuf {
    dir_of(state, &machine.id).join("snapshots")
}

fn valid_id(id: &str) -> bool {
    id.len() == 20 && id.starts_with("snap-") && id[5..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.chars().any(char::is_control)
        && name.trim() == name
}

/// Bytes the file at `path` holds on the node's disk, not its length.
fn allocated(path: &FsPath) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).map_or(0, |m| m.blocks() * 512)
}

fn describe(dir: &FsPath, meta: &Meta) -> Value {
    json!({
        "snapshotID": meta.id,
        "name": meta.name,
        "createdAt": crate_rfc3339(meta.created_ms),
        "diskGiB": meta.disk_gib,
        "bytesOnNode": allocated(&dir.join(&meta.id).join("root.img")),
    })
}

fn crate_rfc3339(ms: u64) -> String {
    hv2_cluster::model::rfc3339(ms)
}

/// Every snapshot of `machine`, oldest first.
fn all(state: &AppState, machine: &Stored) -> Vec<Meta> {
    let mut found: Vec<Meta> = std::fs::read_dir(root_of(state, machine))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read(entry.path().join("snapshot.json")).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    found.sort_by(|a: &Meta, b: &Meta| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
    found
}

fn one(state: &AppState, machine: &Stored, id: &str) -> Option<Meta> {
    if !valid_id(id) {
        return None;
    }
    let bytes = std::fs::read(root_of(state, machine).join(id).join("snapshot.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Copy `from` to `to`, sharing blocks where the filesystem can and leaving
/// holes as holes.
fn copy_disk(from: &FsPath, to: &FsPath) -> Result<(), String> {
    let out = std::process::Command::new("cp")
        .args(["--sparse=always", "--reflink=auto", "--"])
        .arg(from)
        .arg(to)
        .output()
        .map_err(|e| format!("running cp: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(to);
        return Err(format!(
            "copying the disk: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    std::fs::File::open(to)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("{}: {e}", to.display()))
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn stopped(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    doing: &str,
) -> Result<Stored, Response> {
    let machine = named(state, headers, name)?;
    if live(|l| l.contains_key(&machine.id)) {
        return Err(api_error(
            StatusCode::CONFLICT,
            format!(
                "machine {name} is running; stop it to {doing}, so its disk is not half-written"
            ),
        ));
    }
    Ok(machine)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Take {
    name: Option<String>,
}

/// `POST /machines/{name}/snapshots`.
pub(crate) async fn take(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    body: Option<Json<Take>>,
) -> Response {
    let _ops = OPS.lock().await;
    let machine = match stopped(&state, &headers, &name, "take a snapshot") {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let created_ms = now_ms();
    let id = format!("snap-{}", &uuid::Uuid::new_v4().simple().to_string()[..15]);
    let label = body
        .and_then(|Json(take)| take.name)
        .unwrap_or_else(|| crate_rfc3339(created_ms));
    if !valid_name(&label) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "a snapshot's name is 1 to 128 characters, with no control characters and no space at either end",
        );
    }
    if all(&state, &machine).len() >= MOST {
        return api_error(
            StatusCode::CONFLICT,
            format!("a machine keeps at most {MOST} snapshots; delete one first"),
        );
    }
    let root = root_of(&state, &machine);
    let dir = root.join(&id);
    let meta = Meta {
        id: id.clone(),
        name: label,
        created_ms,
        disk_gib: machine.disk_gib,
    };
    let disk = dir_of(&state, &machine.id).join("root.img");
    let made = {
        let (dir, meta) = (dir.clone(), meta.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            // Into a directory of its own, named only once it is whole: a
            // snapshot is listed when its record is there, and the record is
            // written last.
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            copy_disk(&disk, &dir.join("root.img"))?;
            let record = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
            let temporary = dir.join(".snapshot.json");
            std::fs::write(&temporary, record).map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, dir.join("snapshot.json")).map_err(|e| e.to_string())
        })
        .await
    };
    match made {
        Ok(Ok(())) => (StatusCode::CREATED, Json(describe(&root, &meta))).into_response(),
        Ok(Err(why)) => {
            let _ = std::fs::remove_dir_all(&dir);
            api_error(StatusCode::INTERNAL_SERVER_ERROR, why)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            api_error(StatusCode::INTERNAL_SERVER_ERROR, e)
        }
    }
}

/// `GET /machines/{name}/snapshots`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let root = root_of(&state, &machine);
    Json(
        all(&state, &machine)
            .iter()
            .map(|meta| describe(&root, meta))
            .collect::<Vec<_>>(),
    )
    .into_response()
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn found(state: &AppState, machine: &Stored, name: &str, id: &str) -> Result<Meta, Response> {
    one(state, machine, id).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no snapshot {id}"),
        )
    })
}

/// `GET /machines/{name}/snapshots/{id}`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    match found(&state, &machine, &name, &id) {
        Ok(meta) => Json(describe(&root_of(&state, &machine), &meta)).into_response(),
        Err(response) => response,
    }
}

/// `POST /machines/{name}/snapshots/{id}/restore`.
pub(crate) async fn restore(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let _ops = OPS.lock().await;
    let mut machine = match stopped(&state, &headers, &name, "restore a snapshot") {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let meta = match found(&state, &machine, &name, &id) {
        Ok(meta) => meta,
        Err(response) => return response,
    };
    let from = root_of(&state, &machine).join(&id).join("root.img");
    let disk = dir_of(&state, &machine.id).join("root.img");
    let copied = tokio::task::spawn_blocking(move || -> Result<(), String> {
        // Beside the disk, then over it in one rename: a restore that fails
        // part way leaves the disk as it was.
        let incoming = disk.with_extension("restoring");
        let _ = std::fs::remove_file(&incoming);
        copy_disk(&from, &incoming)?;
        std::fs::rename(&incoming, &disk).map_err(|e| format!("{}: {e}", disk.display()))
    })
    .await;
    match copied {
        Ok(Ok(())) => {}
        Ok(Err(why)) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, why),
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
    machine.disk_gib = meta.disk_gib;
    if let Err(e) = save(&state, &machine) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(super::describe(&machine)).into_response()
}

/// `GET /machines/{name}/snapshots/{id}/disk`: the disk, raw, streamed.
pub(crate) async fn disk(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    if let Err(response) = found(&state, &machine, &name, &id) {
        return response;
    }
    let path = root_of(&state, &machine).join(&id).join("root.img");
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let length = match file.metadata().await {
        Ok(meta) => meta.len(),
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("content-length", length)
        .header(
            "content-disposition",
            format!("attachment; filename=\"{name}-{id}.img\""),
        )
        .body(Body::from_stream(super::super::volumes::file_stream(file)))
        .unwrap_or_else(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// `DELETE /machines/{name}/snapshots/{id}`.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let _ops = OPS.lock().await;
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    if let Err(response) = found(&state, &machine, &name, &id) {
        return response;
    }
    match std::fs::remove_dir_all(root_of(&state, &machine).join(&id)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_one_this_made_and_names_no_other_path() {
        assert!(valid_id("snap-0123456789abcde"));
        for id in [
            "",
            "snap-",
            "snap-0123456789abcd",
            "snap-0123456789abcdef",
            "snap-0123456789abcdg",
            "../../machine.json",
            "snap-../../../etc/x",
            "SNAP-0123456789abcde",
        ] {
            assert!(!valid_id(id), "{id}");
        }
    }

    #[test]
    fn a_name_is_text_of_a_sensible_length() {
        for name in [
            "before upgrade",
            "2026-10-12T15:00:00.000Z",
            "ü",
            &"x".repeat(128),
        ] {
            assert!(valid_name(name), "{name}");
        }
        for name in ["", " lead", "trail ", "a\nb", "tab\there", &"x".repeat(129)] {
            assert!(!valid_name(name), "{name:?}");
        }
    }

    #[test]
    fn a_copy_keeps_holes_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("disk.img");
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = std::fs::File::create(&from).unwrap();
            file.write_all(b"start").unwrap();
            file.seek(SeekFrom::Start(64 << 20)).unwrap();
            file.write_all(b"far").unwrap();
        }
        let to = dir.path().join("copy.img");
        copy_disk(&from, &to).unwrap();
        let (a, b) = (std::fs::read(&from).unwrap(), std::fs::read(&to).unwrap());
        assert_eq!(a, b);
        // 64 MiB long, and nearly all of it a hole in the copy too.
        assert!(allocated(&to) < 4 << 20, "{} bytes on disk", allocated(&to));
        // A copy that cannot be made says so and leaves nothing.
        let refused = copy_disk(&dir.path().join("absent.img"), &dir.path().join("x.img"));
        assert!(refused.is_err());
        assert!(!dir.path().join("x.img").exists());
    }
}
