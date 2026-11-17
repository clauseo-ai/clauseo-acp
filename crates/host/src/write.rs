//! The host's file system writes: the one module that may call std's write functions.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's one place that creates folders (the data folder, the matters root and \
              agents' homes), writes an agent home's config.toml, appends to and rotates the log \
              file, and writes unit tests' temp folders"
)]

use std::io;
use std::path::Path;

/// Creates `path` and every missing parent; an existing folder is fine.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Creates the one folder `path`, private to the user on Unix (0700). Its parent must exist; a
/// link or anything else already at `path` fails with `AlreadyExists`, never followed.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        let mut builder = builder;
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    };
    builder.create(path)
}

/// Creates `target` and every missing folder between `base` and it, each new one private to
/// the user on Unix (0700), through no link: each part below `base` must be a real folder, or be
/// made as one (`symlink_metadata`, which never follows a link, and on Windows counts a
/// junction as a link). A link, or anything not a folder, below `base` fails with
/// `InvalidInput`; `base` itself may be a link. A session's run folder and a matter's notes
/// folder (`actor/turn/launch_checks.rs`).
pub(crate) fn create_private_dirs_under(base: &Path, target: &Path) -> io::Result<()> {
    let below = target
        .strip_prefix(base)
        .map_err(|_outside| io::Error::new(io::ErrorKind::InvalidInput, "not under its base"))?;
    let mut at = base.to_owned();
    for part in below.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            // Another launch may make it first: `AlreadyExists` is fine, and the check below
            // still refuses a link or a file there.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match create_private_dir(&at) {
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    made => made?,
                }
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        if !is_real_dir(&at) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is a link or not a folder", at.display()),
            ));
        }
    }
    Ok(())
}

/// Whether `path` is a folder itself, not a link to one (nor, on Windows, a junction).
pub(crate) fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Renames the folder `from` to `to` in one call: the start-up sweep's `run` set aside
/// (`actor/turn/launch_checks.rs`).
pub(crate) fn rename_folder(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Opens the file `path` for appending, creating it (0600 on Unix): the host's log file
/// (`log_file.rs`, decision record 0047).
pub(crate) fn open_append(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// The identity of the guard's record file (`pre_write_guard/record.rs`), read from the open
/// file the host made ([`create_record`]): its device (`st_dev`) and inode (`st_ino`) on Unix,
/// its volume's serial number and file ID on Windows (`network_paths::record_identity`). It goes
/// in Codex's trust-hashed hook command beside the path, as [`Self::hex`], and the guard and the
/// host's read write or read the file only when the file they opened has it: a file moved in at
/// the record's place, or reached through a link at one of its folders, has another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordId {
    /// The device, or the volume's serial number.
    volume: u64,
    /// The inode, or the file ID (128 bits, or the 64-bit file index).
    file: u128,
}

impl RecordId {
    /// How many hex digits [`Self::hex`] has: 16 for the volume, 32 for the file.
    pub(crate) const HEX_LEN: usize = 48;

    /// The identity as [`Self::HEX_LEN`] lowercase hex digits, the volume's then the file's.
    pub(crate) fn hex(self) -> String {
        format!("{:016x}{:032x}", self.volume, self.file)
    }

    /// The identity [`Self::hex`] wrote; `None` for anything else.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        if text.len() != Self::HEX_LEN || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self {
            volume: u64::from_str_radix(text.get(..16)?, 16).ok()?,
            file: u128::from_str_radix(text.get(16..)?, 16).ok()?,
        })
    }

    /// The identity of the open `file`.
    pub(crate) fn of(file: &std::fs::File) -> io::Result<Self> {
        #[cfg(unix)]
        let (volume, file) = {
            use std::os::unix::fs::MetadataExt as _;
            let meta = file.metadata()?;
            (meta.dev(), u128::from(meta.ino()))
        };
        #[cfg(windows)]
        let (volume, file) = crate::network_paths::record_identity(file)?;
        Ok(Self { volume, file })
    }
}

/// Opens the guard's record of its refusals (`pre_write_guard/record.rs`) for appending: the
/// file the host made empty at the launch ([`create_record`]), never created here, never through
/// a link at its last component and never waiting on a pipe (`O_NOFOLLOW | O_NONBLOCK` on Unix,
/// `FILE_FLAG_OPEN_REPARSE_POINT` on Windows). Anything but a regular file with one link and the
/// identity `id` fails ([`single`]), so a hard link planted at its place (to a shell profile,
/// say), a file moved in there, or one reached through a link at its folder is never written.
pub(crate) fn open_append_record(path: &Path, id: RecordId) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true);
    no_follow_no_wait(&mut options);
    single(options.open(path)?, id)
}

/// Makes the guard's record file `path` empty (0600 on Unix), when nothing is there yet:
/// anything already there, a link included, fails with `AlreadyExists` and is never followed.
/// Its identity, read from the file just made. The launch prep's, in a folder of the session's
/// own that only the host makes (`pre_write_guard::make_record`).
pub(crate) fn create_record(path: &Path) -> io::Result<RecordId> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    RecordId::of(&options.open(path)?)
}

/// `O_NOFOLLOW | O_NONBLOCK` on Unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows.
fn no_follow_no_wait(options: &mut std::fs::OpenOptions) {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        options,
        nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK,
    );
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(
        options,
        windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0,
    );
}

/// `file` when it is a regular file with one link (`st_nlink`, or `NumberOfLinks` on
/// Windows) and the identity `id`, all read from the open file, else an error.
fn single(file: std::fs::File, id: RecordId) -> io::Result<std::fs::File> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    #[cfg(unix)]
    let links = std::os::unix::fs::MetadataExt::nlink(&file.metadata()?);
    #[cfg(windows)]
    let links = u64::from(crate::network_paths::link_count(&file)?);
    if links != 1 {
        return Err(io::Error::other("a file with more than one link"));
    }
    if RecordId::of(&file)? != id {
        return Err(io::Error::other("not the record file the host made"));
    }
    Ok(file)
}

/// Renames the file `from` to `to`, replacing a file at `to`: the log file's rotation, and a
/// rejected `allowed.json` set aside (`blocklist/allowed/integrity.rs`).
pub(crate) fn rename_file(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Moves the file `from` to `to`, failing (`AlreadyExists`) when `to` exists, never replacing
/// it: a hard link, then `from` removed, then the folder synced. A rejected `allowed.json` set
/// aside (`blocklist/allowed/integrity.rs`). A crash between leaves both names, which is safe.
/// Where the folder takes no hard link (FAT, exFAT, some network shares: any failure but
/// `AlreadyExists` or `NotFound`), it checks that `to` is free, then renames: there alone a
/// file made at `to` between the two would be replaced.
pub(crate) fn move_file_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    move_no_replace_with(from, to, |from, to| std::fs::hard_link(from, to))
}

/// [`move_file_no_replace`] with `link` making the hard link (a test's seam).
pub(crate) fn move_no_replace_with(
    from: &Path,
    to: &Path,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    match link(from, to) {
        Ok(()) => std::fs::remove_file(from)?,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound
            ) =>
        {
            return Err(error);
        }
        Err(_no_link) => {
            if to.symlink_metadata().is_ok() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            std::fs::rename(from, to)?;
        }
    }
    sync_dir(to.parent().unwrap_or(to))
}

/// Removes the file `path`; a missing one is fine. An old set-aside `allowed.json`, a launch's
/// Claude settings file.
pub(crate) fn remove_old_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Removes the file `path` without following a link: `symlink_metadata` first, which never
/// follows one, then the name itself (a link at it goes, never what it points at). A folder at
/// the name is left, with an error; a missing name is fine. A session's proxy password file
/// (`actor/turn/launch_checks/egress.rs`).
pub(crate) fn remove_file_no_follow(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.is_dir() => Err(io::Error::other("a folder is at the file's name")),
        Ok(_file_or_link) => remove_old_file(path),
    }
}

/// Removes the folder `path` and everything under it; a missing folder is fine. A link inside
/// is removed, never followed (`std::fs::remove_dir_all`): a session's run folder.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The new file `path`, opened to write; anything already there, a link included, fails with
/// `AlreadyExists` and is never followed. A Word blackline's copies (`matter_worker/sides.rs`).
pub(crate) fn create_new_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Removes the link or file `path` itself, never what a link points to: a leftover download's
/// working entry that is not a real folder (`browser/install/archive.rs`). On Windows a link to
/// a folder (or a junction) is removed as a folder, which removes the link only.
pub(crate) fn remove_link(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if cfg!(windows) && std::fs::symlink_metadata(path).is_ok() => {
            std::fs::remove_dir(path).map_err(|_dir| error)
        }
        other => other,
    }
}

/// Sets the folder `path` to 0700 when it is not already (a link is left as it is).
#[cfg(unix)]
pub(crate) fn make_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() && meta.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Replaces the file `path` with `bytes` atomically: a fresh temp file beside it (0600 on Unix),
/// written and synced, then renamed over `path`. A link at `path` is replaced, never followed.
/// The temp file's name holds the process id and a count, so two writes (or two hosts sharing a
/// data folder) never share one.
pub(crate) fn replace_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    place_private_file(path, bytes, |temp, path| std::fs::rename(temp, path))
}

/// Writes `bytes` to the new file `path` atomically: a fresh temp file beside it (0600 on Unix,
/// named as [`replace_private_file`] names it), written and synced, then moved to `path`
/// without replacing anything there ([`move_file_no_replace`]): a file or link already at
/// `path` fails with `AlreadyExists`, and the temp file is removed. A launch's Claude settings
/// file (`agent/launch_settings.rs`).
pub(crate) fn write_new_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    place_private_file(path, bytes, move_file_no_replace)
}

/// `bytes` written to a fresh private temp file beside `path`, then `place` moving it there;
/// the temp file is removed when either fails.
fn place_private_file(
    path: &Path,
    bytes: &[u8],
    place: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut temp = path.as_os_str().to_owned();
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    temp.push(format!(".clauseo-{}-{count}.new", std::process::id()));
    let temp = std::path::PathBuf::from(temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let renamed = written.and_then(|()| place(&temp, path));
    if renamed.is_err() {
        // What is left of the temp file is ours; the error that matters is the write's.
        drop(std::fs::remove_file(&temp));
    }
    renamed
}

/// Syncs the folder `path`, so a rename into it survives a power cut (Unix: `fsync` of the
/// folder). On Windows a folder cannot be opened this way, and NTFS journals the rename itself.
#[cfg_attr(
    windows,
    expect(
        clippy::unnecessary_wraps,
        reason = "one signature on every OS; only Unix can fail, Windows has nothing to sync"
    )
)]
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::fs::File::open(path)?.sync_all();
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Removes the temp files a crashed [`replace_private_file`] of `path` left beside it
/// (`<name>.clauseo-*.new`), best effort: one that cannot be removed stays, and the write goes
/// on with its own fresh name.
pub(crate) fn sweep_temp_files(path: &Path) {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let mut prefix = name.to_owned();
    prefix.push(".clauseo-");
    let prefix = prefix.as_encoded_bytes();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        if bytes.starts_with(prefix) && bytes.ends_with(b".new") {
            drop(std::fs::remove_file(entry.path()));
        }
    }
}

/// Removes `path` and everything under it; a missing folder is fine. For unit tests' temp folders.
#[cfg(test)]
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Writes `bytes` to the file `path`, replacing it. For unit tests' fixtures.
#[cfg(test)]
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// Renames `from` to `to`. For unit tests: the lawyer moving a folder.
#[cfg(test)]
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Removes the file `path`. For unit tests.
#[cfg(test)]
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Sets `path`'s permission bits to `mode`. For unit tests: a folder the user may not search.
#[cfg(all(test, unix))]
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Opens `path` for reading, never through a link at its last component: `O_NOFOLLOW` on Unix
/// (the open fails on a link), and on Windows `FILE_FLAG_OPEN_REPARSE_POINT`, which opens the
/// link itself, not what it points at. A read changes nothing; it lives here for the flags.
pub(crate) fn open_read_nofollow(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, nix::libc::O_NOFOLLOW);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(
        &mut options,
        windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0,
    );
    options.open(path)
}

/// [`open_read_nofollow`] for a regular file with one link and the identity `id` only, never
/// waiting on a pipe (`O_NONBLOCK` on Unix): the host's read of the guard's record
/// (`actor/turn/updates/guard_blocks.rs`).
pub(crate) fn open_read_record(path: &Path, id: RecordId) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    no_follow_no_wait(&mut options);
    single(options.open(path)?, id)
}

/// Opens `path` for reading with a share mode that lets others read but not write or delete
/// (`FILE_SHARE_READ`), as Word holds a document. For the pre-write guard's Windows test.
#[cfg(all(test, windows))]
pub(crate) fn open_deny_write(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`.
    const SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(path)
}

/// Makes `link` a symbolic link to `target`. For unit tests: a matter stored as a link.
#[cfg(all(test, unix))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Makes `link` a symbolic link to the folder `target`; fails without the right to make links
/// (Developer Mode or an elevated token). For unit tests.
#[cfg(all(test, windows))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// Makes `link` a link to the folder `target` that a test of a link never followed can use as
/// any user: a symbolic link, or on Windows, where a standard user may not make one (OS error
/// 1314 without Developer Mode), a directory junction, which any user may make.
#[cfg(test)]
pub(crate) fn folder_link(target: &Path, link: &Path) -> io::Result<()> {
    let made = symlink(target, link);
    #[cfg(windows)]
    if made
        .as_ref()
        .is_err_and(|error| error.raw_os_error() == Some(1314))
    {
        let target = dunce::canonicalize(target)?;
        return clauseo_sandbox::windows::test_junction::junction(link, &target);
    }
    made
}

/// Makes `link` a symbolic link to the file `target`; fails without the right to make links
/// (Developer Mode or an elevated token). For unit tests: a link at the guard's record.
#[cfg(all(test, windows))]
pub(crate) fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

/// Makes `link` a second name of the file `target`. For unit tests: a hard link at the guard's
/// record.
#[cfg(test)]
pub(crate) fn hard_link(target: &Path, link: &Path) -> io::Result<()> {
    std::fs::hard_link(target, link)
}
