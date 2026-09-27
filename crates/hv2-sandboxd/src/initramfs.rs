//! Writing a template's initramfs from a pulled image and the guest kit --
//! what `tools/guest-image/build.sh --rootfs` does, without `cpio` or a
//! scratch directory, for a node that builds templates while it runs.
//!
//! `newc` cpio, gzipped: every entry owned by root, every mtime zero, paths
//! in sorted order -- the same image from the same inputs, byte for byte.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::oci::{Entry, Image};

/// What every template's guest needs beside the image: the agent, busybox,
/// init, and optionally a CA bundle -- the files `build.sh` installs.
#[derive(Debug, Clone)]
pub struct GuestKit {
    pub dir: PathBuf,
}

impl GuestKit {
    fn file(&self, name: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.dir.join(name))
            .map_err(|e| format!("the guest kit's {name} ({}): {e}", self.dir.display()))
    }

    /// Check the kit has what it must before anything is pulled.
    pub fn check(dir: &Path) -> Result<Self, String> {
        let kit = Self {
            dir: dir.to_path_buf(),
        };
        for name in ["hv2-guest-agentd", "busybox", "init"] {
            kit.file(name)?;
        }
        Ok(kit)
    }
}

const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFLNK: u32 = 0o120_000;

/// The image's filesystem with the kit on top, as a gzipped `newc` cpio.
pub fn build(image: &Image, kit: &GuestKit) -> Result<Vec<u8>, String> {
    let mut entries = image.entries.clone();
    let exec = |data| Entry::File { mode: 0o755, data };
    for dir in ["bin", "sbin", "dev", "proc", "sys", "tmp", "etc", "root"] {
        entries
            .entry(dir.to_string())
            .or_insert(Entry::Dir { mode: 0o755 });
    }
    // Where the image's /bin is a symlink (merged /usr), the kit goes where
    // it points, as `cp` into the directory would put it.
    let bin = match entries.get("bin") {
        Some(Entry::Symlink { target }) => target.trim_start_matches('/').to_string(),
        _ => "bin".to_string(),
    };
    // The image's own busybox, if it has one (alpine, busybox), is its to
    // keep: it runs against the image's libc, and replacing it would change
    // every applet the image links to it. The kit's static one fills the gap
    // where there is none, which init needs.
    if let std::collections::btree_map::Entry::Vacant(slot) =
        entries.entry(format!("{bin}/busybox"))
    {
        slot.insert(exec(kit.file("busybox")?));
    }
    entries.insert(
        format!("{bin}/hv2-guest-agentd"),
        exec(kit.file("hv2-guest-agentd")?),
    );
    entries.insert("init".to_string(), exec(kit.file("init")?));
    let sh = format!("{bin}/sh");
    entries.entry(sh).or_insert(Entry::Symlink {
        target: "busybox".into(),
    });
    if let Ok(shim) = kit.file("bash-shim") {
        entries.entry(format!("{bin}/bash")).or_insert(exec(shim));
    }
    if let Ok(bundle) = kit.file("ca-certificates.crt") {
        entries.insert(
            "etc/ssl/certs/ca-certificates.crt".into(),
            Entry::File {
                mode: 0o644,
                data: bundle,
            },
        );
    }
    // The image's environment, for init to load before the agent starts.
    let env: String = image
        .env
        .iter()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| format!("export {key}='{}'\n", value.replace('\'', "'\\''")))
        .collect();
    entries.insert(
        "etc/hv2-env".into(),
        Entry::File {
            mode: 0o644,
            data: env.into_bytes(),
        },
    );
    // Docker's per-container files, which the gateway and kernel supply.
    for gone in ["etc/resolv.conf", "etc/hostname", "etc/hosts", ".dockerenv"] {
        entries.remove(gone);
    }
    // Every parent a directory, since the kernel's unpacker makes none.
    let parents: Vec<String> = entries
        .keys()
        .flat_map(|path| {
            let mut dirs = Vec::new();
            let mut at = path.as_str();
            while let Some((dir, _)) = at.rsplit_once('/') {
                dirs.push(dir.to_string());
                at = dir;
            }
            dirs
        })
        .collect();
    for dir in parents {
        entries.entry(dir).or_insert(Entry::Dir { mode: 0o755 });
    }
    write(&entries)
}

/// The file a hard link finally names, following links to links; `None`
/// if it names no regular file (a target whited out, or never there).
fn link_root<'a>(entries: &'a BTreeMap<String, Entry>, mut target: &'a str) -> Option<&'a str> {
    for _ in 0..16 {
        match entries.get(target)? {
            Entry::File { .. } => return Some(target),
            Entry::Link { target: next } => target = next,
            _ => return None,
        }
    }
    None
}

fn write(entries: &BTreeMap<String, Entry>) -> Result<Vec<u8>, String> {
    // Hard links, grouped by the file they share. In an initramfs the
    // kernel keeps the data of the *first* entry of an inode it meets and
    // links every later one to it, discarding their data -- so the group's
    // first path in archive order carries the bytes, and every member the
    // same inode number and link count.
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, entry) in entries {
        let root = match entry {
            Entry::Link { target } => link_root(entries, target),
            Entry::File { .. } => Some(path.as_str()),
            _ => None,
        };
        if let Some(root) = root {
            groups.entry(root).or_default().push(path);
        }
    }
    let mut group_of: BTreeMap<&str, (&str, u32)> = BTreeMap::new();
    for (root, members) in &groups {
        for member in members {
            group_of.insert(member, (root, members.len() as u32));
        }
    }
    let mut ino_of: BTreeMap<&str, u32> = BTreeMap::new();
    let mut next_ino = 1u32;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    for (path, entry) in entries {
        let mut nlink = 1;
        let mut ino = next_ino;
        let (mode, data): (u32, &[u8]) = match entry {
            Entry::Dir { mode } => (S_IFDIR | mode, &[]),
            Entry::Symlink { target } => (S_IFLNK | 0o777, target.as_bytes()),
            Entry::File { .. } | Entry::Link { .. } => {
                let Some(&(root, count)) = group_of.get(path.as_str()) else {
                    continue; // a link to nothing
                };
                let Some(Entry::File { mode, data }) = entries.get(root) else {
                    continue;
                };
                nlink = count;
                match ino_of.get(root) {
                    // A later member: the inode, no data.
                    Some(&shared) => {
                        ino = shared;
                        (S_IFREG | mode, &[][..])
                    }
                    None => {
                        ino_of.insert(root, ino);
                        (S_IFREG | mode, &data[..])
                    }
                }
            }
        };
        if ino == next_ino {
            next_ino += 1;
        }
        record(&mut gz, ino, mode, nlink, path, data).map_err(|e| e.to_string())?;
    }
    record(&mut gz, 0, 0, 1, "TRAILER!!!", &[]).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

/// One `newc` record: the 110-byte header, the NUL-terminated name padded
/// to four bytes, the data padded to four.
fn record(
    out: &mut impl Write,
    ino: u32,
    mode: u32,
    nlink: u32,
    name: &str,
    data: &[u8],
) -> std::io::Result<()> {
    let nlink = if mode & 0o170_000 == S_IFDIR {
        2
    } else {
        nlink
    };
    let name_len = name.len() + 1;
    let header = format!(
        "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{size:08x}\
         {dmaj:08x}{dmin:08x}{rmaj:08x}{rmin:08x}{name_len:08x}{check:08x}",
        uid = 0,
        gid = 0,
        mtime = 0,
        size = data.len(),
        dmaj = 0,
        dmin = 0,
        rmaj = 0,
        rmin = 0,
        check = 0,
    );
    out.write_all(header.as_bytes())?;
    out.write_all(name.as_bytes())?;
    out.write_all(&[0])?;
    out.write_all(&vec![0; (4 - (110 + name_len) % 4) % 4])?;
    out.write_all(data)?;
    out.write_all(&vec![0; (4 - data.len() % 4) % 4])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A written archive reads back with `cpio`'s own format rules: each
    /// header where the last record's padding ends, names and sizes intact.
    #[test]
    fn records_are_newc_and_aligned() {
        let mut entries = BTreeMap::new();
        entries.insert("bin".to_string(), Entry::Dir { mode: 0o755 });
        entries.insert(
            "bin/tool".to_string(),
            Entry::File {
                mode: 0o755,
                data: b"abcde".to_vec(),
            },
        );
        entries.insert(
            "bin/sh".to_string(),
            Entry::Symlink {
                target: "busybox".into(),
            },
        );
        let gz = write(&entries).unwrap();
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&gz[..]), &mut raw).unwrap();

        let mut at = 0;
        let mut seen = Vec::new();
        loop {
            assert_eq!(&raw[at..at + 6], b"070701", "a header at {at}");
            let field = |i: usize| {
                usize::from_str_radix(
                    std::str::from_utf8(&raw[at + 6 + 8 * i..at + 14 + 8 * i]).unwrap(),
                    16,
                )
                .unwrap()
            };
            let (mode, size, name_len) = (field(1), field(6), field(11));
            let name = std::str::from_utf8(&raw[at + 110..at + 110 + name_len - 1])
                .unwrap()
                .to_string();
            let data_at = (at + 110 + name_len + 3) & !3;
            let data = &raw[data_at..data_at + size];
            at = (data_at + size + 3) & !3;
            if name == "TRAILER!!!" {
                break;
            }
            seen.push((name, mode as u32 & 0o170_000, data.to_vec()));
        }
        assert_eq!(
            seen,
            vec![
                ("bin".to_string(), S_IFDIR, vec![]),
                ("bin/sh".to_string(), S_IFLNK, b"busybox".to_vec()),
                ("bin/tool".to_string(), S_IFREG, b"abcde".to_vec()),
            ]
        );
    }

    /// Hard links become one inode: data once, on the first in archive
    /// order, the same inode number and link count on every member.
    #[test]
    fn hard_links_share_one_inode_and_carry_data_once() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "bin/busybox".to_string(),
            Entry::File {
                mode: 0o755,
                data: vec![7; 1000],
            },
        );
        for applet in ["bin/ash", "bin/cat", "sbin/init"] {
            entries.insert(
                applet.to_string(),
                Entry::Link {
                    target: "bin/busybox".into(),
                },
            );
        }
        entries.insert(
            "bin/dangling".to_string(),
            Entry::Link {
                target: "bin/gone".into(),
            },
        );
        let gz = write(&entries).unwrap();
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&gz[..]), &mut raw).unwrap();

        let mut at = 0;
        let mut seen = Vec::new();
        loop {
            let field = |i: usize| {
                usize::from_str_radix(
                    std::str::from_utf8(&raw[at + 6 + 8 * i..at + 14 + 8 * i]).unwrap(),
                    16,
                )
                .unwrap()
            };
            let (ino, nlink, size, name_len) = (field(0), field(4), field(6), field(11));
            let name = std::str::from_utf8(&raw[at + 110..at + 110 + name_len - 1])
                .unwrap()
                .to_string();
            let data_at = (at + 110 + name_len + 3) & !3;
            at = (data_at + size + 3) & !3;
            if name == "TRAILER!!!" {
                break;
            }
            seen.push((name, ino, nlink, size));
        }
        let inos: std::collections::BTreeSet<usize> = seen.iter().map(|s| s.1).collect();
        assert_eq!(inos.len(), 1, "one inode: {seen:?}");
        assert!(seen.iter().all(|s| s.2 == 4), "nlink 4 on each: {seen:?}");
        assert_eq!(seen[0].0, "bin/ash", "archive order");
        assert_eq!(seen[0].3, 1000, "the first carries the data");
        assert!(
            seen[1..].iter().all(|s| s.3 == 0),
            "the rest carry none: {seen:?}"
        );
        assert!(
            !seen.iter().any(|s| s.0 == "bin/dangling"),
            "a link to nothing is dropped"
        );
    }
}
