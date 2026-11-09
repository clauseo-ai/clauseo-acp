//! Unix only: the `--listen` socket's folder, opened and checked one component at a time from
//! `/`, by descriptor. Each component is opened relative to the one before with
//! `O_NOFOLLOW|O_DIRECTORY|O_CLOEXEC`, so a link anywhere on the way fails the open, and each is
//! checked with `fstat` on what was opened, never by its path again: every folder on the way
//! must be owned by root or the guard's user and writable by no one else unless sticky (as
//! `/tmp`), and the last one, the socket's own folder, must have mode exactly 0700 and be the
//! guard's user's. The guard then binds and later unlinks the socket relative to that last
//! descriptor (`write.rs`), so nothing done to the path after the walk moves either. On macOS
//! `/tmp` is a link to `/private/tmp`, so the host passes the `/private/tmp/...` path.

use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use nix::errno::Errno;
use nix::fcntl::{OFlag, open, openat};
use nix::sys::stat::Mode;

/// How each component is opened.
const FLAGS: OFlag = OFlag::O_RDONLY
    .union(OFlag::O_DIRECTORY)
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_CLOEXEC);

/// Opens and checks the folder of `socket`, an absolute path of plain components. Returns the
/// folder's descriptor and the socket's name in it. `Err` with the reason, which names no path.
pub(crate) fn open_checked(socket: &Path) -> Result<(OwnedFd, OsString), String> {
    let own = nix::unistd::geteuid().as_raw();
    let mut components = socket.components();
    let (Some(Component::RootDir), Some(Component::Normal(name))) =
        (components.next(), components.next_back())
    else {
        return Err("the --listen socket must be an absolute path inside a folder".to_owned());
    };
    let mut folder = open("/", FLAGS, Mode::empty()).map_err(cannot)?;
    let mut steps = components.peekable();
    check(&folder, steps.peek().is_none(), own)?;
    while let Some(step) = steps.next() {
        let Component::Normal(step) = step else {
            return Err("the --listen socket's path must hold no `.` or `..`".to_owned());
        };
        let opened = openat(&folder, step, FLAGS, Mode::empty());
        folder = opened.map_err(|errno| match errno {
            // Linux reports a link opened with `O_DIRECTORY|O_NOFOLLOW` as not a folder.
            Errno::ELOOP | Errno::ENOTDIR if is_link(&folder, step) => {
                "the --listen socket's path goes through a link".to_owned()
            }
            Errno::ENOTDIR => "the --listen socket's path goes through a file".to_owned(),
            errno => cannot(errno),
        })?;
        check(&folder, steps.peek().is_none(), own)?;
    }
    Ok((folder, name.to_owned()))
}

/// Checks the folder `folder` holds: the socket's own (`last`) or one on the way to it.
fn check(folder: &OwnedFd, last: bool, own: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    // `fstat` on the descriptor, through a second handle on it.
    let stat = folder
        .try_clone()
        .map(std::fs::File::from)
        .and_then(|file| file.metadata())
        .map_err(|error| format!("cannot read the --listen socket's folder: {error}"))?;
    let mode = stat.mode() & 0o7777;
    let refusal = if last {
        folder_refusal(mode, stat.uid(), own)
    } else {
        ancestor_refusal(mode, stat.uid(), own)
    };
    refusal.map_or(Ok(()), Err)
}

/// Why the socket's own folder, with this mode and owner, may not hold it; `None` if it may.
pub(crate) fn folder_refusal(mode: u32, uid: u32, own: u32) -> Option<String> {
    if mode != 0o700 {
        return Some(format!(
            "the --listen socket's folder: its mode is {mode:o}, not 700"
        ));
    }
    (uid != own).then(|| {
        format!("the --listen socket's folder: its owner is uid {uid}, not the guard's uid {own}")
    })
}

/// Why a folder on the way to the socket's, with this mode and owner, may not be; `None` if it
/// may: owned by root or the guard's user, and writable by no one else unless sticky.
pub(crate) fn ancestor_refusal(mode: u32, uid: u32, own: u32) -> Option<String> {
    if uid != 0 && uid != own {
        return Some(format!(
            "a folder on the --listen socket's path is owned by uid {uid}, not root or the \
             guard's uid {own}"
        ));
    }
    (mode & 0o022 != 0 && mode & 0o1000 == 0).then(|| {
        format!("a folder on the --listen socket's path has mode {mode:o}: others may write to it")
    })
}

/// Whether `name` in `folder` is a link, read without following it.
fn is_link(folder: &OwnedFd, name: &std::ffi::OsStr) -> bool {
    nix::sys::stat::fstatat(folder, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW).is_ok_and(
        |stat| {
            use nix::sys::stat::SFlag;
            SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT == SFlag::S_IFLNK
        },
    )
}

/// The reason for an `open` the kernel refused.
fn cannot(errno: Errno) -> String {
    format!("cannot open the --listen socket's folder: {errno}")
}
