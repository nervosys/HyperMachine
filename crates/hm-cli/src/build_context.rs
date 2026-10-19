//! A build's context directory, packed as the tar a `COPY` reads from.
//!
//! The same archive serves every `COPY` of a build: the node picks each
//! step's source out of it. It is made the same way each time (sorted paths,
//! no owners, no times) so its hash, which names the upload, changes only
//! when the files do. `.dockerignore` is honoured.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// The most a context may hold, as the node's own upload limit is 4 GiB and
/// the client holds the archive in memory.
const MOST_BYTES: u64 = 512 << 20;

/// The patterns in a `.dockerignore`, in order.
pub fn ignore_patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.trim_start_matches("./").to_string())
        .collect()
}

/// Whether the context-relative `path` (with `/` separators) is ignored: the
/// last pattern that matches it, or one of its parent directories, decides.
pub fn ignored(path: &str, patterns: &[String]) -> bool {
    let mut result = false;
    for pattern in patterns {
        let (negated, pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern.as_str()),
        };
        let pattern = pattern.trim_start_matches('/').trim_end_matches('/');
        let segments: Vec<&str> = path.split('/').collect();
        let wanted: Vec<&str> = pattern.split('/').collect();
        // The path itself, or any directory above it.
        let hit = (1..=segments.len()).any(|n| matches_segments(&segments[..n], &wanted));
        if hit {
            result = !negated;
        }
    }
    result
}

fn matches_segments(path: &[&str], pattern: &[&str]) -> bool {
    match (pattern.first(), path.first()) {
        (None, None) => true,
        (Some(&"**"), _) => {
            matches_segments(path, &pattern[1..])
                || (!path.is_empty() && matches_segments(&path[1..], pattern))
        }
        (Some(p), Some(s)) => segment_matches(s, p) && matches_segments(&path[1..], &pattern[1..]),
        _ => false,
    }
}

/// `*` (any run, not `/`), `?` (one character) and `[...]` are not special
/// beyond `*` and `?` here.
fn segment_matches(text: &str, pattern: &str) -> bool {
    fn go(t: &[char], p: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => go(t, &p[1..]) || (!t.is_empty() && go(&t[1..], p)),
            Some('?') => !t.is_empty() && go(&t[1..], &p[1..]),
            Some(c) => t.first() == Some(c) && go(&t[1..], &p[1..]),
        }
    }
    let (t, p): (Vec<char>, Vec<char>) = (text.chars().collect(), pattern.chars().collect());
    go(&t, &p)
}

/// The context under `dir` as a tar, and its SHA-256 as hex.
pub fn pack(dir: &Path) -> Result<(Vec<u8>, String)> {
    let patterns = match std::fs::read_to_string(dir.join(".dockerignore")) {
        Ok(text) => ignore_patterns(&text),
        Err(_) => Vec::new(),
    };
    let mut files = Vec::new();
    walk(dir, dir, &patterns, &mut files)?;
    files.sort();
    let mut builder = tar::Builder::new(Vec::new());
    let mut total = 0u64;
    for relative in &files {
        let path = dir.join(relative);
        let meta =
            std::fs::symlink_metadata(&path).with_context(|| format!("{}", path.display()))?;
        let name = relative.to_string_lossy().replace('\\', "/");
        let mut header = tar::Header::new_gnu();
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        if meta.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder.append_data(&mut header, format!("{name}/"), std::io::empty())?;
        } else if meta.is_file() {
            total += meta.len();
            if total > MOST_BYTES {
                bail!(
                    "the build context is over {} MiB; add a .dockerignore",
                    MOST_BYTES >> 20
                );
            }
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(mode_of(&meta));
            header.set_size(meta.len());
            let file = std::fs::File::open(&path).with_context(|| format!("{}", path.display()))?;
            builder.append_data(&mut header, &name, file)?;
        }
        // Links and anything else are left out: a link out of the context
        // would not mean the same in the guest.
    }
    let bytes = builder.into_inner()?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((bytes, hash))
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

/// Windows keeps no execute bit; `COPY --chmod` sets one.
#[cfg(not(unix))]
fn mode_of(_: &std::fs::Metadata) -> u32 {
    0o644
}

fn walk(root: &Path, dir: &Path, patterns: &[String], out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("{}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?.to_path_buf();
        let text = relative.to_string_lossy().replace('\\', "/");
        if ignored(&text, patterns) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            out.push(relative);
            walk(root, &path, patterns, out)?;
        } else if kind.is_file() {
            out.push(relative);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(text: &str) -> Vec<String> {
        ignore_patterns(text)
    }

    #[test]
    fn dockerignore_patterns_follow_the_last_match() {
        let p = patterns("# comment\n\nnode_modules\n*.log\n**/secret.txt\n!keep.log\ntmp/*\n");
        for (path, want) in [
            ("node_modules", true),
            ("node_modules/a/b.js", true),
            ("src/node_modules/x", false),
            ("a.log", true),
            ("keep.log", false),
            ("src/a.log", false),
            ("secret.txt", true),
            ("deep/er/secret.txt", true),
            ("tmp/x", true),
            ("tmp", false),
            ("src/main.rs", false),
        ] {
            assert_eq!(ignored(path, &p), want, "{path}");
        }
    }

    #[test]
    fn a_context_packs_the_same_bytes_each_time_and_honours_dockerignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/app.py"), "print('hi')\n").unwrap();
        std::fs::write(root.join("Dockerfile"), "FROM a\n").unwrap();
        std::fs::write(root.join("debug.log"), "noise").unwrap();
        std::fs::write(root.join(".dockerignore"), "*.log\n").unwrap();

        let (first, hash) = pack(root).unwrap();
        let (second, again) = pack(root).unwrap();
        assert_eq!((&first, &hash), (&second, &again));
        assert_eq!(hash.len(), 64);

        let mut names = Vec::new();
        for entry in tar::Archive::new(&first[..]).entries().unwrap() {
            names.push(
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        assert!(names.contains(&"src/app.py".to_string()), "{names:?}");
        assert!(names.contains(&"Dockerfile".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.ends_with(".log")), "{names:?}");

        std::fs::write(root.join("src/app.py"), "print('changed')\n").unwrap();
        assert_ne!(
            pack(root).unwrap().1,
            hash,
            "a changed file changes the hash"
        );
    }
}
