//! Telling cloud-only placeholders apart: files a sync client (`iCloud` Drive, `OneDrive`, Dropbox)
//! lists but has not downloaded. Reading one would download it, so they are never opened.

use std::fs::Metadata;
use std::path::Path;

/// Says whether a file is a cloud-only placeholder. The host passes [`OsProbe`]; tests pass one
/// that marks the paths they choose.
pub trait PlaceholderProbe {
    /// Whether the regular file at `path` (absolute), whose metadata (taken without following a
    /// symlink) is `metadata`, is cloud-only.
    fn is_cloud_only(&self, path: &Path, metadata: &Metadata) -> bool;
}

/// The probe for this OS: the file's flags or attributes. macOS: `SF_DATALESS` in `st_flags`.
/// Windows: `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `FILE_ATTRIBUTE_OFFLINE`. Other OSes:
/// never cloud-only.
// These flag names and values are unverified against real placeholders; a measurement task
// checks them on each OS and each sync client.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsProbe;

/// macOS `SF_DATALESS`: the file's data is not on the disk.
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x4000_0000;

/// Windows `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`: reading the file fetches it.
#[cfg(windows)]
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

/// Windows `FILE_ATTRIBUTE_OFFLINE`: the file's data is not available locally.
#[cfg(windows)]
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;

impl PlaceholderProbe for OsProbe {
    fn is_cloud_only(&self, path: &Path, metadata: &Metadata) -> bool {
        let _ = path;
        #[cfg(target_os = "macos")]
        {
            use std::os::macos::fs::MetadataExt;
            metadata.st_flags() & SF_DATALESS != 0
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes()
                & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE)
                != 0
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = metadata;
            false
        }
    }
}
