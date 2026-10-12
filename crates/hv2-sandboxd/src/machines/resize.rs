//! Resizing: a stopped machine given other processors, other memory, or a
//! larger disk.
//!
//! `PATCH /machines/{name}` `{"cpuCount", "memoryMB", "diskGiB"}`, any of
//! them. The machine is stopped: a size is what a machine boots with, and
//! nothing here changes one under a running guest. It has the new size the
//! next time it starts.
//!
//! # The disk
//!
//! A disk only grows. Making one smaller means deciding what on it to lose,
//! and that is the guest's to decide, not the node's.
//!
//! - A machine from a template has one ext4 filesystem on its whole disk,
//!   which the node made. The node grows that filesystem with the disk, so
//!   the guest finds the space there when it starts.
//! - A machine from an image has whatever partitions the image has. The node
//!   grows the disk and leaves them alone: a cloud image grows its own root
//!   partition when it boots, and any other is grown from inside.

use std::path::Path as FsPath;

use axum::http::HeaderMap;

use super::super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State, StatusCode,
};
use super::{decide_cpus, describe, dir_of, live, named, save, Stored, OPS};

/// The least and the most memory a machine may have, in MiB: what a new
/// machine's is held to.
const MEMORY_MB: std::ops::RangeInclusive<u64> = 128..=1024 * 1024;
/// The most disk, in GiB: a new machine's limit.
const MOST_DISK_GIB: u64 = 2048;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resize {
    #[serde(rename = "cpuCount")]
    cpus: Option<u32>,
    #[serde(rename = "memoryMB")]
    memory_mb: Option<u64>,
    #[serde(rename = "diskGiB")]
    disk_gib: Option<u64>,
}

/// `machine` as `change` would have it, or why it cannot be.
///
/// Nothing is clamped: a size that cannot be is refused, so a caller is
/// never told a machine has what was asked when it has something else.
fn resized(machine: &Stored, change: &Resize) -> Result<Stored, String> {
    if change.cpus.is_none() && change.memory_mb.is_none() && change.disk_gib.is_none() {
        return Err("give cpuCount, memoryMB or diskGiB".to_string());
    }
    let mut next = machine.clone();
    if let Some(cpus) = change.cpus {
        next.cpus = decide_cpus(Some(cpus), machine.image.is_some(), cpus)?;
    }
    if let Some(memory_mb) = change.memory_mb {
        if !MEMORY_MB.contains(&memory_mb) {
            return Err(format!(
                "memoryMB is {} to {}, not {memory_mb}",
                MEMORY_MB.start(),
                MEMORY_MB.end()
            ));
        }
        next.memory_mb = memory_mb;
    }
    if let Some(disk_gib) = change.disk_gib {
        if disk_gib < machine.disk_gib {
            return Err(format!(
                "diskGiB only grows: the disk is {} GiB, and a smaller one would lose what \
                 is on it",
                machine.disk_gib
            ));
        }
        if disk_gib > MOST_DISK_GIB {
            return Err(format!("diskGiB is at most {MOST_DISK_GIB}"));
        }
        next.disk_gib = disk_gib;
    }
    Ok(next)
}

/// Run one of e2fsprogs' tools on `image`; its exit code and what it said.
fn e2fs(tool: &str, args: &[&str], image: &FsPath) -> Result<(i32, String), String> {
    let out = std::process::Command::new(tool)
        .args(args)
        .arg(image)
        .output()
        .map_err(|e| format!("running {tool} (is e2fsprogs installed?): {e}"))?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.code().unwrap_or(-1), said.trim().to_string()))
}

/// Grow the disk at `image` to `gib`, and with `filesystem` the ext4
/// filesystem that is the whole of it.
///
/// The filesystem is checked first, since growing one that is damaged makes
/// more of the damage, and a machine that was stopped has a journal that may
/// want replaying. Then the file is lengthened, then the filesystem is grown
/// into it. Stopping after any step leaves a disk that boots as it did: a
/// file longer than its filesystem is only unused space.
fn grow(image: &FsPath, gib: u64, filesystem: bool) -> Result<(), String> {
    if filesystem {
        // 0 is clean and 1 is errors put right; anything more was not.
        let (code, said) = e2fs("e2fsck", &["-f", "-p"], image)?;
        if code > 1 {
            return Err(format!(
                "the disk's filesystem did not pass its check, so it was not grown: {said}"
            ));
        }
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(image)
        .and_then(|file| file.set_len(gib << 30))
        .map_err(|e| format!("growing {}: {e}", image.display()))?;
    if filesystem {
        let (code, said) = e2fs("resize2fs", &[], image)?;
        if code != 0 {
            return Err(format!(
                "the disk grew and its filesystem did not; it is as large as it was: {said}"
            ));
        }
    }
    Ok(())
}

/// `PATCH /machines/{name}`.
pub(crate) async fn patch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(change): Json<Resize>,
) -> Response {
    let _ops = OPS.lock().await;
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let next = match resized(&machine, &change) {
        Ok(next) => next,
        Err(why) => return api_error(StatusCode::BAD_REQUEST, why),
    };
    if live(|l| l.contains_key(&machine.id)) {
        return api_error(
            StatusCode::CONFLICT,
            format!(
                "machine {name} is running; stop it first: a size is what a machine starts with"
            ),
        );
    }
    if next.disk_gib > machine.disk_gib {
        let image = dir_of(&state, &machine.id).join("root.img");
        let (gib, filesystem) = (next.disk_gib, machine.image.is_none());
        match tokio::task::spawn_blocking(move || grow(&image, gib, filesystem)).await {
            Ok(Ok(())) => {}
            Ok(Err(why)) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, why),
            Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }
    match save(&state, &next) {
        Ok(()) => Json(describe(&next)).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(image: Option<&str>) -> Stored {
        serde_json::from_value(serde_json::json!({
            "machineID": "m-1", "name": "web-01", "templateID": "base", "cpuCount": 2,
            "memoryMB": 1024, "diskGiB": 8, "autostart": true, "restartPolicy": "always",
            "desired": "stopped", "createdAt": 1, "image": image,
        }))
        .unwrap()
    }

    fn change(cpus: Option<u32>, memory_mb: Option<u64>, disk_gib: Option<u64>) -> Resize {
        Resize {
            cpus,
            memory_mb,
            disk_gib,
        }
    }

    #[test]
    fn only_what_is_given_changes() {
        let before = machine(None);
        let after = resized(&before, &change(Some(4), None, None)).unwrap();
        assert_eq!((after.cpus, after.memory_mb, after.disk_gib), (4, 1024, 8));
        let after = resized(&before, &change(None, Some(4096), Some(20))).unwrap();
        assert_eq!((after.cpus, after.memory_mb, after.disk_gib), (2, 4096, 20));
        // The same size again is no change, and is not an error.
        let after = resized(&before, &change(Some(2), Some(1024), Some(8))).unwrap();
        assert_eq!((after.cpus, after.memory_mb, after.disk_gib), (2, 1024, 8));
        assert_eq!((after.id, after.name), (before.id, before.name));
    }

    #[test]
    fn a_size_that_cannot_be_is_refused_and_not_clamped() {
        let before = machine(Some("noble.raw"));
        let most = hv2_core::boot::mptable::MAX_CPUS;
        for (asked, expect) in [
            (change(None, None, None), "give cpuCount"),
            (change(Some(0), None, None), "cpuCount is 1 to"),
            (change(Some(most + 1), None, None), "cpuCount is 1 to"),
            (change(None, Some(127), None), "memoryMB is 128 to"),
            (
                change(None, Some(1024 * 1024 + 1), None),
                "memoryMB is 128 to",
            ),
            (change(None, None, Some(7)), "only grows"),
            (change(None, None, Some(0)), "only grows"),
            (change(None, None, Some(2049)), "at most 2048"),
            // One that cannot be refuses the rest with it.
            (change(Some(4), Some(4096), Some(1)), "only grows"),
        ] {
            let refused = resized(&before, &asked).unwrap_err();
            assert!(refused.contains(expect), "{asked:?}: {refused}");
        }
        assert!(resized(&before, &change(Some(most), Some(128), Some(2048))).is_ok());
    }

    #[test]
    fn a_field_that_is_not_a_size_is_not_ignored() {
        assert!(serde_json::from_str::<Resize>(r#"{"cpuCount": 2}"#).is_ok());
        for body in [r#"{"cpus": 2}"#, r#"{"cpuCount": 2, "name": "other"}"#] {
            assert!(serde_json::from_str::<Resize>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn a_disk_with_no_filesystem_of_the_node_s_is_only_lengthened() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("root.img");
        std::fs::write(&image, b"an image's own partitions, not ext4").unwrap();
        grow(&image, 1, false).unwrap();
        assert_eq!(std::fs::metadata(&image).unwrap().len(), 1 << 30);
        let mut start = [0u8; 35];
        std::io::Read::read_exact(&mut std::fs::File::open(&image).unwrap(), &mut start).unwrap();
        assert_eq!(&start, b"an image's own partitions, not ext4");
        // As a filesystem of the node's it does not pass its check (or
        // there is nothing here to check it with), and it is left as long
        // as it was.
        assert!(grow(&image, 2, true).is_err());
        assert_eq!(std::fs::metadata(&image).unwrap().len(), 1 << 30);
    }
}
