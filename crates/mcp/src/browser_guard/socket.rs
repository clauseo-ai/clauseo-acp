//! Unix only: the folder agent-browser's socket goes in. A Unix socket path holds at most 103
//! bytes (`connection.rs:902-913` refuses longer), and `<cwd>/run/...` can exceed that, so the
//! host makes a short folder in `/tmp` for each session (stage C's `BrowserSocketDir`,
//! `/tmp/clauseo-ab-<16 random>`, mode 0700) and passes it as `AGENT_BROWSER_SOCKET_DIR`. The
//! guard makes nothing there: it checks the folder it is given and refuses to start unless the
//! path is absolute, names a real folder under `lstat` (not a link), with mode exactly 0700 and
//! the same owner as `--cwd`, and `<folder>/<session>.sock` fits. Windows keeps `<cwd>\run`,
//! which holds only `.port` and `.pid` files.

use std::path::Path;

/// The longest socket path agent-browser accepts, in bytes (`connection.rs:902-913`).
pub(crate) const MAX_SOCKET_PATH_BYTES: usize = 103;

/// What the guard reads about the socket folder, from `symlink_metadata`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Facts {
    /// A link, whatever it points at.
    pub(crate) is_link: bool,
    /// A real folder.
    pub(crate) is_dir: bool,
    /// The permission bits, `st_mode & 0o7777`.
    pub(crate) mode: u32,
    /// The owner's uid.
    pub(crate) uid: u32,
}

/// `Err` with the reason when the socket folder is not one the guard may use.
pub(crate) fn check_facts(facts: Facts, cwd_uid: u32) -> Result<(), String> {
    if facts.is_link {
        return Err("it is a link".to_owned());
    }
    if !facts.is_dir {
        return Err("it is not a folder".to_owned());
    }
    if facts.mode != 0o700 {
        return Err(format!("its mode is {:o}, not 700", facts.mode));
    }
    if facts.uid != cwd_uid {
        return Err(format!(
            "its owner is uid {}, not uid {cwd_uid}, the owner of --cwd",
            facts.uid
        ));
    }
    Ok(())
}

/// `Err` with the reason when `<dir>/<session>.sock` is over [`MAX_SOCKET_PATH_BYTES`].
pub(crate) fn check_length(dir: &Path, session: &str) -> Result<(), String> {
    let path = dir.join(format!("{session}.sock"));
    let length = path.as_os_str().len();
    if length > MAX_SOCKET_PATH_BYTES {
        return Err(format!(
            "the socket path {} is {length} bytes, over the {MAX_SOCKET_PATH_BYTES} a Unix socket \
             allows",
            path.display()
        ));
    }
    Ok(())
}

/// Checks the socket folder the host gave: absolute, a real folder (not a link, read with
/// `symlink_metadata`), mode 0700, owned by `--cwd`'s owner, and `<dir>/<session>.sock`
/// within [`MAX_SOCKET_PATH_BYTES`]. Makes nothing. `Err` with the reason.
#[cfg(unix)]
pub(crate) fn check(dir: &Path, cwd: &Path, session: &str) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    if !dir.is_absolute() {
        return Err(format!(
            "the socket folder {} is not an absolute path",
            dir.display()
        ));
    }
    let cwd_uid = std::fs::metadata(cwd)
        .map_err(|error| format!("cannot read --cwd: {error}"))?
        .uid();
    let metadata = std::fs::symlink_metadata(dir).map_err(|error| {
        format!(
            "the socket folder {}: cannot read it: {error}",
            dir.display()
        )
    })?;
    let facts = Facts {
        is_link: metadata.file_type().is_symlink(),
        is_dir: metadata.is_dir(),
        mode: metadata.mode() & 0o7777,
        uid: metadata.uid(),
    };
    check_facts(facts, cwd_uid)
        .map_err(|reason| format!("the socket folder {}: {reason}", dir.display()))?;
    check_length(dir, session)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Facts, check_facts, check_length};

    const GOOD: Facts = Facts {
        is_link: false,
        is_dir: true,
        mode: 0o700,
        uid: 501,
    };

    #[test]
    fn only_a_private_real_folder_of_the_same_owner_passes() {
        assert_eq!(check_facts(GOOD, 501), Ok(()));
        let cases = [
            (
                Facts {
                    is_link: true,
                    ..GOOD
                },
                "it is a link",
            ),
            (
                Facts {
                    is_link: true,
                    is_dir: false,
                    ..GOOD
                },
                "it is a link",
            ),
            (
                Facts {
                    is_dir: false,
                    ..GOOD
                },
                "it is not a folder",
            ),
            (
                Facts {
                    mode: 0o755,
                    ..GOOD
                },
                "its mode is 755, not 700",
            ),
            (
                Facts {
                    mode: 0o1700,
                    ..GOOD
                },
                "its mode is 1700, not 700",
            ),
            (
                Facts {
                    mode: 0o600,
                    ..GOOD
                },
                "its mode is 600, not 700",
            ),
            (
                Facts { uid: 0, ..GOOD },
                "its owner is uid 0, not uid 501, the owner of --cwd",
            ),
        ];
        for (facts, reason) in cases {
            assert_eq!(check_facts(facts, 501), Err(reason.to_owned()), "{facts:?}");
        }
    }

    #[test]
    fn the_socket_path_must_fit_in_103_bytes() {
        let session = "x".repeat(64);
        // `/tmp/clauseo-ab-` and 16 characters (the host's folder), `/`, 64, `.sock`: 102 bytes.
        let dir = Path::new("/tmp/clauseo-ab-xxxxxxxxxxxxxxxx");
        assert_eq!(check_length(dir, &session), Ok(()));
        assert_eq!(
            check_length(Path::new("/tmp/clauseo-ab-xxxxxxxxxxxxxxxxy"), &session),
            Ok(())
        );
        let long = Path::new("/tmp/clauseo-ab-xxxxxxxxxxxxxxxxyz");
        let error = check_length(long, &session).unwrap_err();
        assert!(error.contains("is 104 bytes, over the 103"), "{error}");
    }
}
