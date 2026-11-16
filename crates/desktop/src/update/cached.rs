//! Last run's download, checked against this run's offer before the host reuses it.
//!
//! The host makes an offer `ready` from `updates/<version>/` when the version matches
//! (`find_cached`), and in `auto` installs it at start. Since the arm64 move (`arch.rs`), two
//! runs can want different installers of one version: the gate can flip between them (an admin
//! copy refreshed), or a release can drop its `windows-aarch64` entry. The install's binding
//! would refuse the other one, but only after the app quit for it. So each offer's check reads
//! the cached file, as `find_cached` finds it (the folder's one file), and binds it to the
//! offer's own signature and target (`file_binding::check_update_file`), the same check the
//! install applies. Any file that does not bind, an old run's included, sets
//! [`Offer::refetch`]: the host downloads the version again, which replaces the folder.

use std::path::Path;

use clauseo_host::Offer;

use super::file_binding::{check_update_file, expected_update_file};
use super::install::LIFECYCLE;
use super::plugin::{Pending, is_plain_version};

/// `offer`, with [`Offer::refetch`] set when `updates/<version>/` holds one file and it is not
/// the installer this offer's signature names for its target, under `pubkey` (none: no key is
/// set, so nothing binds). A folder with no file or several holds nothing the host reuses. An
/// offer from another updater, or a version that is not plain dotted numbers, is left as it is.
/// It reads the whole file, so it runs off the async workers.
pub(super) fn checked(mut offer: Offer, updates: &Path, pubkey: Option<&str>) -> Offer {
    let Some(pending) = offer.handle.downcast_ref::<Pending>() else {
        return offer;
    };
    if !is_plain_version(&offer.version) {
        return offer;
    }
    let Some(file) = only_file(&updates.join(&offer.version)) else {
        return offer;
    };
    let update = &pending.update;
    let expected = expected_update_file(&update.version, pending.target);
    let binds = match (std::fs::read(&file), expected, pubkey) {
        (Ok(bytes), Some(expected), Some(key)) => {
            check_update_file(key, &update.signature, &bytes, &expected, &update.version).is_ok()
        }
        _ => false,
    };
    if !binds {
        tracing::info!(
            target: LIFECYCLE,
            version = offer.version,
            arch = pending.target.platform().1,
            "last run's download is not this update's installer: downloading it again"
        );
        offer.refetch = true;
    }
    offer
}

/// The one regular file of `folder`, as the host's `find_cached` reads it; none when it has no
/// file or several, or cannot be read.
fn only_file(folder: &Path) -> Option<std::path::PathBuf> {
    let mut files = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path());
    let first = files.next()?;
    files.next().is_none().then_some(first)
}
