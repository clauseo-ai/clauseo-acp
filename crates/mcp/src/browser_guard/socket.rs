//! Unix only: the folder agent-browser's socket goes in. A Unix socket path holds at most 103
//! bytes (`connection.rs:902-913` refuses longer), and `<cwd>/run/...` can exceed that, so the
//! socket folder is `/tmp/clauseo-ab-<first 16 characters of the session>` instead
//! (`env::socket_dir`). Other users can write to `/tmp`, so the guard makes the folder itself
//! with mode 0700, and refuses one that already exists unless it is a real folder (not a link)
//! with mode 0700 and the same owner as `--cwd`. Then it checks `<folder>/<session>.sock` fits.
//! Windows keeps `<cwd>\run`, which holds only `.port` files.

use std::path::Path;

/// The longest socket path agent-browser accepts, in bytes (`connection.rs:902-913`).
pub(crate) const MAX_SOCKET_PATH_BYTES: usize = 103;

/// What the guard reads about an existing socket folder, from `symlink_metadata`.
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

/// `Err` with the reason when an existing socket folder is not one the guard may use.
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

/// Makes the socket folder (mode 0700) or checks the one there, then checks the socket path's
/// length. `Err` with the reason.
#[cfg(unix)]
pub(crate) fn prepare(dir: &Path, cwd: &Path, session: &str) -> Result<(), String> {
    use std::io::ErrorKind;
    use std::os::unix::fs::MetadataExt;

    let cwd_uid = std::fs::metadata(cwd)
        .map_err(|error| format!("cannot read --cwd: {error}"))?
        .uid();
    match crate::write::create_private_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("cannot make {}: {error}", dir.display())),
    }
    let metadata = std::fs::symlink_metadata(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
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
        // `/tmp/clauseo-ab-` and 16 characters, `/`, 64, `.sock`: 102 bytes.
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
