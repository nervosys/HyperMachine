//! The files in a machine's disk snapshot, read without booting it.
//!
//! - `GET /machines/{name}/snapshots/{id}/files?path=/etc&partition=N`
//!   lists a directory.
//! - `GET /machines/{name}/snapshots/{id}/file?path=/etc/hostname` is one
//!   file's bytes.
//!
//! Read with e2fsprogs' `debugfs`, which opens an ext2, ext3 or ext4
//! filesystem in an image file and changes nothing in it. A template
//! machine's disk is one such filesystem. A disk image has partitions: the
//! filesystem read is the one `partition` names (1-based, as `lsblk` counts
//! them), or else the largest ext filesystem among them, which is the root of
//! a distribution's cloud image. A filesystem that is not ext (XFS, Btrfs)
//! is not read.
//!
//! # Paths
//!
//! A path never reaches `debugfs` as text. It is walked a name at a time
//! from the root, each directory listed by its inode number, and only inode
//! numbers are put in a `debugfs` request; so a name with a space, a quote or
//! anything else is matched as the bytes it is and cannot be read as a
//! command. Symbolic links are listed as such and not followed.

use std::path::Path as FsPath;

use axum::http::{header, HeaderMap};
use serde::Deserialize;
use serde_json::{json, Value};

use super::super::{
    api_error, AppState, Arc, IntoResponse, Json, Path, Response, State, StatusCode,
};
use super::named;

/// The most bytes of one file read out of a snapshot.
pub(crate) const MAX_FILE: usize = 64 << 20;
/// An ext superblock's magic, at this offset in its filesystem.
const EXT_MAGIC_AT: u64 = 1080;
const EXT_MAGIC: [u8; 2] = [0x53, 0xEF];
const SECTOR: u64 = 512;

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    name: String,
    size: Option<u64>,
}

impl Entry {
    fn kind(&self) -> &'static str {
        match self.mode & 0o170_000 {
            0o040_000 => "directory",
            0o100_000 => "file",
            0o120_000 => "symlink",
            _ => "other",
        }
    }
}

/// Entries as `debugfs`'s `ls -p` prints them: `/inode/mode/uid/gid/name/size/`.
fn parse_listing(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let inner = line.strip_prefix('/')?.strip_suffix('/')?;
            let mut fields = inner.splitn(5, '/');
            let inode = fields.next()?.parse().ok()?;
            let mode = u32::from_str_radix(fields.next()?, 8).ok()?;
            let uid = fields.next()?.parse().ok()?;
            let gid = fields.next()?.parse().ok()?;
            // The name, then the size: a name has no '/' in it.
            let (name, size) = fields.next()?.rsplit_once('/')?;
            if inode == 0 || name.is_empty() {
                return None;
            }
            Some(Entry {
                inode,
                mode,
                uid,
                gid,
                name: name.to_string(),
                size: size.parse().ok(),
            })
        })
        .collect()
}

/// Where an ext filesystem starts in `head`, the first sectors of a disk, and
/// which partition it is. `read_at` reads two bytes at an offset of the disk.
fn find_ext(
    head: &[u8],
    partition: Option<u32>,
    read_at: impl Fn(u64) -> Option<[u8; 2]>,
) -> Result<(u64, Option<u32>), String> {
    let is_ext = |offset: u64| read_at(offset + EXT_MAGIC_AT) == Some(EXT_MAGIC);
    if partition.is_none() && is_ext(0) {
        return Ok((0, None));
    }
    let mut parts: Vec<(u32, u64, u64)> = Vec::new(); // number, start, length
    if head.len() >= 512 && head[510..512] == [0x55, 0xAA] {
        let protective = head[0x1BE + 4] == 0xEE;
        if protective && head.len() >= 1024 && &head[512..520] == b"EFI PART" {
            let le64 = |b: &[u8]| u64::from_le_bytes(b.try_into().unwrap_or([0; 8]));
            let le32 = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap_or([0; 4]));
            let entries_lba = le64(&head[512 + 72..512 + 80]);
            let count = le32(&head[512 + 80..512 + 84]).min(256);
            let size = u64::from(le32(&head[512 + 84..512 + 88]));
            for index in 0..count {
                let at = entries_lba * SECTOR + u64::from(index) * size;
                let (Some(entry), true) = (head.get(at as usize..(at + 48) as usize), size >= 48)
                else {
                    break;
                };
                if entry[..16].iter().all(|b| *b == 0) {
                    continue;
                }
                let (first, last) = (le64(&entry[32..40]), le64(&entry[40..48]));
                if last >= first {
                    parts.push((index + 1, first * SECTOR, (last - first + 1) * SECTOR));
                }
            }
        } else {
            for index in 0..4u32 {
                let at = 0x1BE + 16 * index as usize;
                let kind = head[at + 4];
                let start = u32::from_le_bytes(head[at + 8..at + 12].try_into().unwrap_or([0; 4]));
                let sectors =
                    u32::from_le_bytes(head[at + 12..at + 16].try_into().unwrap_or([0; 4]));
                if kind != 0 && sectors != 0 {
                    parts.push((
                        index + 1,
                        u64::from(start) * SECTOR,
                        u64::from(sectors) * SECTOR,
                    ));
                }
            }
        }
    }
    if let Some(wanted) = partition {
        let (number, start, _) = parts
            .iter()
            .find(|(number, _, _)| *number == wanted)
            .ok_or_else(|| format!("the disk has no partition {wanted}"))?;
        return if is_ext(*start) {
            Ok((*start, Some(*number)))
        } else {
            Err(format!(
                "partition {wanted} holds no ext2, ext3 or ext4 filesystem, which is all this reads"
            ))
        };
    }
    parts
        .iter()
        .filter(|(_, start, _)| is_ext(*start))
        .max_by_key(|(_, _, length)| *length)
        .map(|(number, start, _)| (*start, Some(*number)))
        .ok_or_else(|| {
            "the disk holds no ext2, ext3 or ext4 filesystem, which is all this reads".to_string()
        })
}

/// Run one `debugfs` request against the filesystem at `offset` in `image`;
/// what it printed, or what it complained of.
fn debugfs(image: &FsPath, offset: u64, request: &str) -> Result<Vec<u8>, String> {
    let mut spec = image.as_os_str().to_owned();
    if offset != 0 {
        spec.push(format!("?offset={offset}"));
    }
    let out = std::process::Command::new("debugfs")
        .arg("-R")
        .arg(request)
        .arg(spec)
        .output()
        .map_err(|e| format!("running debugfs (is e2fsprogs installed?): {e}"))?;
    // It says what went wrong on standard error, after its version line,
    // and exits 0 regardless.
    let complaint: Vec<&str> = std::str::from_utf8(&out.stderr)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.starts_with("debugfs ") && !line.trim().is_empty())
        .collect();
    if !complaint.is_empty() || !out.status.success() {
        return Err(complaint.join("; "));
    }
    Ok(out.stdout)
}

/// The entry `path` names, walked from the root by inode.
fn resolve(image: &FsPath, offset: u64, path: &str) -> Result<Entry, (StatusCode, String)> {
    let bad = |why: String| (StatusCode::BAD_REQUEST, why);
    let gone = |why: String| (StatusCode::NOT_FOUND, why);
    if !path.starts_with('/') {
        return Err(bad(format!("{path:?}: a path is absolute")));
    }
    let mut here = Entry {
        inode: 2,
        mode: 0o040_755,
        uid: 0,
        gid: 0,
        name: "/".into(),
        size: None,
    };
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if part == "." || part == ".." {
            return Err(bad(format!("{path:?}: a path has no `.` or `..` in it")));
        }
        if here.kind() != "directory" {
            return Err(bad(format!("{path:?}: {} is not a directory", here.name)));
        }
        let listing = debugfs(image, offset, &format!("ls -p <{}>", here.inode)).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reading the disk: {e}"),
            )
        })?;
        here = parse_listing(&String::from_utf8_lossy(&listing))
            .into_iter()
            .find(|entry| entry.name == part)
            .ok_or_else(|| gone(format!("{path:?}: there is no {part:?}")))?;
    }
    Ok(here)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct At {
    path: Option<String>,
    partition: Option<u32>,
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn disk_of(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    id: &str,
    partition: Option<u32>,
) -> Result<(std::path::PathBuf, u64, Option<u32>), Response> {
    let machine = named(state, headers, name)?;
    let image = super::snapshots::disk_path(state, &machine, id).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no snapshot {id}"),
        )
    })?;
    let mut head = vec![0u8; 64 * 1024];
    let file =
        std::fs::File::open(&image).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let read_at = |offset: u64| {
        use std::os::unix::fs::FileExt;
        let mut two = [0u8; 2];
        file.read_exact_at(&mut two, offset).ok().map(|()| two)
    };
    {
        use std::os::unix::fs::FileExt;
        let read = file.read_at(&mut head, 0).unwrap_or(0);
        head.truncate(read);
    }
    let (offset, partition) =
        find_ext(&head, partition, read_at).map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok((image, offset, partition))
}

/// `GET /machines/{name}/snapshots/{id}/files?path=`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
    axum::extract::Query(at): axum::extract::Query<At>,
) -> Response {
    let (image, offset, partition) = match disk_of(&state, &headers, &name, &id, at.partition) {
        Ok(found) => found,
        Err(response) => return response,
    };
    let path = at.path.unwrap_or_else(|| "/".into());
    let listed = tokio::task::spawn_blocking(move || {
        let dir = resolve(&image, offset, &path)?;
        if dir.kind() != "directory" {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{path:?} is a {}, not a directory", dir.kind()),
            ));
        }
        let text = debugfs(&image, offset, &format!("ls -p <{}>", dir.inode)).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reading the disk: {e}"),
            )
        })?;
        let entries: Vec<Value> = parse_listing(&String::from_utf8_lossy(&text))
            .into_iter()
            .filter(|entry| entry.name != "." && entry.name != "..")
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "type": entry.kind(),
                    "mode": format!("{:o}", entry.mode & 0o7777),
                    "uid": entry.uid,
                    "gid": entry.gid,
                    "size": if entry.kind() == "directory" { None } else { entry.size },
                })
            })
            .collect();
        Ok(json!({"path": path, "partition": partition, "entries": entries}))
    })
    .await;
    match listed {
        Ok(Ok(answer)) => Json(answer).into_response(),
        Ok(Err((status, why))) => api_error(status, why),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// `GET /machines/{name}/snapshots/{id}/file?path=`.
pub(crate) async fn file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, String)>,
    axum::extract::Query(at): axum::extract::Query<At>,
) -> Response {
    let Some(path) = at.path else {
        return api_error(StatusCode::BAD_REQUEST, "give the file's path");
    };
    let (image, offset, _) = match disk_of(&state, &headers, &name, &id, at.partition) {
        Ok(found) => found,
        Err(response) => return response,
    };
    let read = tokio::task::spawn_blocking(move || {
        let entry = resolve(&image, offset, &path)?;
        if entry.kind() != "file" {
            let follow = if entry.kind() == "symlink" {
                "; symbolic links are not followed"
            } else {
                ""
            };
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{path:?} is a {}, not a file{follow}", entry.kind()),
            ));
        }
        if entry.size.is_some_and(|size| size > MAX_FILE as u64) {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("{path:?} is over {} MiB; download the disk", MAX_FILE >> 20),
            ));
        }
        debugfs(&image, offset, &format!("cat <{}>", entry.inode)).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reading the disk: {e}"),
            )
        })
    })
    .await;
    match read {
        Ok(Ok(bytes)) => {
            ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
        }
        Ok(Err((status, why))) => api_error(status, why),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_read_with_names_that_hold_anything_but_a_slash() {
        let text = "/2/040755/0/0/.//\n/2/040755/0/0/..//\n/12/0100644/1000/1000/a file \"x\" $(y)/42/\n\
                    /13/0120777/0/0/os-release/21/\n/14/040700/0/0/root//\nnot a line\n/0/0/0/0/gone//\n";
        let entries = parse_listing(text);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [".", "..", "a file \"x\" $(y)", "os-release", "root"]
        );
        assert_eq!(entries[2].kind(), "file");
        assert_eq!(entries[2].size, Some(42));
        assert_eq!((entries[2].uid, entries[2].mode & 0o7777), (1000, 0o644));
        assert_eq!(entries[3].kind(), "symlink");
        assert_eq!(entries[4].kind(), "directory");
        assert_eq!(entries[4].size, None);
    }

    /// A disk of `sectors`, with an ext magic at each of `ext_at`.
    fn disk(head: Vec<u8>, ext_at: &'static [u64]) -> (Vec<u8>, impl Fn(u64) -> Option<[u8; 2]>) {
        (head, move |offset: u64| {
            ext_at
                .iter()
                .any(|start| start + EXT_MAGIC_AT == offset)
                .then_some(EXT_MAGIC)
        })
    }

    fn gpt(parts: &[(u64, u64)]) -> Vec<u8> {
        let mut head = vec![0u8; 512 * 34];
        head[0x1BE + 4] = 0xEE;
        head[510] = 0x55;
        head[511] = 0xAA;
        head[512..520].copy_from_slice(b"EFI PART");
        head[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        head[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        head[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        for (index, (first, last)) in parts.iter().enumerate() {
            let at = 1024 + index * 128;
            head[at] = 1; // a type GUID that is not zero
            head[at + 32..at + 40].copy_from_slice(&first.to_le_bytes());
            head[at + 40..at + 48].copy_from_slice(&last.to_le_bytes());
        }
        head
    }

    #[test]
    fn a_disk_s_filesystem_is_found_whole_or_by_partition() {
        // A filesystem on the whole disk, as a template machine's is.
        let (head, read) = disk(vec![0; 4096], &[0]);
        assert_eq!(find_ext(&head, None, read), Ok((0, None)));
        // A cloud image: a large root, a small /boot, a partition that is
        // not ext. The largest ext one is the root.
        let parts = [(2_099_200, 7_339_998), (2048, 10239), (227_328, 2_097_152)];
        static EXTS: [u64; 2] = [2_099_200 * 512, 227_328 * 512];
        let (head, read) = disk(gpt(&parts), &EXTS);
        assert_eq!(find_ext(&head, None, &read), Ok((2_099_200 * 512, Some(1))));
        assert_eq!(
            find_ext(&head, Some(3), &read),
            Ok((227_328 * 512, Some(3)))
        );
        assert!(find_ext(&head, Some(2), &read)
            .unwrap_err()
            .contains("no ext2"));
        assert!(find_ext(&head, Some(9), &read)
            .unwrap_err()
            .contains("no partition 9"));
        // MBR.
        let mut mbr = vec![0u8; 512];
        mbr[510] = 0x55;
        mbr[511] = 0xAA;
        mbr[0x1BE + 4] = 0x83;
        mbr[0x1BE + 8..0x1BE + 12].copy_from_slice(&2048u32.to_le_bytes());
        mbr[0x1BE + 12..0x1BE + 16].copy_from_slice(&100_000u32.to_le_bytes());
        static MBR_EXT: [u64; 1] = [2048 * 512];
        let (head, read) = disk(mbr, &MBR_EXT);
        assert_eq!(find_ext(&head, None, read), Ok((2048 * 512, Some(1))));
        // Nothing ext anywhere.
        let (head, read) = disk(vec![0; 4096], &[]);
        assert!(find_ext(&head, None, read).unwrap_err().contains("no ext2"));
    }
}
