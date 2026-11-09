//! The Chrome for Testing download (S27, decision record 0029): where it stands, as
//! `HostEvent::BrowserInstall` carries it to every app subscriber after
//! `HostCommand::InstallBrowser`. App-wide and never logged, like `SettingsChanged`.

use serde::{Deserialize, Serialize};

use crate::Count;
use crate::open_enum::open_unit_enum;

/// Where the one Chrome for Testing download stands. A download publishes some `Downloading`,
/// then `Unpacking`, then one `Done` or one `Failed`; a download refused before it starts (an
/// unsupported machine, too little disk) publishes only its `Failed`. After `Done` the next
/// session started finds the browser; sessions already running keep their header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BrowserInstallState {
    /// The zip is downloading: `bytes` of `total` so far, at most about four a second. `total`
    /// is the response's `Content-Length`.
    Downloading {
        /// Bytes on disk so far.
        bytes: Count,
        /// The whole zip's size, from `Content-Length`.
        total: Count,
    },
    /// The zip is downloaded; it is being unpacked and checked.
    Unpacking,
    /// Chrome for Testing is in place; the next session uses it.
    Done,
    /// The download ended without a browser and left no folder behind.
    Failed {
        /// Why, for the app's copy: the app picks what it shows by `code` alone.
        code: BrowserInstallFailure,
        /// What went wrong, in English, for the log only: the app never shows it.
        message: String,
    },
}

open_unit_enum! {
    /// Why a Chrome for Testing download failed. Open: a value a newer host adds decodes to
    /// `Unknown`, which the app shows as a plain failure. The app picks its copy by this code,
    /// never from `message`.
    pub enum BrowserInstallFailure {
        /// The network failed: curl could not connect, the server answered an error, or the
        /// transfer stalled (slower than 1 byte a second for 60 s); or the proxy the download
        /// goes through (the firm's `network.upstream_proxy`, else the system's) was refused.
        Network,
        /// The zip's size did not match its `Content-Length` or the size the signed browser list
        /// pins, the response had none, or it was larger than the pinned size.
        SizeMismatch,
        /// Less than about 1 GB is free in the app data folder.
        DiskSpace,
        /// The signed browser list pins a version the host does not accept (not dotted digits),
        /// or the server answered with a redirect, which the host does not follow.
        HostRefused,
        /// Chrome's code signature did not check out (only while the host requires it).
        Signature,
        /// The zip did not unpack, held an entry that would land outside its folder or lacked
        /// the browser, or a folder in the app data folder, or the record of what was
        /// installed, could not be written.
        Unpack,
        /// This machine gets no Chrome for Testing download: Linux, Windows on ARM64, or a
        /// system tool the download needs is missing.
        Unsupported,
        /// The lawyer cancelled, or the app quit, during the download.
        Cancelled,
        /// No signed browser list verifies (neither the bundled one nor a fetched one), so the
        /// host does not know which Chrome for Testing to download, nor its hash; nothing is
        /// downloaded.
        ListUnavailable,
        /// The downloaded zip's sha256 differs from the one the signed browser list pins; the
        /// zip was removed and nothing is installed.
        HashMismatch,
    }
    test mod browser_install_failure;
}

#[cfg(test)]
mod tests;
