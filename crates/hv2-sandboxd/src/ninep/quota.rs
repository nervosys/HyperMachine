//! How much a guest may grow a volume.
//!
//! A volume is a host directory the guest writes into, so without a limit a
//! guest fills the host's disk -- and every other sandbox's, and the node's
//! own state with it. Two limits stand between them, both enforced before the
//! host operation that would grow storage (a write past the end of a file, a
//! create, `mkdir`, `symlink`, `link`, a truncate to a larger size) and both
//! answered with an errno the guest's kernel already understands:
//!
//! * a **host-disk guard**, on by default: the operation is refused with
//!   `ENOSPC` when it would leave the volume's filesystem with less than a
//!   reserve free -- 5% of the filesystem or 2 GiB, whichever is smaller, and
//!   1% of its inodes. `statvfs` is not a syscall per write: its answer is
//!   kept for a second, or for [`PROBE_EVERY_BYTES`] of admitted growth, and
//!   what was admitted since is subtracted from it in the meantime.
//! * a **per-volume quota**, off by default: at most `max_bytes` of file
//!   contents and `max_inodes` entries, refused with `EDQUOT`. Usage is
//!   counted once when the volume is first used (a walk of its tree) and then
//!   kept as a counter that every create, write, truncate and unlink moves.
//!   Growth is charged *before* it is done, so the limit is never passed, not
//!   even by one write; what is approximate is the other direction (see
//!   [`Budget::admit`]).
//!
//! One [`Budget`] belongs to one volume directory and is shared by every
//! mount of it and by the content API, so they spend the same allowance.
//! Sizes are apparent sizes (`st_size`): a sparse file counts what it claims,
//! which is the safe way to be wrong.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::io::{Error, Result};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use super::{fstat, is_dir, is_symlink, proc_path, Beneath};

/// The default reserve, as a percentage of the filesystem.
pub const DEFAULT_RESERVE_PERCENT: u64 = 5;

/// The reserve never needs to be more than this, however large the disk.
const RESERVE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The share of the filesystem's inodes kept free, when the guard is on.
const INODE_RESERVE_PERCENT: u64 = 1;

/// How long a `statvfs` answer is believed.
const PROBE_TTL: Duration = Duration::from_secs(1);

/// How much growth is admitted on one answer before asking again.
const PROBE_EVERY_BYTES: u64 = 64 * 1024 * 1024;

/// What a node lets volumes spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Keep this percentage of the filesystem free (capped at 2 GiB); 0
    /// turns the host-disk guard off.
    pub reserve_percent: u64,
    /// The most bytes of contents one volume may hold; 0 is no limit.
    pub max_bytes: u64,
    /// The most entries one volume may hold; 0 is no limit.
    pub max_inodes: u64,
}

impl Policy {
    pub const DEFAULT: Policy = Policy {
        reserve_percent: DEFAULT_RESERVE_PERCENT,
        max_bytes: 0,
        max_inodes: 0,
    };

    fn counted(&self) -> bool {
        self.max_bytes > 0 || self.max_inodes > 0
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a filesystem has left.
#[derive(Debug, Clone, Copy)]
pub struct Space {
    pub avail_bytes: u64,
    pub total_bytes: u64,
    pub avail_inodes: u64,
    pub total_inodes: u64,
}

/// Where the free space comes from; a closure so a test can say the disk is
/// full without filling one.
pub type Probe = Box<dyn Fn() -> Result<Space> + Send + Sync>;

/// What one operation would add to a volume.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cost {
    /// Apparent bytes, against the volume's byte quota.
    pub size: u64,
    /// Bytes the host will really have to find, against the disk's reserve
    /// (less than `size` for a write past a hole, none for a sparse truncate,
    /// and more for a write into what that made).
    pub disk: u64,
    /// Entries, against both the inode quota and the disk's inode reserve.
    pub inodes: u64,
}

#[derive(Default)]
struct State {
    used_bytes: u64,
    used_inodes: u64,
    probed: Option<(Instant, Space)>,
    /// Growth admitted since `probed`, taken off its answer.
    bytes_since: u64,
    inodes_since: u64,
}

/// One volume's allowance.
pub struct Budget {
    policy: Policy,
    probe: Probe,
    ttl: Duration,
    every_bytes: u64,
    state: Mutex<State>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Budget {
    /// A budget with nothing counted yet: the quota starts from empty.
    pub fn new(policy: Policy, probe: Probe, ttl: Duration) -> Self {
        Self {
            policy,
            probe,
            ttl,
            every_bytes: PROBE_EVERY_BYTES,
            state: Mutex::new(State::default()),
        }
    }

    /// A budget for the volume at `dir`, with what is already in it counted
    /// when a quota is set.
    pub fn scanned(policy: Policy, probe: Probe, ttl: Duration, dir: &Path) -> Self {
        let budget = Self::new(policy, probe, ttl);
        if policy.counted() {
            if let Ok(fs) = Beneath::open(dir) {
                let (bytes, inodes) = scan(&fs);
                let mut st = lock(&budget.state);
                st.used_bytes = bytes;
                st.used_inodes = inodes;
            }
        }
        budget
    }

    /// `(bytes, inodes)` counted against the quota now.
    #[cfg(test)]
    pub fn used(&self) -> (u64, u64) {
        let st = lock(&self.state);
        (st.used_bytes, st.used_inodes)
    }

    /// Take `cost` from the volume's allowance, or refuse it: `EDQUOT` if a
    /// quota would pass, `ENOSPC` if the host disk would be left under its
    /// reserve. Nothing is taken on a refusal. The caller does the operation
    /// afterwards and calls [`Budget::release`] with the same cost if it
    /// fails.
    ///
    /// The counter errs toward refusing: space freed behind its back (a file
    /// deleted while a guest still holds it open, an inode shared by a hard
    /// link) is given back late or not at all, so a guest can be refused with
    /// room left but not admitted past the limit.
    pub fn admit(&self, cost: Cost) -> Result<()> {
        let mut st = lock(&self.state);
        if self.policy.max_bytes > 0
            && st.used_bytes.saturating_add(cost.size) > self.policy.max_bytes
        {
            return Err(Error::from_raw_os_error(libc::EDQUOT));
        }
        if self.policy.max_inodes > 0
            && st.used_inodes.saturating_add(cost.inodes) > self.policy.max_inodes
        {
            return Err(Error::from_raw_os_error(libc::EDQUOT));
        }
        if self.policy.reserve_percent > 0 && (cost.disk > 0 || cost.inodes > 0) {
            self.check_host(&mut st, cost)?;
        }
        st.used_bytes = st.used_bytes.saturating_add(cost.size);
        st.used_inodes = st.used_inodes.saturating_add(cost.inodes);
        st.bytes_since = st.bytes_since.saturating_add(cost.disk);
        st.inodes_since = st.inodes_since.saturating_add(cost.inodes);
        Ok(())
    }

    fn check_host(&self, st: &mut State, cost: Cost) -> Result<()> {
        let stale = match st.probed {
            None => true,
            Some((at, _)) => at.elapsed() >= self.ttl || st.bytes_since >= self.every_bytes,
        };
        if stale {
            // A disk that cannot be asked is not a reason to refuse a guest.
            if let Ok(space) = (self.probe)() {
                st.probed = Some((Instant::now(), space));
                st.bytes_since = 0;
                st.inodes_since = 0;
            }
        }
        let Some((_, space)) = st.probed else {
            return Ok(());
        };
        let pct = u128::from(self.policy.reserve_percent.min(100));
        let reserve = u64::try_from(u128::from(space.total_bytes) * pct / 100)
            .unwrap_or(u64::MAX)
            .min(RESERVE_CAP_BYTES);
        let free = space.avail_bytes.saturating_sub(st.bytes_since);
        if free.saturating_sub(cost.disk) < reserve {
            return Err(Error::from_raw_os_error(libc::ENOSPC));
        }
        // Some filesystems report no inode count at all.
        if space.total_inodes > 0 && cost.inodes > 0 {
            let reserve = space.total_inodes * INODE_RESERVE_PERCENT / 100;
            let free = space.avail_inodes.saturating_sub(st.inodes_since);
            if free.saturating_sub(cost.inodes) < reserve {
                return Err(Error::from_raw_os_error(libc::ENOSPC));
            }
        }
        Ok(())
    }

    /// Give back what [`Budget::admit`] took, or what was freed.
    pub fn release(&self, cost: Cost) {
        let mut st = lock(&self.state);
        st.used_bytes = st.used_bytes.saturating_sub(cost.size);
        st.used_inodes = st.used_inodes.saturating_sub(cost.inodes);
        if cost.size > 0 {
            // Freed space is real: believe the disk again, not an old answer.
            st.probed = None;
            st.bytes_since = 0;
            st.inodes_since = 0;
        }
    }

    /// An entry was removed (`stat` is from just before): give back its
    /// share. A file with other hard links still holds its bytes.
    pub fn removed(&self, st: &libc::stat) {
        if is_dir(st) {
            self.release(Cost {
                inodes: 1,
                ..Cost::default()
            });
        } else if st.st_nlink <= 1 {
            let size = if is_symlink(st) || st.st_mode & libc::S_IFMT == libc::S_IFREG {
                u64::try_from(st.st_size).unwrap_or(0)
            } else {
                0
            };
            self.release(Cost {
                size,
                disk: size,
                inodes: 1,
            });
        }
    }
}

/// The bytes and entries beneath `fs`'s root, counting each hard-linked inode
/// once. Every directory is opened beneath the root with no symlink followed
/// and every entry is `fstatat`ed through that descriptor, so a guest cannot
/// steer the walk out of the volume.
fn scan(fs: &Beneath) -> (u64, u64) {
    let (mut bytes, mut inodes) = (0u64, 0u64);
    let mut linked: HashSet<(u64, u64)> = HashSet::new();
    let mut work: Vec<Vec<String>> = vec![Vec::new()];
    while let Some(dir) = work.pop() {
        let Ok(fd) = fs.dir(&dir) else { continue };
        let path = proc_path(fd.as_raw_fd());
        let Ok(names) = std::fs::read_dir(std::ffi::OsStr::from_bytes(path.to_bytes())) else {
            continue;
        };
        for name in names.flatten() {
            let Ok(c) = CString::new(name.file_name().as_bytes()) else {
                continue;
            };
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::fstatat(
                    fd.as_raw_fd(),
                    c.as_ptr(),
                    &mut st,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } < 0
            {
                continue;
            }
            if is_dir(&st) {
                inodes += 1;
                let mut child = dir.clone();
                child.push(name.file_name().to_string_lossy().into_owned());
                work.push(child);
                continue;
            }
            if st.st_nlink > 1 && !linked.insert((st.st_dev, st.st_ino)) {
                continue;
            }
            inodes += 1;
            if is_symlink(&st) || st.st_mode & libc::S_IFMT == libc::S_IFREG {
                bytes = bytes.saturating_add(u64::try_from(st.st_size).unwrap_or(0));
            }
        }
    }
    (bytes, inodes)
}

/// What `statvfs` says about the filesystem holding `dir`.
fn statvfs_probe(dir: PathBuf) -> Probe {
    Box::new(move || {
        let path = CString::new(dir.as_os_str().as_bytes())
            .map_err(|_| Error::from_raw_os_error(libc::EINVAL))?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(path.as_ptr(), &mut s) } < 0 {
            return Err(Error::last_os_error());
        }
        Ok(Space {
            avail_bytes: s.f_bavail.saturating_mul(s.f_frsize),
            total_bytes: s.f_blocks.saturating_mul(s.f_frsize),
            avail_inodes: s.f_favail,
            total_inodes: s.f_files,
        })
    })
}

static POLICY: Mutex<Policy> = Mutex::new(Policy::DEFAULT);

/// Change the node's policy; read when a volume's budget is first made, so
/// this is for startup, from the command line.
pub fn configure(change: impl FnOnce(&mut Policy)) {
    change(&mut lock(&POLICY));
}

type Registry = Mutex<HashMap<PathBuf, Arc<OnceLock<Arc<Budget>>>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// The budget of the volume at `dir`: the same one for every caller, made on
/// first use under the node's policy. The first call walks the volume when a
/// quota is set, so call it where blocking is fine.
pub fn budget_for(dir: &Path) -> Arc<Budget> {
    let slot = Arc::clone(lock(registry()).entry(dir.to_path_buf()).or_default());
    Arc::clone(slot.get_or_init(|| {
        let policy = *lock(&POLICY);
        Arc::new(Budget::scanned(
            policy,
            statvfs_probe(dir.to_path_buf()),
            PROBE_TTL,
            dir,
        ))
    }))
}

/// Drop the budget of a volume that is gone.
pub fn forget(dir: &Path) {
    lock(registry()).remove(dir);
}

/// The apparent size of the file behind `fd`.
pub fn size_of(fd: i32) -> Result<u64> {
    Ok(u64::try_from(fstat(fd)?.st_size).unwrap_or(0))
}

/// [`size_of`], and whether the file has holes: less allocated to it than its
/// size, so a write inside it can take space.
pub fn size_and_holes(fd: i32) -> Result<(u64, bool)> {
    let stat = fstat(fd)?;
    let size = u64::try_from(stat.st_size).unwrap_or(0);
    let allocated = u64::try_from(stat.st_blocks).unwrap_or(0);
    Ok((size, allocated.saturating_mul(512) < size))
}
