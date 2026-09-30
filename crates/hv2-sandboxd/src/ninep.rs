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

/// The attribute holding a file's owner in the guest, `"uid:gid"`.
const OWNER_XATTR: &str = "user.hv2.owner";

/// The largest message this server negotiates: big enough that a read or
/// write moves a useful amount per round trip.
const MAX_MSIZE: u32 = 512 * 1024;

/// `NOFID`: a fid slot left empty.
const NOFID: u32 = u32::MAX;

/// A directory, and everything beneath it -- nothing else.
pub struct Beneath {
    root: OwnedFd,
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
        })
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
            u64::from(mode)
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
    if unsafe { libc::chmod(path.as_ptr(), mode & 0o7777) } < 0 {
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
    file: Option<File>,
    /// A directory's listing, taken when read from the start.
    listing: Option<Vec<(Qid, u8, String)>>,
}

/// A 9P2000.L server for one mount of one volume.
pub struct Server {
    fs: Beneath,
    fids: HashMap<u32, Fid>,
    msize: u32,
}

const RLERROR: u8 = 7;

impl Server {
    pub fn new(fs: Beneath) -> Self {
        Self {
            fs,
            fids: HashMap::new(),
            msize: 8192,
        }
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
                let fd = self.fs.open_at(&parts, open_flags, 0)?;
                let f = self.fid_mut(fid)?;
                f.file = Some(File::from(fd));
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
                let fd = self.fs.open_at(&parts, flags, mode & 0o7777)?;
                let _ = set_owner(fd.as_raw_fd(), uid, gid);
                let st = fstat(fd.as_raw_fd())?;
                let f = self.fid_mut(fid)?;
                f.parts = parts;
                f.file = Some(File::from(fd));
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
                if unsafe { libc::symlinkat(t.as_ptr(), dir.as_raw_fd(), n.as_ptr()) } < 0 {
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
                    let path = proc_path(fd);
                    if unsafe { libc::truncate(path.as_ptr(), size as libc::off_t) } < 0 {
                        return Err(last_error());
                    }
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
                if unsafe { libc::mkdirat(dir.as_raw_fd(), n.as_ptr(), mode & 0o7777) } < 0 {
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
                if unsafe { libc::unlinkat(dir.as_raw_fd(), n.as_ptr(), at) } < 0 {
                    return Err(last_error());
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
                file.write_all_at(data, offset)?;
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
        if unsafe { libc::renameat(a.as_raw_fd(), an.as_ptr(), b.as_raw_fd(), bn.as_ptr()) } < 0 {
            return Err(last_error());
        }
        Ok(())
    }

    fn readdir(
        &mut self,
        fid: u32,
        offset: u64,
        count: usize,
        tag: u16,
    ) -> std::io::Result<Vec<u8>> {
        let parts = self.fid(fid)?.parts.clone();
        if offset == 0 || self.fid(fid)?.listing.is_none() {
            let dir = self.fs.dir(&parts)?;
            let here = fstat(dir.as_raw_fd())?;
            let mut listing = vec![
                (qid(&here), 4, ".".to_string()),
                (qid(&here), 4, "..".to_string()),
            ];
            let path = proc_path(dir.as_raw_fd());
            for entry in std::fs::read_dir(std::ffi::OsStr::new(path.to_str().expect("ascii")))? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let mut child = parts.clone();
                child.push(name.clone());
                let Ok(e) = self.fs.entry(&child) else {
                    continue;
                };
                let dtype = match e.stat.st_mode & libc::S_IFMT {
                    libc::S_IFDIR => 4,
                    libc::S_IFLNK => 10,
                    libc::S_IFREG => 8,
                    _ => 0,
                };
                listing.push((qid(&e.stat), dtype, name));
            }
            self.fid_mut(fid)?.listing = Some(listing);
        }
        let listing = self.fid(fid)?.listing.as_ref().expect("listed");
        let limit = count.min(self.msize as usize - 11);
        let mut data = Out(Vec::new());
        for (index, (q, dtype, name)) in listing.iter().enumerate().skip(offset as usize) {
            let size = 13 + 8 + 1 + 2 + name.len();
            if data.0.len() + size > limit {
                break;
            }
            data.qid(*q).u64(index as u64 + 1).u8(*dtype).str(name);
        }
        let mut out = Out::new(41, tag);
        out.u32(data.0.len() as u32).bytes(&data.0);
        Ok(out.finish())
    }
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
        let mut s = Server::new(Beneath::open(dir).unwrap());
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

    #[test]
    fn logical_paths_do_not_climb_out() {
        assert_eq!(components("a/./b/../c").unwrap(), ["a", "c"]);
        assert!(components("../x").is_err());
        assert!(!valid_name("a/b") && !valid_name("..") && valid_name("a b"));
    }
}
