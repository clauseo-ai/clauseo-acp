//! Whether the git watch's stream can watch the matter's volume. `FSEvents` reports nothing a
//! network volume's server or another machine changes, and an automounted volume can come and
//! go under the watch. A matter on such a volume is allowed only when no other path the agent
//! can write is on that volume: then nothing can be renamed into the matter from elsewhere,
//! and the scans before and after the session see all the agent left there.

use std::io;
use std::path::Path;

use super::super::build::refuse;
use super::Policy;
use crate::Error;

/// How the git watch watches the matter ([`Policy::matter_watch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatterWatch {
    /// A local volume: the live stream and its passes, and the scans before and after the
    /// session.
    #[default]
    Live,
    /// A volume the stream can't watch (a network or automounted volume, or one under `/net`
    /// or `/Network`) that holds no other path the agent writes: no live stream, only the
    /// scans before and after the session.
    AfterOnly,
}

/// What the build needs to know of a volume, read from an open folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Volume {
    /// The volume's device number, from the same open folder.
    pub(crate) id: u64,
    /// Whether the stream can watch it: local, not mounted by the automounter, not `autofs`,
    /// and not under `/net` or `/Network`.
    pub(crate) watchable: bool,
}

/// How the git watch can watch `policy`'s matter; refused when the matter's volume is one the
/// stream can't watch and another path the agent writes is on it, or a volume can't be read.
pub(super) fn matter_watch(policy: &Policy) -> Result<MatterWatch, Error> {
    let read = |path: &Path| {
        volume_of(path).or_else(|e| refuse(format!("{} can't be read: {e}", path.display())))
    };
    let matter = read(policy.matter())?;
    if matter.watchable {
        return Ok(MatterWatch::Live);
    }
    for path in writable(policy) {
        if read(path)?.id == matter.id {
            return refuse(format!(
                "the matter is on a network or automounted volume, which the git watch can't \
                 watch as it changes, and {}, which the agent can also write, is on the same \
                 volume",
                path.display()
            ));
        }
    }
    tracing::info!(
        matter = %policy.matter().display(),
        "the matter is on a volume the git watch can't watch live; it scans before and after"
    );
    Ok(MatterWatch::AfterOnly)
}

/// Every path the agent can write but the matter: the profile's read-write allows.
fn writable(policy: &Policy) -> Vec<&Path> {
    let mut out = vec![policy.state()];
    out.extend(policy.session_tmpdir());
    out.extend(policy.notes());
    out.extend(policy.credential_link());
    out.extend(policy.claude_store().map(|store| store.dir.as_path()));
    out.extend(policy.browser_socket_dir());
    out
}

/// The volume of `path`, read from an open folder: `path`'s own, else that of the nearest
/// folder above it (a file, a link or a missing name is on its folder's volume). Opened
/// without following a link at the end.
fn volume_of(path: &Path) -> io::Result<Volume> {
    #[cfg(test)]
    if let Some(volume) = faked(path) {
        return Ok(volume);
    }
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for dir in path.ancestors() {
        match read(dir) {
            Ok(volume) => return Ok(volume),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// The volume of folder `dir`, from `fstat` and `fstatfs` on one descriptor of it.
#[cfg(target_os = "macos")]
fn read(dir: &Path) -> io::Result<Volume> {
    use rustix::fs::{Mode, OFlags, fstat, fstatfs, open};
    /// `statfs`'s flags (`<sys/mount.h>`): a volume on this Mac, and one the automounter made.
    const MNT_LOCAL: u32 = 0x0000_1000;
    const MNT_AUTOMOUNTED: u32 = 0x0040_0000;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = open(dir, flags, Mode::empty())?;
    let id = fstat(&fd)?.st_dev;
    let stat = fstatfs(&fd)?;
    let kind: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.to_ne_bytes()[0])
        .collect();
    let mounted_far = [Path::new("/net"), Path::new("/Network")]
        .iter()
        .any(|far| super::under(dir, far));
    let watchable = stat.f_flags & MNT_LOCAL != 0
        && stat.f_flags & MNT_AUTOMOUNTED == 0
        && kind != b"autofs"
        && !mounted_far;
    Ok(Volume {
        id: u64::try_from(id).unwrap_or_default(),
        watchable,
    })
}

/// Elsewhere there is no git watch: every existing folder's volume counts as watchable.
#[cfg(not(target_os = "macos"))]
fn read(dir: &Path) -> io::Result<Volume> {
    std::fs::metadata(dir).map(|_| Volume {
        id: 0,
        watchable: true,
    })
}

#[cfg(test)]
std::thread_local! {
    /// The tests' volumes, by folder: what [`volume_of`] reads at and below each.
    static FAKED: std::cell::RefCell<Vec<(std::path::PathBuf, Volume)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Makes [`volume_of`] read `volume` at and below `dir`, on this thread.
#[cfg(test)]
pub(crate) fn fake_volume(dir: &Path, volume: Volume) {
    FAKED.with(|faked| faked.borrow_mut().push((dir.to_owned(), volume)));
}

/// The faked volume of `path`: the deepest folder faked at or above it.
#[cfg(test)]
fn faked(path: &Path) -> Option<Volume> {
    FAKED.with(|faked| {
        faked
            .borrow()
            .iter()
            .filter(|(dir, _)| path.starts_with(dir))
            .max_by_key(|(dir, _)| dir.components().count())
            .map(|(_, volume)| *volume)
    })
}
