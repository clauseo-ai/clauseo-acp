//! macOS: the Trash, through `NSFileManager`'s `trashItemAtURL:resultingItemURL:error:` alone.
//! Never through Finder: that raises an Automation prompt, and the app asks for none.
//! A volume without a Trash (a share, some external drives) refuses the move with an error,
//! never deleting the file for good.

use std::io;
use std::path::{Path, PathBuf};

use objc2_foundation::{NSError, NSFileManager, NSString, NSURL};

use super::TrashError;

/// `NSFileNoSuchFileError` (`FoundationErrors.h`): nothing at the path.
const NO_SUCH_FILE: isize = 4;
/// `NSFileReadNoSuchFileError` (`FoundationErrors.h`).
const READ_NO_SUCH_FILE: isize = 260;
/// `NSFeatureUnsupportedError` (`FoundationErrors.h`): the volume has no Trash.
const FEATURE_UNSUPPORTED: isize = 3328;

/// Moves `path` to the Trash; returns where it went.
pub(super) fn trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    // APFS and HFS+ names are UTF-8; anything else is left to the plain delete.
    let text = path
        .to_str()
        .ok_or_else(|| TrashError::Failed(io::ErrorKind::InvalidData.into()))?;
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(text), false);
    let manager = NSFileManager::defaultManager();
    let mut resulting = None;
    match manager.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting)) {
        Ok(()) => Ok(resulting
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))),
        Err(error) => Err(why(&error)),
    }
}

/// `error` as a [`TrashError`]: a missing file is `NotFound`, an unsupported volume
/// [`TrashError::NoTrash`], anything else a failure carrying the Cocoa error's code.
fn why(error: &NSError) -> TrashError {
    let cocoa = error.domain().to_string() == "NSCocoaErrorDomain";
    match error.code() {
        NO_SUCH_FILE | READ_NO_SUCH_FILE if cocoa => {
            TrashError::Failed(io::ErrorKind::NotFound.into())
        }
        FEATURE_UNSUPPORTED if cocoa => TrashError::NoTrash,
        code => TrashError::Failed(io::Error::other(format!("NSError code {code}"))),
    }
}
