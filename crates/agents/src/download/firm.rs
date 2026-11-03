//! The firm's two ways around the public registry: a mirror base for tarball URLs, and an
//! offline bundle folder read instead of the network.

use std::path::{Path, PathBuf};

use super::{FetchError, FetchKind, hash_file, host_of};
use crate::integrity::Expected;
use crate::log::redact_url;

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

/// The path of `entry` in the offline bundle at `bundle`, once its digest is checked: no
/// network and no copy. A missing or mismatched file is [`FetchKind::Failed`], with `label`
/// (the package's file name, such as `sdk-0.1.0.tgz`) in the detail.
pub(crate) fn bundle_file(
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
    let mut hasher = expected.hasher();
    hash_file(&path, &mut hasher).map_err(|err| {
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
