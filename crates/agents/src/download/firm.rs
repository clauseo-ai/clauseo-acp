//! The firm's two ways around the public registry: a mirror base for tarball URLs, and an
//! offline bundle folder read instead of the network.

use std::io;
use std::path::{Path, PathBuf};

use super::{
    FetchError, FetchJob, FetchKind, Place, Source, complete_size, hash_read, host_of, part_path,
    write_checked,
};
use crate::cancel::Cancel;
use crate::integrity::Expected;
use crate::log::redact_url;
use crate::write;

/// `resolved` with its registry base (`registry`: the snapshot's, [`crate::NPM_REGISTRY`] outside
/// tests) replaced by `mirror`'s base (one trailing `/` either way). A URL off the registry
/// is refused rather than sent elsewhere.
pub(crate) fn mirror_url(
    resolved: &str,
    registry: &str,
    mirror: &str,
    data_dir: &Path,
) -> Result<String, FetchError> {
    let refuse = |detail: &str| {
        FetchError::new(
            FetchKind::Failed,
            host_of(mirror).as_deref(),
            None,
            detail,
            data_dir,
        )
    };
    let Some(path) = resolved.strip_prefix(registry) else {
        return Err(refuse(&format!(
            "the package URL {} is not on {}, so the mirror cannot serve it",
            redact_url(resolved),
            redact_url(registry)
        )));
    };
    let base = mirror.trim().trim_end_matches('/');
    if !base.contains("://") {
        return Err(refuse("the mirror setting is not a URL"));
    }
    Ok(format!("{base}/{path}"))
}

/// A file in an offline bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleEntry<'a> {
    /// `npm/<sha512 hex>.tgz`, named by the expected digest.
    Npm,
    /// `node/<archive name>`.
    Node(&'a str),
}

/// The path of `entry` in the offline bundle at `bundle`, once it is there: no network. A
/// missing file is [`FetchKind::Failed`], with `label` (the package's file name, such as
/// `sdk-0.1.0.tgz`) in the detail. Nothing is hashed here.
pub(crate) fn bundle_path(
    bundle: &Path,
    entry: BundleEntry<'_>,
    expected: &Expected,
    label: &str,
    data_dir: &Path,
) -> Result<PathBuf, FetchError> {
    let fail = |detail: &str| FetchError::new(FetchKind::Failed, None, None, detail, data_dir);
    let path = match entry {
        BundleEntry::Npm => bundle.join("npm").join(format!("{}.tgz", expected.hex())),
        BundleEntry::Node(name) => {
            if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
                return Err(fail(&format!(
                    "{label}: not a file name the bundle can hold"
                )));
            }
            bundle.join("node").join(name)
        }
    };
    if !path.is_file() {
        return Err(fail(&format!("the offline bundle has no {label}")));
    }
    Ok(path)
}

/// The path of an npm tarball in the offline bundle, once its digest is checked where it sits.
/// npm reads it again by path (a `file:` `resolved` in the install's lock), which is safe:
/// `npm ci` checks what it reads against the entry's `integrity`, so a file swapped after this
/// check is `EINTEGRITY`, not an install. A mismatch is [`FetchKind::Failed`].
pub(crate) fn bundle_file(
    bundle: &Path,
    expected: &Expected,
    label: &str,
    data_dir: &Path,
) -> Result<PathBuf, FetchError> {
    let fail = |detail: &str| FetchError::new(FetchKind::Failed, None, None, detail, data_dir);
    let path = bundle_path(bundle, BundleEntry::Npm, expected, label, data_dir)?;
    let mut hasher = expected.hasher();
    // The firm's folder: a link there is the firm's choice, so it is followed.
    std::fs::File::open(&path)
        .and_then(|file| hash_read(file, &mut hasher))
        .map_err(|err| {
            fail(&format!(
                "the offline bundle's {label} could not be read: {err}"
            ))
        })?;
    if !hasher.matches(expected) {
        return Err(fail(&format!(
            "the offline bundle's {label} does not match its pinned checksum"
        )));
    }
    Ok(path)
}

/// Copies `from` (Node's archive, or an npm tarball on a Windows share, in the offline bundle)
/// to `job.dest` in `agents/downloads/`, hashing the bytes as they are written, through the same
/// path as a download: the magic, the size cap, the digest, then the rename. Only the copy is
/// unpacked or handed to npm, so a share that hands different bytes to a second read cannot
/// change what runs. A copy already there with the right digest is kept. Each failure's detail
/// names `label` (the archive's or the package's file name, such as `sdk-0.1.0.tgz`), never the
/// bundle's `<hex>.tgz`.
pub(crate) fn copy_from_bundle(
    from: &Path,
    label: &str,
    job: &FetchJob,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
    data_dir: &Path,
) -> Result<u64, FetchError> {
    let at = Place {
        host: None,
        data_dir,
    };
    if let Some(size) = complete_size(&job.dest, &job.expected).map_err(|err| at.io(&err))? {
        progress(size, size);
        return Ok(size);
    }
    if let Some(parent) = job.dest.parent() {
        write::create_dir_all(parent).map_err(|err| at.io(&err))?;
    }
    let read_error = |err: io::Error| {
        at.fail(
            FetchKind::Failed,
            None,
            &format!("the offline bundle's {label} could not be read: {err}"),
        )
    };
    let file = std::fs::File::open(from).map_err(read_error)?;
    let source = Source {
        status: None,
        offset: 0,
        total: job.max_bytes,
        read_error: &read_error,
    };
    let part = part_path(&job.dest);
    write_checked(job, &part, file, &source, cancel, progress, &at).map_err(|mut err| {
        if err.kind != FetchKind::Cancelled {
            err.detail = format!("the offline bundle's {label}: {}", err.detail);
        }
        err
    })
}

#[cfg(test)]
mod tests;
