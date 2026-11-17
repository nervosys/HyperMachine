//! A 9P2000.L server for volumes: a host directory, served to a guest over
//! one vsock connection, which the guest's kernel mounts (`trans=fd`) as a
//! filesystem. Every sandbox mounting a volume sees the same files, live;
//! they outlive the sandbox.
//!
//! The guest is the adversary here. Every path is resolved with `openat2`
//! beneath the volume's root with no symbolic links followed
//! (`RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`), and every operation that
//! names an entry takes one validated name relative to a directory already
//! resolved that way -- so nothing a guest can create, rename or race makes
//! the server touch a host path outside the volume. Attributes are changed
//! through the resolved inode (`/proc/self/fd`), never by a path that could
//! be swapped for a symlink in between.
//!
//! A guest shares the node, and its other guests, with this server, so what it
//! may hold is bounded: at most [`MAX_FIDS`] handles and [`MAX_OPEN_FILES`] host
//! files open per mount, and [`MAX_OPEN_FILES_TOTAL`] over all of them (an open
//! file is a host descriptor, and the node's are everyone's); a directory is
//! listed a screenful at a time, not read whole. No file or directory made
//! here keeps a setuid or setgid bit: the guest's own root may set them in
//! its own filesystem, but a volume is shared with the host and with other
//! sandboxes, where a setuid file somebody else runs is theirs to fear.
//!
//! Ownership is the guest's, not the host's: the server runs as whatever
//! user the node does, so a file's owner in the guest is kept in an extended
//! attribute (`user.hv2.owner`) set on create and `chown`. A file without
//! one belongs to whoever looks -- one written by the host, or copied in.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::FileExt;
use std::time::Duration;

use hv2_agent::GuestChannel;

pub mod quota;
use quota::{Budget, Cost};

/// The attribute holding a file's owner in the guest, `"uid:gid"`.
const OWNER_XATTR: &str = "user.hv2.owner";

/// The largest message this server negotiates: big enough that a read or
/// write moves a useful amount per round trip.
const MAX_MSIZE: u32 = 512 * 1024;

/// `NOFID`: a fid slot left empty.
const NOFID: u32 = u32::MAX;

/// The most fids (handles on names) one mount may hold; a new one past it is
/// refused with `EMFILE`.
pub const MAX_FIDS: usize = 4096;

/// The most host files one mount may hold open at once.
pub const MAX_OPEN_FILES: usize = 2048;

/// The most host files all mounts together may hold open: a share of the
/// node's descriptors that no guest can spend whole.
pub const MAX_OPEN_FILES_TOTAL: usize = 16384;

/// The files held open by every mount on this node.
static OPEN_FILES_TOTAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The mode bits a volume keeps: permissions and the sticky bit, never
/// setuid or setgid.
pub const KEPT_MODE_BITS: u32 = 0o1777;

/// `mode` as a volume stores it: [`KEPT_MODE_BITS`] of it.
pub fn safe_mode(mode: u32) -> u32 {
    mode & KEPT_MODE_BITS
}

/// A host file a guest holds open, counted against its mount's share and the
/// node's until it is closed.
struct OpenFile {
    file: File,
    mount: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl OpenFile {
    /// Count one more open file for the mount, or `EMFILE` / `ENFILE`: take
    /// the share before the file is opened, so a refusal makes nothing.
    fn reserve(
        mount: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        mount_limit: usize,
        total_limit: usize,
    ) -> std::io::Result<Reservation> {
        use std::sync::atomic::Ordering::SeqCst;
        if mount.fetch_add(1, SeqCst) >= mount_limit {
            mount.fetch_sub(1, SeqCst);
            return Err(Error::from_raw_os_error(libc::EMFILE));
        }
        if OPEN_FILES_TOTAL.fetch_add(1, SeqCst) >= total_limit {
            OPEN_FILES_TOTAL.fetch_sub(1, SeqCst);
            mount.fetch_sub(1, SeqCst);
            return Err(Error::from_raw_os_error(libc::ENFILE));
        }
        Ok(Reservation(Some(std::sync::Arc::clone(mount))))
    }
}

/// A counted share of open files, not yet a file: dropped, it is given back.
struct Reservation(Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>);

impl Reservation {
    fn file(mut self, file: File) -> OpenFile {
        OpenFile {
            file,
            mount: self.0.take().expect("a reservation holds its mount"),
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(mount) = self.0.take() {
            mount.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            OPEN_FILES_TOTAL.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        self.mount.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        OPEN_FILES_TOTAL.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl std::ops::Deref for OpenFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}

/// A directory, and everything beneath it -- nothing else.
pub struct Beneath {
    root: OwnedFd,
    path: std::path::PathBuf,
}

/// What a path names: its host fd (`O_PATH`, the entry itself even when a
/// symlink) and its `stat`.
pub struct Entry {
    pub fd: OwnedFd,
    pub stat: libc::stat,
}

fn last_error() -> Error {
    Error::last_os_error()
}

fn cstring(s: &str) -> std::io::Result<CString> {
    CString::new(s).map_err(|_| Error::from_raw_os_error(libc::EINVAL))
}

/// One name within a directory: not empty, not `.` or `..`, no `/`, no NUL.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\0'])
        && name.len() <= 255
}

/// A path relative to a volume's root, as its components; `..` and `.` are
/// resolved logically and may not climb out.
pub fn components(path: &str) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.pop().is_none() {
                    return Err(Error::from_raw_os_error(libc::EACCES));
                }
            }
            name if valid_name(name) => out.push(name.to_string()),
            _ => return Err(Error::from_raw_os_error(libc::EINVAL)),
        }
    }
    Ok(out)
}

impl Beneath {
    /// Serve `dir`, which must exist.
    pub fn open(dir: &std::path::Path) -> std::io::Result<Self> {
        let path = cstring(&dir.to_string_lossy())?;
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(last_error());
        }
        Ok(Self {
            root: unsafe { OwnedFd::from_raw_fd(fd) },
            path: dir.to_path_buf(),
        })
    }

    /// Where the volume is on the host.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Open `parts` beneath the root with `flags` (`O_NOFOLLOW` added): no
    /// symlink anywhere on the way, and nowhere outside.
    pub fn open_at(&self, parts: &[String], flags: i32, mode: u32) -> std::io::Result<OwnedFd> {
        let rel = if parts.is_empty() {
            ".".to_string()
        } else {
            parts.join("/")
        };
        let path = cstring(&rel)?;
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (flags | libc::O_NOFOLLOW | libc::O_CLOEXEC) as u64;
        how.mode = if flags & libc::O_CREAT != 0 {
            u64::from(safe_mode(mode))
        } else {
            0
        };
        how.resolve =
            libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                self.root.as_raw_fd(),
                path.as_ptr(),
                &how as *const libc::open_how,
                std::mem::size_of::<libc::open_how>(),
            )
        };
        if fd < 0 {
            let e = last_error();
            // Following a symlink is what was refused: to the guest, the
            // path is not a directory it may walk through.
            return Err(
                if e.raw_os_error() == Some(libc::ELOOP) && flags & libc::O_PATH == 0 {
                    Error::from_raw_os_error(libc::ELOOP)
                } else {
                    e
                },
            );
        }
        Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
    }

    /// The entry at `parts`, itself (a symlink is not followed).
    pub fn entry(&self, parts: &[String]) -> std::io::Result<Entry> {
        let fd = self.open_at(parts, libc::O_PATH, 0)?;
        let stat = fstat(fd.as_raw_fd())?;
        Ok(Entry { fd, stat })
    }

    /// The directory at `parts`, for `*at` calls on one name within it.
    pub fn dir(&self, parts: &[String]) -> std::io::Result<OwnedFd> {
        self.open_at(parts, libc::O_PATH | libc::O_DIRECTORY, 0)
    }
}

pub fn fstat(fd: RawFd) -> std::io::Result<libc::stat> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let empty = c"";
    if unsafe {
        libc::fstatat(
            fd,
            empty.as_ptr(),
            &mut st,
            libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(last_error());
    }
    Ok(st)
}

/// The resolved inode behind `fd`, as a path the kernel will not re-resolve
/// through anything the guest controls.
pub fn proc_path(fd: RawFd) -> CString {
    CString::new(format!("/proc/self/fd/{fd}")).expect("no NUL")
}

pub fn is_dir(st: &libc::stat) -> bool {
    st.st_mode & libc::S_IFMT == libc::S_IFDIR
}

pub fn is_symlink(st: &libc::stat) -> bool {
    st.st_mode & libc::S_IFMT == libc::S_IFLNK
}

/// The owner a guest sees: the recorded one, else `default`.
pub fn owner(fd: RawFd, st: &libc::stat, default: (u32, u32)) -> (u32, u32) {
    if is_symlink(st) {
        return default;
    }
    let path = proc_path(fd);
    let name = CString::new(OWNER_XATTR).expect("no NUL");
    let mut buf = [0u8; 32];
    let n = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if n <= 0 {
        return default;
    }
    std::str::from_utf8(&buf[..n as usize])
        .ok()
        .and_then(|s| s.split_once(':'))
        .and_then(|(u, g)| Some((u.parse().ok()?, g.parse().ok()?)))
        .unwrap_or(default)
}

/// Record the guest owner of the inode behind `fd`.
pub fn set_owner(fd: RawFd, uid: u32, gid: u32) -> std::io::Result<()> {
    let path = proc_path(fd);
    let name = CString::new(OWNER_XATTR).expect("no NUL");
    let value = format!("{uid}:{gid}");
    if unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    } < 0
    {
        return Err(last_error());
    }
    Ok(())
}

pub fn chmod(fd: RawFd, mode: u32) -> std::io::Result<()> {
    let path = proc_path(fd);
    if unsafe { libc::chmod(path.as_ptr(), safe_mode(mode)) } < 0 {
        return Err(last_error());
    }
    Ok(())
}

/// A message being built: little-endian fields, as 9P has them.
struct Out(Vec<u8>);

impl Out {
    fn new(kind: u8, tag: u16) -> Self {
        let mut v = vec![0; 4];
        v.push(kind);
        v.extend_from_slice(&tag.to_le_bytes());
        Self(v)
    }
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(&mut self, s: &str) -> &mut Self {
        let bytes = s.as_bytes();
        self.u16(u16::try_from(bytes.len()).unwrap_or(u16::MAX));
        self.0
            .extend_from_slice(&bytes[..bytes.len().min(usize::from(u16::MAX))]);
        self
    }
    fn qid(&mut self, q: Qid) -> &mut Self {
        self.u8(q.kind).u32(q.version).u64(q.path)
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    fn finish(mut self) -> Vec<u8> {
        let len = u32::try_from(self.0.len()).unwrap_or(u32::MAX);
        self.0[..4].copy_from_slice(&len.to_le_bytes());
        self.0
    }
}

/// A message being read.
struct In<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> std::io::Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len());
        let end = end.ok_or_else(|| Error::from_raw_os_error(libc::EPROTO))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> std::io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> std::io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2")))
    }
    fn u32(&mut self) -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> std::io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn str(&mut self) -> std::io::Result<String> {
        let n = usize::from(self.u16()?);
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| Error::from_raw_os_error(libc::EINVAL))
    }
}

#[derive(Clone, Copy)]
struct Qid {
    kind: u8,
    version: u32,
    path: u64,
}

fn qid(st: &libc::stat) -> Qid {
    let kind = if is_dir(st) {
        0x80
    } else if is_symlink(st) {
        0x02
    } else {
        0
    };
    Qid {
        kind,
        version: 0,
        path: st.st_ino,
    }
}

/// One of the guest's handles on a file.
struct Fid {
    parts: Vec<String>,
    /// The guest user it acts for, from its `Tattach`.
    uid: u32,
    file: Option<OpenFile>,
    /// A directory being listed, read as far as the guest has asked.
    listing: Option<Listing>,
}

/// One directory entry as `Treaddir` gives it: its qid, its type, its name.
type DirEntry = (Qid, u8, String);

/// A directory read a screenful at a time. The guest's offsets are the index
/// after the last entry it was given (`.` is 0, `..` 1, the rest from 2), so a
/// listing that goes on from where it stopped is one more read; only a guest
/// that asks for somewhere else costs a second pass over the directory.
struct Listing {
    entries: std::fs::ReadDir,
    /// The index of the next entry to give.
    next: u64,
    /// An entry read and not given, because it did not fit.
    held: Option<DirEntry>,
}

/// A 9P2000.L server for one mount of one volume.
pub struct Server {
    fs: Beneath,
    fids: HashMap<u32, Fid>,
    msize: u32,
    /// How many host files this mount holds open.
    open_files: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    max_fids: usize,
    max_open_files: usize,
    /// What the volume may still grow by, shared with its other mounts.
    budget: std::sync::Arc<Budget>,
}

const RLERROR: u8 = 7;

impl Server {
    pub fn new(fs: Beneath) -> Self {
        let budget = quota::budget_for(fs.path());
        Self::with_budget(fs, budget)
    }

    /// A server whose volume spends `budget`.
    pub fn with_budget(fs: Beneath, budget: std::sync::Arc<Budget>) -> Self {
        Self {
            fs,
            budget,
            fids: HashMap::new(),
            msize: 8192,
            open_files: std::sync::Arc::default(),
            max_fids: MAX_FIDS,
            max_open_files: MAX_OPEN_FILES,
        }
    }

    /// Make room for fid `id`: a new one past the limit is refused, `EMFILE`;
    /// naming one already held (to replace it) needs none.
    fn room_for(&self, id: u32) -> std::io::Result<()> {
        if self.fids.contains_key(&id) || self.fids.len() < self.max_fids {
            Ok(())
        } else {
            Err(Error::from_raw_os_error(libc::EMFILE))
        }
    }

    /// A share of this mount's open files and the node's, before opening one.
    fn reserve_file(&self) -> std::io::Result<Reservation> {
        OpenFile::reserve(&self.open_files, self.max_open_files, MAX_OPEN_FILES_TOTAL)
    }

    /// Answer one whole message (size prefix included).
    pub fn handle(&mut self, msg: &[u8]) -> Vec<u8> {
        let (kind, tag) = match (msg.get(4), msg.get(5..7)) {
            (Some(&k), Some(t)) => (k, u16::from_le_bytes([t[0], t[1]])),
            _ => return Vec::new(),
        };
        let mut body = In { b: msg, at: 7 };
        match self.dispatch(kind, tag, &mut body) {
            Ok(reply) => reply,
            Err(e) => {
                let code = e.raw_os_error().unwrap_or(libc::EIO);
                let mut out = Out::new(RLERROR, tag);
                out.u32(u32::try_from(code).unwrap_or(5));
                out.finish()
            }
        }
    }

    fn fid(&self, id: u32) -> std::io::Result<&Fid> {
        self.fids
            .get(&id)
            .ok_or_else(|| Error::from_raw_os_error(libc::EBADF))
    }

    fn fid_mut(&mut self, id: u32) -> std::io::Result<&mut Fid> {
        self.fids
            .get_mut(&id)
            .ok_or_else(|| Error::from_raw_os_error(libc::EBADF))
    }

    fn child(&self, dir: u32, name: &str) -> std::io::Result<(Vec<String>, u32)> {
        if !valid_name(name) {
            return Err(Error::from_raw_os_error(libc::EINVAL));
        }
        let fid = self.fid(dir)?;
        let mut parts = fid.parts.clone();
        parts.push(name.to_string());
        Ok((parts, fid.uid))
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch(&mut self, kind: u8, tag: u16, m: &mut In<'_>) -> std::io::Result<Vec<u8>> {
        let reply = |k: u8| Out::new(k, tag);
        match kind {
            // Tversion
            100 => {
                let msize = m.u32()?;
                let version = m.str()?;
                self.fids.clear();
                self.msize = msize.clamp(4096, MAX_MSIZE);
                let mut out = reply(101);
                let answer = if version.starts_with("9P2000.L") {
                    "9P2000.L"
                } else {
                    "unknown"
                };
                out.u32(self.msize).str(answer);
                Ok(out.finish())
            }
            // Tauth: none is needed -- the connection is this sandbox's.
            102 => Err(Error::from_raw_os_error(libc::EOPNOTSUPP)),
            // Tattach
            104 => {
                let fid = m.u32()?;
                let _afid = m.u32()?;
                let _uname = m.str()?;
                let _aname = m.str()?;
                let uid = m.u32().unwrap_or(0);
                self.room_for(fid)?;
                let entry = self.fs.entry(&[])?;
                self.fids.insert(
                    fid,
                    Fid {
                        parts: Vec::new(),
                        uid: if uid == NOFID { 0 } else { uid },
                        file: None,
                        listing: None,
                    },
                );
                let mut out = reply(105);
                out.qid(qid(&entry.stat));
                Ok(out.finish())
            }
            // Tflush: every request is answered before the next is read.
            108 => Ok(reply(109).finish()),
            // Twalk
            110 => {
                let fid = m.u32()?;
                let newfid = m.u32()?;
                let n = m.u16()?;
                self.room_for(newfid)?;
                let (mut parts, uid) = {
                    let f = self.fid(fid)?;
                    (f.parts.clone(), f.uid)
                };
                let mut qids = Vec::new();
                for i in 0..n {
                    let name = m.str()?;
                    if name == ".." {
                        parts.pop();
                    } else if valid_name(&name) {
                        parts.push(name);
                    } else {
                        return Err(Error::from_raw_os_error(libc::EINVAL));
                    }
                    match self.fs.entry(&parts) {
                        Ok(e) => qids.push(qid(&e.stat)),
                        Err(e) if i == 0 => return Err(e),
                        Err(_) => break,
                    }
                }
                if qids.len() == usize::from(n) {
                    self.fids.insert(
                        newfid,
                        Fid {
                            parts,
                            uid,
                            file: None,
                            listing: None,
                        },
                    );
                }
                let mut out = reply(111);
                out.u16(u16::try_from(qids.len()).unwrap_or(0));
                for q in qids {
                    out.qid(q);
                }
                Ok(out.finish())
            }
            // Tlopen
            12 => {
                let fid = m.u32()?;
                let flags = m.u32()? as i32;
                let parts = self.fid(fid)?.parts.clone();
                let entry = self.fs.entry(&parts)?;
                let open_flags = if is_dir(&entry.stat) {
                    libc::O_RDONLY | libc::O_DIRECTORY
                } else {
                    flags & (libc::O_ACCMODE | libc::O_TRUNC | libc::O_APPEND)
                };
                let share = self.reserve_file()?;
                let fd = self.fs.open_at(&parts, open_flags, 0)?;
                if open_flags & libc::O_TRUNC != 0 {
                    self.truncated(fd.as_raw_fd(), entry.stat.st_size);
                }
                let f = self.fid_mut(fid)?;
                f.file = Some(share.file(File::from(fd)));
                f.listing = None;
                let mut out = reply(13);
                out.qid(qid(&entry.stat)).u32(0);
                Ok(out.finish())
            }
            // Tlcreate
            14 => {
                let fid = m.u32()?;
                let name = m.str()?;
                let flags = m.u32()? as i32;
                let mode = m.u32()?;
                let gid = m.u32()?;
                let (parts, uid) = self.child(fid, &name)?;
                let flags = (flags
                    & (libc::O_ACCMODE | libc::O_TRUNC | libc::O_APPEND | libc::O_EXCL))
                    | libc::O_CREAT;
                // Before the file is made: a refusal leaves nothing behind.
                let share = self.reserve_file()?;
                // A name already there is opened (and perhaps emptied), not
                // made: only a new entry is charged, and the charge is taken
                // before it exists.
                let before = self.fs.entry(&parts).ok().map(|e| e.stat);
                let charge = Cost {
                    inodes: u64::from(before.is_none()),
                    ..Cost::default()
                };
                self.budget.admit(charge)?;
                let fd = match self.fs.open_at(&parts, flags, safe_mode(mode)) {
                    Ok(fd) => fd,
                    Err(e) => {
                        self.budget.release(charge);
                        return Err(e);
                    }
                };
                if let Some(before) = &before {
                    if flags & libc::O_TRUNC != 0 {
                        self.truncated(fd.as_raw_fd(), before.st_size);
                    }
                }
                let _ = set_owner(fd.as_raw_fd(), uid, gid);
                let st = fstat(fd.as_raw_fd())?;
                let f = self.fid_mut(fid)?;
                f.parts = parts;
                f.file = Some(share.file(File::from(fd)));
                f.listing = None;
                let mut out = reply(15);
                out.qid(qid(&st)).u32(0);
                Ok(out.finish())
            }
            // Tsymlink
            16 => {
                let fid = m.u32()?;
                let name = m.str()?;
                let target = m.str()?;
                let _gid = m.u32()?;
                let (parts, _) = self.child(fid, &name)?;
                let dir = self.fs.dir(&parts[..parts.len() - 1])?;
                let (t, n) = (cstring(&target)?, cstring(&name)?);
                let charge = Cost {
                    size: target.len() as u64,
                    disk: target.len() as u64,
                    inodes: 1,
                };
                self.budget.admit(charge)?;
                if unsafe { libc::symlinkat(t.as_ptr(), dir.as_raw_fd(), n.as_ptr()) } < 0 {
                    self.budget.release(charge);
                    return Err(last_error());
                }
                let st = self.fs.entry(&parts)?.stat;
                let mut out = reply(17);
                out.qid(qid(&st));
                Ok(out.finish())
            }
            // Tmknod: devices and FIFOs are not volume contents.
            18 => Err(Error::from_raw_os_error(libc::EPERM)),
            // Trename
            20 => {
                let fid = m.u32()?;
                let dfid = m.u32()?;
                let name = m.str()?;
                let from = self.fid(fid)?.parts.clone();
                if from.is_empty() {
                    return Err(Error::from_raw_os_error(libc::EBUSY));
                }
                let (to, _) = self.child(dfid, &name)?;
                self.rename(&from, &to)?;
                self.fid_mut(fid)?.parts = to;
                Ok(reply(21).finish())
            }
            // Treadlink
            22 => {
                let fid = m.u32()?;
                let parts = self.fid(fid)?.parts.clone();
                let Some((name, dir_parts)) = parts.split_last() else {
                    return Err(Error::from_raw_os_error(libc::EINVAL));
                };
                let dir = self.fs.dir(dir_parts)?;
                let n = cstring(name)?;
                let mut buf = vec![0u8; 4096];
                let len = unsafe {
                    libc::readlinkat(
                        dir.as_raw_fd(),
                        n.as_ptr(),
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                    )
                };
                if len < 0 {
                    return Err(last_error());
                }
                buf.truncate(len as usize);
                let mut out = reply(23);
                out.str(&String::from_utf8_lossy(&buf));
                Ok(out.finish())
            }
            // Tgetattr
            24 => {
                let fid = m.u32()?;
                let _mask = m.u64()?;
                let (parts, uid) = {
                    let f = self.fid(fid)?;
                    (f.parts.clone(), f.uid)
                };
                let entry = self.fs.entry(&parts)?;
                let st = &entry.stat;
                let (ouid, ogid) = owner(entry.fd.as_raw_fd(), st, (uid, uid));
                let mut out = reply(25);
                out.u64(0x0000_07FF) // P9_GETATTR_BASIC
                    .qid(qid(st))
                    .u32(st.st_mode)
                    .u32(ouid)
                    .u32(ogid)
                    .u64(st.st_nlink)
                    .u64(st.st_rdev)
                    .u64(st.st_size as u64)
                    .u64(st.st_blksize as u64)
                    .u64(st.st_blocks as u64)
                    .u64(st.st_atime as u64)
                    .u64(st.st_atime_nsec as u64)
                    .u64(st.st_mtime as u64)
                    .u64(st.st_mtime_nsec as u64)
                    .u64(st.st_ctime as u64)
                    .u64(st.st_ctime_nsec as u64)
                    .u64(0)
                    .u64(0)
                    .u64(0)
                    .u64(0);
                Ok(out.finish())
            }
            // Tsetattr
            26 => {
                let fid = m.u32()?;
                let valid = m.u32()?;
                let mode = m.u32()?;
                let uid = m.u32()?;
                let gid = m.u32()?;
                let size = m.u64()?;
                let atime = (m.u64()?, m.u64()?);
                let mtime = (m.u64()?, m.u64()?);
                let (parts, fuid) = {
                    let f = self.fid(fid)?;
                    (f.parts.clone(), f.uid)
                };
                let entry = self.fs.entry(&parts)?;
                let fd = entry.fd.as_raw_fd();
                let symlink = is_symlink(&entry.stat);
                if valid & 0x1 != 0 && !symlink {
                    chmod(fd, mode)?;
                }
                if valid & 0x6 != 0 && !symlink {
                    let (cu, cg) = owner(fd, &entry.stat, (fuid, fuid));
                    let nu = if valid & 0x2 != 0 { uid } else { cu };
                    let ng = if valid & 0x4 != 0 { gid } else { cg };
                    set_owner(fd, nu, ng)?;
                }
                if valid & 0x8 != 0 {
                    if symlink || is_dir(&entry.stat) {
                        return Err(Error::from_raw_os_error(libc::EINVAL));
                    }
                    let old = u64::try_from(entry.stat.st_size).unwrap_or(0);
                    // Growing is charged to the quota only: a truncate
                    // makes a hole, which the host does not have to fill.
                    let grow = Cost {
                        size: size.saturating_sub(old),
                        ..Cost::default()
                    };
                    self.budget.admit(grow)?;
                    let path = proc_path(fd);
                    if unsafe { libc::truncate(path.as_ptr(), size as libc::off_t) } < 0 {
                        self.budget.release(grow);
                        return Err(last_error());
                    }
                    self.budget.release(Cost {
                        size: old.saturating_sub(size),
                        ..Cost::default()
                    });
                }
                if valid & 0x30 != 0 && !symlink {
                    // ATIME 0x10, MTIME 0x20; *_SET 0x80/0x100 give the time,
                    // otherwise now.
                    let time = |set: bool, bit: u32, given: (u64, u64)| libc::timespec {
                        tv_sec: given.0 as libc::time_t,
                        tv_nsec: if valid & bit == 0 {
                            libc::UTIME_OMIT
                        } else if set {
                            given.1 as libc::c_long
                        } else {
                            libc::UTIME_NOW
                        },
                    };
                    let times = [
                        time(valid & 0x80 != 0, 0x10, atime),
                        time(valid & 0x100 != 0, 0x20, mtime),
                    ];
                    let path = proc_path(fd);
                    if unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) }
                        < 0
                    {
                        return Err(last_error());
                    }
                }
                Ok(reply(27).finish())
            }
            // Txattrwalk, Txattrcreate: extended attributes are not served.
            30 | 32 => Err(Error::from_raw_os_error(libc::EOPNOTSUPP)),
            // Treaddir
            40 => {
                let fid = m.u32()?;
                let offset = m.u64()?;
                let count = m.u32()? as usize;
                self.readdir(fid, offset, count, tag)
            }
            // Tfsync
            50 => {
                let fid = m.u32()?;
                if let Some(file) = &self.fid(fid)?.file {
                    file.sync_data()?;
                }
                Ok(reply(51).finish())
            }
            // Tlock: advisory locks are the guest's own business.
            52 => {
                let mut out = reply(53);
                out.u8(0);
                Ok(out.finish())
            }
            // Tgetlock
            54 => {
                let _fid = m.u32()?;
                let _kind = m.u8()?;
                let start = m.u64()?;
                let length = m.u64()?;
                let proc_id = m.u32()?;
                let client = m.str()?;
                let mut out = reply(55);
                out.u8(2).u64(start).u64(length).u32(proc_id).str(&client);
                Ok(out.finish())
            }
            // Tlink
            70 => {
                let dfid = m.u32()?;
                let fid = m.u32()?;
                let name = m.str()?;
                let from = self.fid(fid)?.parts.clone();
                let (to, _) = self.child(dfid, &name)?;
                let (Some((fname, fdir)), Some((tname, tdir))) =
                    (from.split_last(), to.split_last())
                else {
                    return Err(Error::from_raw_os_error(libc::EINVAL));
                };
                let (a, b) = (self.fs.dir(fdir)?, self.fs.dir(tdir)?);
                let (an, bn) = (cstring(fname)?, cstring(tname)?);
                // A link is a directory entry: no inode, no bytes of its own,
                // but it still needs room on a disk that is nearly full.
                self.budget.admit(Cost {
                    disk: 1,
                    ..Cost::default()
                })?;
                if unsafe {
                    libc::linkat(a.as_raw_fd(), an.as_ptr(), b.as_raw_fd(), bn.as_ptr(), 0)
                } < 0
                {
                    return Err(last_error());
                }
                Ok(reply(71).finish())
            }
            // Tmkdir
            72 => {
                let dfid = m.u32()?;
                let name = m.str()?;
                let mode = m.u32()?;
                let gid = m.u32()?;
                let (parts, uid) = self.child(dfid, &name)?;
                let dir = self.fs.dir(&parts[..parts.len() - 1])?;
                let n = cstring(&name)?;
                let charge = Cost {
                    inodes: 1,
                    ..Cost::default()
                };
                self.budget.admit(charge)?;
                if unsafe { libc::mkdirat(dir.as_raw_fd(), n.as_ptr(), safe_mode(mode)) } < 0 {
                    self.budget.release(charge);
                    return Err(last_error());
                }
                let entry = self.fs.entry(&parts)?;
                let _ = set_owner(entry.fd.as_raw_fd(), uid, gid);
                let mut out = reply(73);
                out.qid(qid(&entry.stat));
                Ok(out.finish())
            }
            // Trenameat
            74 => {
                let olddir = m.u32()?;
                let oldname = m.str()?;
                let newdir = m.u32()?;
                let newname = m.str()?;
                let (from, _) = self.child(olddir, &oldname)?;
                let (to, _) = self.child(newdir, &newname)?;
                self.rename(&from, &to)?;
                Ok(reply(75).finish())
            }
            // Tunlinkat
            76 => {
                let dfid = m.u32()?;
                let name = m.str()?;
                let flags = m.u32()?;
                let (parts, _) = self.child(dfid, &name)?;
                let dir = self.fs.dir(&parts[..parts.len() - 1])?;
                let n = cstring(&name)?;
                let at = if flags & 0x200 != 0 {
                    libc::AT_REMOVEDIR
                } else {
                    0
                };
                let gone = self.fs.entry(&parts).ok().map(|e| e.stat);
                if unsafe { libc::unlinkat(dir.as_raw_fd(), n.as_ptr(), at) } < 0 {
                    return Err(last_error());
                }
                if let Some(gone) = &gone {
                    self.budget.removed(gone);
                }
                Ok(reply(77).finish())
            }
            // Tstatfs
            8 => {
                let _fid = m.u32()?;
                let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
                let path = proc_path(self.fs.root.as_raw_fd());
                if unsafe { libc::statvfs(path.as_ptr(), &mut s) } < 0 {
                    return Err(last_error());
                }
                let mut out = reply(9);
                out.u32(0x0102_1997) // V9FS_MAGIC
                    .u32(s.f_bsize as u32)
                    .u64(s.f_blocks)
                    .u64(s.f_bfree)
                    .u64(s.f_bavail)
                    .u64(s.f_files)
                    .u64(s.f_ffree)
                    .u64(s.f_fsid as u64)
                    .u32(255);
                Ok(out.finish())
            }
            // Tread
            116 => {
                let fid = m.u32()?;
                let offset = m.u64()?;
                let count = (m.u32()? as usize).min(self.msize as usize - 11);
                let file = self
                    .fid(fid)?
                    .file
                    .as_ref()
                    .ok_or_else(|| Error::from_raw_os_error(libc::EBADF))?;
                let mut buf = vec![0u8; count];
                let n = read_at_full(file, &mut buf, offset)?;
                let mut out = reply(117);
                out.u32(n as u32).bytes(&buf[..n]);
                Ok(out.finish())
            }
            // Twrite
            118 => {
                let fid = m.u32()?;
                let offset = m.u64()?;
                let count = m.u32()? as usize;
                let data = m.take(count)?;
                let file = self
                    .fid(fid)?
                    .file
                    .as_ref()
                    .ok_or_else(|| Error::from_raw_os_error(libc::EBADF))?;
                // Only what passes the end of the file is new: an append
                // (which ignores the offset) is all new, a rewrite is not.
                let fd = file.as_raw_fd();
                let size = quota::size_of(fd)?;
                let appending = unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_APPEND != 0;
                let end = if appending {
                    size.saturating_add(count as u64)
                } else {
                    offset.saturating_add(count as u64)
                };
                let growth = end.saturating_sub(size);
                let charge = Cost {
                    size: growth,
                    disk: growth.min(count as u64),
                    inodes: 0,
                };
                if growth > 0 {
                    self.budget.admit(charge)?;
                }
                if let Err(e) = file.write_all_at(data, offset) {
                    if growth > 0 {
                        self.budget.release(charge);
                    }
                    return Err(e);
                }
                let mut out = reply(119);
                out.u32(count as u32);
                Ok(out.finish())
            }
            // Tclunk
            120 => {
                let fid = m.u32()?;
                self.fids.remove(&fid);
                Ok(reply(121).finish())
            }
            // Tremove
            122 => {
                let fid = m.u32()?;
                let f = self
                    .fids
                    .remove(&fid)
                    .ok_or_else(|| Error::from_raw_os_error(libc::EBADF))?;
                let Some((name, dir_parts)) = f.parts.split_last() else {
                    return Err(Error::from_raw_os_error(libc::EBUSY));
                };
                let entry = self.fs.entry(&f.parts)?;
                let dir = self.fs.dir(dir_parts)?;
                let n = cstring(name)?;
                let at = if is_dir(&entry.stat) {
                    libc::AT_REMOVEDIR
                } else {
                    0
                };
                if unsafe { libc::unlinkat(dir.as_raw_fd(), n.as_ptr(), at) } < 0 {
                    return Err(last_error());
                }
                self.budget.removed(&entry.stat);
                Ok(reply(123).finish())
            }
            _ => Err(Error::from_raw_os_error(libc::EOPNOTSUPP)),
        }
    }

    fn rename(&self, from: &[String], to: &[String]) -> std::io::Result<()> {
        let (Some((fname, fdir)), Some((tname, tdir))) = (from.split_last(), to.split_last())
        else {
            return Err(Error::from_raw_os_error(libc::EBUSY));
        };
        let (a, b) = (self.fs.dir(fdir)?, self.fs.dir(tdir)?);
        let (an, bn) = (cstring(fname)?, cstring(tname)?);
        // A rename over an existing name removes what was there.
        let moved = self.fs.entry(from).ok().map(|e| e.stat);
        let replaced = self.fs.entry(to).ok().map(|e| e.stat);
        if unsafe { libc::renameat(a.as_raw_fd(), an.as_ptr(), b.as_raw_fd(), bn.as_ptr()) } < 0 {
            return Err(last_error());
        }
        if let (Some(moved), Some(replaced)) = (&moved, &replaced) {
            if (moved.st_dev, moved.st_ino) != (replaced.st_dev, replaced.st_ino) {
                self.budget.removed(replaced);
            }
        }
        Ok(())
    }

    /// The file behind `fd` was opened with `O_TRUNC` when it was `before`
    /// bytes long: what it no longer holds is given back.
    fn truncated(&self, fd: RawFd, before: i64) {
        let before = u64::try_from(before).unwrap_or(0);
        if let Ok(after) = quota::size_of(fd) {
            self.budget.release(Cost {
                size: before.saturating_sub(after),
                ..Cost::default()
            });
        }
    }

    /// `Treaddir`: the entries from `offset`, as many as fit in `count`. The
    /// directory is read as far as the reply goes and no further, so a
    /// directory of millions of entries costs a screenful per call, not the
    /// node's memory.
    fn readdir(
        &mut self,
        fid: u32,
        offset: u64,
        count: usize,
        tag: u16,
    ) -> std::io::Result<Vec<u8>> {
        let parts = self.fid(fid)?.parts.clone();
        let here_dir = self.fs.dir(&parts)?;
        let here = fstat(here_dir.as_raw_fd())?;
        let mut listing = match self.fid_mut(fid)?.listing.take() {
            Some(listing) if listing.next == offset => listing,
            // From the start, or from somewhere other than where the last
            // reply stopped: read the directory again, up to there.
            _ => {
                let path = proc_path(here_dir.as_raw_fd());
                let mut entries =
                    std::fs::read_dir(std::ffi::OsStr::new(path.to_str().expect("ascii")))?;
                for _ in 2..offset {
                    if entries.next().is_none() {
                        break;
                    }
                }
                Listing {
                    entries,
                    next: offset,
                    held: None,
                }
            }
        };
        let limit = count.min(self.msize as usize - 11);
        let mut data = Out(Vec::new());
        loop {
            let item = match listing.held.take() {
                Some(item) => Some(item),
                None if listing.next == 0 => Some((qid(&here), 4, ".".to_string())),
                None if listing.next == 1 => Some((qid(&here), 4, "..".to_string())),
                None => next_entry(&self.fs, &parts, &mut listing.entries),
            };
            let Some((q, dtype, name)) = item else {
                break;
            };
            let size = 13 + 8 + 1 + 2 + name.len();
            if data.0.len() + size > limit {
                listing.held = Some((q, dtype, name));
                break;
            }
            data.qid(q).u64(listing.next + 1).u8(dtype).str(&name);
            listing.next += 1;
        }
        self.fid_mut(fid)?.listing = Some(listing);
        let mut out = Out::new(41, tag);
        out.u32(data.0.len() as u32).bytes(&data.0);
        Ok(out.finish())
    }
}

/// The next entry of a directory the guest may see, with its `stat`: those
/// that vanished since they were listed, or are not nameable, are passed over.
fn next_entry(fs: &Beneath, parts: &[String], entries: &mut std::fs::ReadDir) -> Option<DirEntry> {
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !valid_name(&name) {
            continue;
        }
        let mut child = parts.to_vec();
        child.push(name.clone());
        let Ok(e) = fs.entry(&child) else {
            continue;
        };
        let dtype = match e.stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => 4,
            libc::S_IFLNK => 10,
            libc::S_IFREG => 8,
            _ => 0,
        };
        return Some((qid(&e.stat), dtype, name));
    }
    None
}

fn read_at_full(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match file.read_at(&mut buf[done..], offset + done as u64) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// Serve 9P on `channel` until the guest unmounts or the VM stops: read
/// whole messages, answer each. `pending` is what already arrived with the
/// mount's acknowledgement.
pub fn serve(mut channel: Box<dyn GuestChannel>, mut pending: Vec<u8>, mut server: Server) {
    let mut outbox: Vec<u8> = Vec::new();
    loop {
        if !channel.open() {
            return;
        }
        match channel.recv() {
            Ok(bytes) => pending.extend_from_slice(&bytes),
            Err(_) => return,
        }
        let mut progressed = false;
        while pending.len() >= 4 {
            let size = u32::from_le_bytes(pending[..4].try_into().expect("4")) as usize;
            if size < 7 || size > MAX_MSIZE as usize + 4096 {
                return; // not 9P: drop the connection
            }
            if pending.len() < size {
                break;
            }
            let msg: Vec<u8> = pending.drain(..size).collect();
            outbox.extend_from_slice(&server.handle(&msg));
            progressed = true;
        }
        while !outbox.is_empty() {
            match channel.send(&outbox) {
                Ok(0) => {
                    if !channel.open() {
                        return;
                    }
                    channel.wait(Duration::from_millis(5));
                }
                Ok(n) => {
                    outbox.drain(..n);
                    progressed = true;
                }
                Err(_) => return,
            }
        }
        if !progressed {
            channel.wait(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("hv2-9p-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn msg(kind: u8, build: impl FnOnce(&mut Out)) -> Vec<u8> {
        let mut out = Out::new(kind, 1);
        build(&mut out);
        out.finish()
    }

    fn kind(reply: &[u8]) -> u8 {
        reply[4]
    }

    fn error(reply: &[u8]) -> Option<i32> {
        (reply[4] == RLERROR).then(|| i32::from_le_bytes(reply[7..11].try_into().unwrap()))
    }

    fn attached(dir: &std::path::Path) -> Server {
        handshake(Server::new(Beneath::open(dir).unwrap()))
    }

    fn handshake(mut s: Server) -> Server {
        let r = s.handle(&msg(100, |o| {
            o.u32(65536).str("9P2000.L");
        }));
        assert_eq!(kind(&r), 101);
        let r = s.handle(&msg(104, |o| {
            o.u32(0).u32(NOFID).str("").str("").u32(1000);
        }));
        assert_eq!(kind(&r), 105, "attach: {:?}", error(&r));
        s
    }

    fn walk(s: &mut Server, from: u32, to: u32, names: &[&str]) -> Vec<u8> {
        s.handle(&msg(110, |o| {
            o.u32(from).u32(to).u16(names.len() as u16);
            for n in names {
                o.str(n);
            }
        }))
    }

    #[test]
    fn files_are_created_written_read_and_owned_by_the_guest_user() {
        let tmp = Tmp::new();
        let mut s = attached(&tmp.0);
        assert_eq!(kind(&walk(&mut s, 0, 1, &[])), 111);
        let r = s.handle(&msg(14, |o| {
            o.u32(1)
                .str("hello.txt")
                .u32(libc::O_RDWR as u32)
                .u32(0o644)
                .u32(1000);
        }));
        assert_eq!(kind(&r), 15, "create: {:?}", error(&r));
        let r = s.handle(&msg(118, |o| {
            o.u32(1).u64(0).u32(5).bytes(b"hello");
        }));
        assert_eq!(kind(&r), 119);
        assert_eq!(std::fs::read(tmp.0.join("hello.txt")).unwrap(), b"hello");

        let r = s.handle(&msg(116, |o| {
            o.u32(1).u64(1).u32(100);
        }));
        assert_eq!(&r[11..], b"ello");

        assert_eq!(kind(&walk(&mut s, 0, 2, &["hello.txt"])), 111);
        let r = s.handle(&msg(24, |o| {
            o.u32(2).u64(0x7ff);
        }));
        assert_eq!(kind(&r), 25);
        // valid[8] qid[13] mode[4] uid[4] gid[4]
        let uid = u32::from_le_bytes(r[7 + 8 + 13 + 4..7 + 8 + 13 + 8].try_into().unwrap());
        assert_eq!(uid, 1000, "the guest's owner, not the host's");
    }

    #[test]
    fn nothing_outside_the_volume_is_reachable() {
        let tmp = Tmp::new();
        let outside = Tmp::new();
        std::fs::write(outside.0.join("secret"), b"host").unwrap();
        std::os::unix::fs::symlink(&outside.0, tmp.0.join("escape")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", tmp.0.join("passwd")).unwrap();
        let mut s = attached(&tmp.0);

        // `..` from the root stays at the root.
        let r = walk(&mut s, 0, 1, &["..", ".."]);
        assert_eq!(kind(&r), 111);
        let r = s.handle(&msg(12, |o| {
            o.u32(1).u32(0);
        }));
        assert_eq!(kind(&r), 13);
        let r = s.handle(&msg(40, |o| {
            o.u32(1).u64(0).u32(8192);
        }));
        let listing = String::from_utf8_lossy(&r);
        assert!(listing.contains("escape") && !listing.contains("secret"));

        // Walking through a symlink stops at it: a partial walk, as 9P
        // answers one, and no fid for what lies beyond.
        let r = walk(&mut s, 0, 2, &["escape", "secret"]);
        assert_eq!(kind(&r), 111);
        assert_eq!(
            u16::from_le_bytes([r[7], r[8]]),
            1,
            "only the symlink resolved"
        );
        let r = s.handle(&msg(12, |o| {
            o.u32(2).u32(0);
        }));
        assert_eq!(error(&r), Some(libc::EBADF), "fid 2 was never made");
        let r = walk(&mut s, 0, 3, &["passwd"]);
        assert_eq!(kind(&r), 111, "the symlink itself may be named");
        let r = s.handle(&msg(12, |o| {
            o.u32(3).u32(0);
        }));
        assert!(error(&r).is_some(), "but not opened through");

        // Names with slashes, and creating through the symlink, are refused.
        assert!(error(&walk(&mut s, 0, 4, &["escape/secret"])).is_some());
        assert_eq!(kind(&walk(&mut s, 0, 5, &[])), 111);
        let r = s.handle(&msg(72, |o| {
            o.u32(5).str("../x").u32(0o755).u32(0);
        }));
        assert!(error(&r).is_some());
        assert!(!outside.0.join("x").exists());
        assert_eq!(std::fs::read(outside.0.join("secret")).unwrap(), b"host");
    }

    #[test]
    fn directories_are_made_listed_renamed_and_removed() {
        let tmp = Tmp::new();
        let mut s = attached(&tmp.0);
        assert_eq!(kind(&walk(&mut s, 0, 1, &[])), 111);
        let r = s.handle(&msg(72, |o| {
            o.u32(1).str("d").u32(0o755).u32(1000);
        }));
        assert_eq!(kind(&r), 73);
        let r = s.handle(&msg(74, |o| {
            o.u32(1).str("d").u32(1).str("e");
        }));
        assert_eq!(kind(&r), 75);
        assert!(tmp.0.join("e").is_dir());
        let r = s.handle(&msg(76, |o| {
            o.u32(1).str("e").u32(0x200);
        }));
        assert_eq!(kind(&r), 77);
        assert!(!tmp.0.join("e").exists());
    }

    /// A `Tlopen` / `Tlcreate` style helper: the reply, and (for a create) the
    /// qid's presence is the kind.
    fn create(s: &mut Server, dir: u32, name: &str, mode: u32) -> Vec<u8> {
        s.handle(&msg(14, |o| {
            o.u32(dir)
                .str(name)
                .u32(libc::O_RDWR as u32)
                .u32(mode)
                .u32(1000);
        }))
    }

    /// A guest that makes fid after fid, none of which it clunks, is refused at
    /// the cap -- with `EMFILE`, and with the fids it has still good -- rather
    /// than growing the node's memory without end.
    #[test]
    fn the_fid_n_plus_one_is_refused() {
        let tmp = Tmp::new();
        let mut s = attached(&tmp.0);
        // Fid 0 is the attach; clones of it fill the table to the cap.
        for fid in 1..MAX_FIDS as u32 {
            assert_eq!(kind(&walk(&mut s, 0, fid, &[])), 111, "fid {fid}");
        }
        assert_eq!(s.fids.len(), MAX_FIDS);
        let refused = walk(&mut s, 0, MAX_FIDS as u32, &[]);
        assert_eq!(error(&refused), Some(libc::EMFILE));
        // Attaching afresh is a new fid too.
        let attach = s.handle(&msg(104, |o| {
            o.u32(99_999).u32(NOFID).str("").str("").u32(1000);
        }));
        assert_eq!(error(&attach), Some(libc::EMFILE));
        assert_eq!(s.fids.len(), MAX_FIDS, "a refusal makes nothing");
        // Naming a fid it already holds replaces it, which needs no room...
        assert_eq!(kind(&walk(&mut s, 0, 7, &[])), 111);
        // ...and a clunk makes room for one new.
        assert_eq!(
            kind(&s.handle(&msg(120, |o| {
                o.u32(7);
            }))),
            121
        );
        assert_eq!(kind(&walk(&mut s, 0, MAX_FIDS as u32, &[])), 111);
    }

    /// Open files are the scarce part: each is a host descriptor. They are
    /// capped apart from fids, a create past the cap makes no file, and a
    /// clunk gives the share back.
    #[test]
    fn files_open_past_the_cap_are_refused_and_made_nowhere() {
        let tmp = Tmp::new();
        let mut s = attached(&tmp.0);
        s.max_open_files = 2;
        for (fid, name) in [(1, "a"), (2, "b"), (3, "c")] {
            assert_eq!(kind(&walk(&mut s, 0, fid, &[])), 111);
            let r = create(&mut s, fid, name, 0o644);
            if fid < 3 {
                assert_eq!(kind(&r), 15, "{name}: {:?}", error(&r));
            } else {
                assert_eq!(error(&r), Some(libc::EMFILE));
                assert!(!tmp.0.join(name).exists(), "a refused create makes no file");
            }
        }
        assert_eq!(s.open_files.load(std::sync::atomic::Ordering::SeqCst), 2);
        // An open of an existing file counts too.
        assert_eq!(kind(&walk(&mut s, 0, 4, &["a"])), 111);
        let r = s.handle(&msg(12, |o| {
            o.u32(4).u32(0);
        }));
        assert_eq!(error(&r), Some(libc::EMFILE));
        // Clunking one gives its share back.
        assert_eq!(
            kind(&s.handle(&msg(120, |o| {
                o.u32(1);
            }))),
            121
        );
        let r = s.handle(&msg(12, |o| {
            o.u32(4).u32(0);
        }));
        assert_eq!(kind(&r), 13, "{:?}", error(&r));
        drop(s);
    }

    /// A guest's setuid and setgid bits are not kept, however they are set:
    /// by create, by chmod, by mkdir. The sticky bit and permissions are.
    #[test]
    fn setuid_and_setgid_bits_are_not_stored() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = Tmp::new();
        let mut s = attached(&tmp.0);
        assert_eq!(kind(&walk(&mut s, 0, 1, &[])), 111);
        let mode = |name: &str| {
            std::fs::symlink_metadata(tmp.0.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        let r = create(&mut s, 1, "suid", 0o4755);
        assert_eq!(kind(&r), 15, "{:?}", error(&r));
        assert_eq!(mode("suid"), 0o755);
        // (A create turns its fid into the new file, so each needs its own.)
        assert_eq!(kind(&walk(&mut s, 0, 6, &[])), 111);
        let r = create(&mut s, 6, "both", 0o6700);
        assert_eq!(kind(&r), 15);
        assert_eq!(mode("both"), 0o700);

        assert_eq!(kind(&walk(&mut s, 0, 2, &[])), 111);
        let r = s.handle(&msg(72, |o| {
            o.u32(2).str("d").u32(0o2755).u32(1000);
        }));
        assert_eq!(kind(&r), 73);
        assert_eq!(mode("d"), 0o755);

        // chmod after the fact, on a file and on a directory.
        let setattr = |s: &mut Server, fid: u32, mode: u32| {
            s.handle(&msg(26, |o| {
                o.u32(fid).u32(0x1).u32(mode).u32(0).u32(0).u64(0);
                o.u64(0).u64(0).u64(0).u64(0);
            }))
        };
        assert_eq!(kind(&walk(&mut s, 0, 3, &["suid"])), 111);
        assert_eq!(kind(&setattr(&mut s, 3, 0o6755)), 27);
        assert_eq!(mode("suid"), 0o755);
        assert_eq!(kind(&walk(&mut s, 0, 4, &["d"])), 111);
        assert_eq!(kind(&setattr(&mut s, 4, 0o3777)), 27);
        assert_eq!(mode("d"), 0o1777, "sticky is kept, setgid is not");
        // What the guest reads back is what is stored.
        let r = s.handle(&msg(24, |o| {
            o.u32(3).u64(0x7ff);
        }));
        let st_mode = u32::from_le_bytes(r[7 + 8 + 13..7 + 8 + 13 + 4].try_into().unwrap());
        assert_eq!(st_mode & 0o7777, 0o755);
    }

    /// The entries of a `Rreaddir`, and the offset to ask for the next ones.
    fn dir_entries(reply: &[u8]) -> (Vec<String>, u64) {
        assert_eq!(kind(reply), 41, "{:?}", error(reply));
        let len = u32::from_le_bytes(reply[7..11].try_into().unwrap()) as usize;
        let mut at = 11;
        let mut names = Vec::new();
        let mut next = 0;
        while at < 11 + len {
            next = u64::from_le_bytes(reply[at + 13..at + 21].try_into().unwrap());
            let n = u16::from_le_bytes(reply[at + 22..at + 24].try_into().unwrap()) as usize;
            names.push(String::from_utf8(reply[at + 24..at + 24 + n].to_vec()).unwrap());
            at += 24 + n;
        }
        (names, next)
    }

    /// A big directory is listed in pieces: each reply reads the directory
    /// only as far as it needs, every entry appears once, and asking again
    /// from the start (or from the middle) works.
    #[test]
    fn a_directory_is_listed_in_pieces_not_whole() {
        let tmp = Tmp::new();
        let total = 400;
        for i in 0..total {
            std::fs::write(tmp.0.join(format!("file-{i:04}")), b"").unwrap();
        }
        let mut s = attached(&tmp.0);
        assert_eq!(kind(&walk(&mut s, 0, 1, &[])), 111);
        let r = s.handle(&msg(12, |o| {
            o.u32(1).u32(0);
        }));
        assert_eq!(kind(&r), 13);

        let ask = |s: &mut Server, offset: u64, count: u32| {
            s.handle(&msg(40, |o| {
                o.u32(1).u64(offset).u32(count);
            }))
        };
        let mut seen = Vec::new();
        let mut offset = 0;
        let mut replies = 0;
        loop {
            let (names, next) = dir_entries(&ask(&mut s, offset, 600));
            if names.is_empty() {
                break;
            }
            replies += 1;
            // Only as much of the directory as has been asked for is read.
            let read = s.fids[&1].listing.as_ref().unwrap().next;
            assert_eq!(read, next);
            assert!(read < total as u64 + 3);
            seen.extend(names);
            offset = next;
        }
        assert!(replies > 10, "listed in many pieces: {replies}");
        assert_eq!(seen.len(), total + 2);
        assert_eq!(&seen[..2], [".", ".."]);
        let mut real: Vec<_> = seen[2..].to_vec();
        real.sort();
        real.dedup();
        assert_eq!(real.len(), total, "each entry once");

        // The first reply held only a handful: the rest was never read.
        let (first, next) = dir_entries(&ask(&mut s, 0, 300));
        assert!(first.len() < 12, "{}", first.len());
        assert_eq!(next as usize, first.len());
        // Asking again from the middle gives what followed it, as before.
        let (again, _) = dir_entries(&ask(&mut s, 50, 300));
        assert_eq!(again[0], seen[50]);
    }

    #[test]
    fn logical_paths_do_not_climb_out() {
        assert_eq!(components("a/./b/../c").unwrap(), ["a", "c"]);
        assert!(components("../x").is_err());
        assert!(!valid_name("a/b") && !valid_name("..") && valid_name("a b"));
    }

    // ---- space: the host-disk guard and the per-volume quota ----

    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    const GIB: u64 = 1024 * 1024 * 1024;

    /// A disk the test controls: 10 GiB with `avail` bytes and `inodes` of
    /// 1000 inodes free, and a count of how often it was asked.
    struct Disk {
        avail: Arc<AtomicU64>,
        inodes: Arc<AtomicU64>,
        asked: Arc<AtomicUsize>,
    }

    impl Disk {
        fn new(avail: u64) -> Self {
            Self {
                avail: Arc::new(AtomicU64::new(avail)),
                inodes: Arc::new(AtomicU64::new(1000)),
                asked: Arc::new(AtomicUsize::new(0)),
            }
        }
        fn budget(&self, policy: quota::Policy, ttl: Duration) -> Arc<Budget> {
            let (a, i, n) = (
                Arc::clone(&self.avail),
                Arc::clone(&self.inodes),
                Arc::clone(&self.asked),
            );
            Arc::new(Budget::new(
                policy,
                Box::new(move || {
                    n.fetch_add(1, Ordering::SeqCst);
                    Ok(quota::Space {
                        avail_bytes: a.load(Ordering::SeqCst),
                        total_bytes: 10 * GIB,
                        avail_inodes: i.load(Ordering::SeqCst),
                        total_inodes: 1000,
                    })
                }),
                ttl,
            ))
        }
    }

    fn quota_of(max_bytes: u64, max_inodes: u64) -> quota::Policy {
        quota::Policy {
            reserve_percent: 0,
            max_bytes,
            max_inodes,
        }
    }

    fn served(dir: &std::path::Path, budget: &Arc<Budget>) -> Server {
        let mut s = handshake(Server::with_budget(
            Beneath::open(dir).unwrap(),
            Arc::clone(budget),
        ));
        assert_eq!(kind(&walk(&mut s, 0, 1, &[])), 111);
        s
    }

    /// Make `name` in the root, left open on `fid`.
    fn make(s: &mut Server, fid: u32, name: &str) -> Vec<u8> {
        assert_eq!(kind(&walk(s, 0, fid, &[])), 111);
        create(s, fid, name, 0o644)
    }

    fn write(s: &mut Server, fid: u32, offset: u64, data: &[u8]) -> Vec<u8> {
        s.handle(&msg(118, |o| {
            o.u32(fid).u64(offset).u32(data.len() as u32).bytes(data);
        }))
    }

    fn set_size(s: &mut Server, fid: u32, size: u64) -> Vec<u8> {
        s.handle(&msg(26, |o| {
            o.u32(fid).u32(0x8).u32(0).u32(0).u32(0).u64(size);
            o.u64(0).u64(0).u64(0).u64(0);
        }))
    }

    fn unlink(s: &mut Server, name: &str) -> Vec<u8> {
        s.handle(&msg(76, |o| {
            o.u32(1).str(name).u32(0);
        }))
    }

    /// With the disk under its reserve, nothing that grows storage is
    /// accepted -- not a create, a write, a mkdir or a symlink -- and each is
    /// refused with `ENOSPC` and leaves nothing on the host.
    #[test]
    fn a_nearly_full_host_disk_refuses_growth_with_enospc() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota::Policy::DEFAULT, Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "ok")), 15);
        assert_eq!(kind(&write(&mut s, 2, 0, b"fine")), 119);

        // 5% of 10 GiB is the reserve; 100 MiB free is under it.
        disk.avail.store(100 * 1024 * 1024, Ordering::SeqCst);
        let r = make(&mut s, 3, "refused");
        assert_eq!(error(&r), Some(libc::ENOSPC));
        assert!(!tmp.0.join("refused").exists());
        assert_eq!(error(&write(&mut s, 2, 4, b"more")), Some(libc::ENOSPC));
        assert_eq!(std::fs::read(tmp.0.join("ok")).unwrap(), b"fine");
        let r = s.handle(&msg(72, |o| {
            o.u32(1).str("d").u32(0o755).u32(1000);
        }));
        assert_eq!(error(&r), Some(libc::ENOSPC));
        assert!(!tmp.0.join("d").exists());
        let r = s.handle(&msg(16, |o| {
            o.u32(1).str("l").str("target").u32(1000);
        }));
        assert_eq!(error(&r), Some(libc::ENOSPC));
        assert!(std::fs::symlink_metadata(tmp.0.join("l")).is_err());

        // Space comes back: the same guest carries on.
        disk.avail.store(5 * GIB, Ordering::SeqCst);
        assert_eq!(kind(&write(&mut s, 2, 4, b"more")), 119);
    }

    /// The reserve is 5% or 2 GiB, whichever is less; a write that would
    /// leave less than it is refused before it is made, not after.
    #[test]
    fn the_reserve_is_capped_and_counts_the_write_itself() {
        let tmp = Tmp::new();
        let disk = Disk::new(0);
        let budget = disk.budget(quota::Policy::DEFAULT, Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        // 10 GiB disk: 5% is 512 MiB. Exactly the reserve plus the write is
        // allowed; one byte less is not.
        disk.avail.store(512 * 1024 * 1024 + 4, Ordering::SeqCst);
        assert_eq!(kind(&make(&mut s, 2, "f")), 15);
        assert_eq!(kind(&write(&mut s, 2, 0, b"1234")), 119);
        disk.avail.store(512 * 1024 * 1024 + 3, Ordering::SeqCst);
        assert_eq!(error(&write(&mut s, 2, 4, b"1234")), Some(libc::ENOSPC));

        // On a big disk the cap applies: 5% of 1 TiB would be 51 GiB.
        let on_big_disk = |avail: u64| {
            Budget::new(
                quota::Policy::DEFAULT,
                Box::new(move || {
                    Ok(quota::Space {
                        avail_bytes: avail,
                        total_bytes: 1024 * GIB,
                        avail_inodes: 0,
                        total_inodes: 0,
                    })
                }),
                Duration::ZERO,
            )
        };
        let one_byte = quota::Cost {
            size: 1,
            disk: 1,
            inodes: 0,
        };
        assert!(on_big_disk(3 * GIB).admit(one_byte).is_ok());
        let r = on_big_disk(2 * GIB).admit(one_byte);
        assert_eq!(r.unwrap_err().raw_os_error(), Some(libc::ENOSPC));
    }

    #[test]
    fn running_out_of_inodes_refuses_creates_and_percent_zero_turns_the_guard_off() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota::Policy::DEFAULT, Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        // 1% of 1000 inodes is 10 to keep.
        disk.inodes.store(10, Ordering::SeqCst);
        assert_eq!(error(&make(&mut s, 2, "a")), Some(libc::ENOSPC));
        assert!(!tmp.0.join("a").exists());
        disk.inodes.store(12, Ordering::SeqCst);
        assert_eq!(kind(&make(&mut s, 2, "a")), 15);

        // Off: a disk with nothing free is not this server's business.
        let off = disk.budget(
            quota::Policy {
                reserve_percent: 0,
                ..quota::Policy::DEFAULT
            },
            Duration::ZERO,
        );
        disk.avail.store(0, Ordering::SeqCst);
        disk.inodes.store(0, Ordering::SeqCst);
        let tmp2 = Tmp::new();
        let mut s = served(&tmp2.0, &off);
        assert_eq!(kind(&make(&mut s, 2, "b")), 15);
        assert_eq!(kind(&write(&mut s, 2, 0, b"x")), 119);
    }

    /// `statvfs` is not a syscall per write: within its lifetime one answer
    /// serves every write, with what was admitted since subtracted from it.
    #[test]
    fn the_disk_is_asked_once_per_interval_not_once_per_write() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota::Policy::DEFAULT, Duration::from_secs(3600));
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "f")), 15);
        for i in 0..50u64 {
            assert_eq!(kind(&write(&mut s, 2, i * 4, b"abcd")), 119);
        }
        assert_eq!(disk.asked.load(Ordering::SeqCst), 1);

        // Within one answer, growth already admitted counts against it:
        // 28 MiB above a 512 MiB reserve admits 28 MiB, not more.
        let disk = Disk::new(512 * 1024 * 1024 + 28 * 1024 * 1024);
        let budget = disk.budget(quota::Policy::DEFAULT, Duration::from_secs(3600));
        let grow = |n| quota::Cost {
            size: n,
            disk: n,
            inodes: 0,
        };
        let mib = 1024 * 1024;
        assert!(budget.admit(grow(10 * mib)).is_ok());
        assert!(budget.admit(grow(10 * mib)).is_ok());
        let r = budget.admit(grow(10 * mib));
        assert_eq!(r.unwrap_err().raw_os_error(), Some(libc::ENOSPC));
    }

    #[test]
    fn a_byte_quota_refuses_the_write_that_would_pass_it() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota_of(100, 0), Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "f")), 15);
        assert_eq!(kind(&write(&mut s, 2, 0, &[1; 60])), 119);
        // 60 more would be 120.
        assert_eq!(error(&write(&mut s, 2, 60, &[2; 60])), Some(libc::EDQUOT));
        assert_eq!(std::fs::metadata(tmp.0.join("f")).unwrap().len(), 60);
        // Exactly the limit is fine; one more byte is not.
        assert_eq!(kind(&write(&mut s, 2, 60, &[2; 40])), 119);
        assert_eq!(error(&write(&mut s, 2, 100, &[3])), Some(libc::EDQUOT));
        // Rewriting what is there is not growth.
        assert_eq!(kind(&write(&mut s, 2, 0, &[9; 100])), 119);
        // Nor can a truncate make the file bigger than the quota.
        assert_eq!(error(&set_size(&mut s, 2, 101)), Some(libc::EDQUOT));
        assert_eq!(std::fs::metadata(tmp.0.join("f")).unwrap().len(), 100);
        assert_eq!(budget.used(), (100, 1));
        // A second file shares the volume's allowance.
        assert_eq!(kind(&make(&mut s, 3, "g")), 15);
        assert_eq!(error(&write(&mut s, 3, 0, &[1])), Some(libc::EDQUOT));
    }

    #[test]
    fn an_inode_quota_refuses_the_create() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota_of(0, 2), Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "a")), 15);
        let r = s.handle(&msg(72, |o| {
            o.u32(1).str("d").u32(0o755).u32(1000);
        }));
        assert_eq!(kind(&r), 73);
        assert_eq!(error(&make(&mut s, 3, "b")), Some(libc::EDQUOT));
        assert!(!tmp.0.join("b").exists());
        let r = s.handle(&msg(16, |o| {
            o.u32(1).str("l").str("t").u32(1000);
        }));
        assert_eq!(error(&r), Some(libc::EDQUOT));
        assert!(std::fs::symlink_metadata(tmp.0.join("l")).is_err());
        // Opening a name that is already there makes nothing: allowed.
        assert_eq!(kind(&make(&mut s, 4, "a")), 15);
    }

    #[test]
    fn deleting_frees_quota() {
        let tmp = Tmp::new();
        let disk = Disk::new(10 * GIB);
        let budget = disk.budget(quota_of(100, 2), Duration::ZERO);
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "a")), 15);
        assert_eq!(kind(&write(&mut s, 2, 0, &[1; 100])), 119);
        assert_eq!(kind(&make(&mut s, 3, "b")), 15);
        assert_eq!(error(&make(&mut s, 4, "c")), Some(libc::EDQUOT));
        assert_eq!(error(&write(&mut s, 3, 0, &[1])), Some(libc::EDQUOT));
        assert_eq!(kind(&unlink(&mut s, "a")), 77);
        assert_eq!(budget.used(), (0, 1));
        // Both the inode and the 100 bytes are free again.
        assert_eq!(kind(&write(&mut s, 3, 0, &[1; 100])), 119);
        assert_eq!(kind(&make(&mut s, 4, "c")), 15);

        // A truncate gives bytes back; so does renaming another file over
        // one that holds some.
        assert_eq!(kind(&set_size(&mut s, 3, 10)), 27);
        assert_eq!(budget.used(), (10, 2));
        assert_eq!(kind(&write(&mut s, 3, 10, &[1; 90])), 119);
        let r = s.handle(&msg(74, |o| {
            o.u32(1).str("c").u32(1).str("b");
        }));
        assert_eq!(kind(&r), 75);
        assert_eq!(budget.used(), (0, 1));
    }

    #[test]
    fn what_a_volume_already_holds_counts_against_its_quota() {
        let tmp = Tmp::new();
        std::fs::write(tmp.0.join("old"), [0u8; 50]).unwrap();
        std::fs::create_dir(tmp.0.join("sub")).unwrap();
        std::fs::write(tmp.0.join("sub/older"), [0u8; 30]).unwrap();
        std::fs::hard_link(tmp.0.join("old"), tmp.0.join("sub/link")).unwrap();
        let budget = Arc::new(Budget::scanned(
            quota_of(100, 0),
            Box::new(|| {
                Ok(quota::Space {
                    avail_bytes: 10 * GIB,
                    total_bytes: 10 * GIB,
                    avail_inodes: 1000,
                    total_inodes: 1000,
                })
            }),
            Duration::ZERO,
            &tmp.0,
        ));
        // 50 + 30 bytes (the hard link is the same inode); old, sub, older.
        assert_eq!(budget.used(), (80, 3));
        let mut s = served(&tmp.0, &budget);
        assert_eq!(kind(&make(&mut s, 2, "new")), 15);
        assert_eq!(error(&write(&mut s, 2, 0, &[1; 21])), Some(libc::EDQUOT));
        assert_eq!(kind(&write(&mut s, 2, 0, &[1; 20])), 119);
    }
}
