//! The Chrome for Testing download (S27, decision record 0029): where it stands, as
//! `HostEvent::BrowserInstall` carries it to every app subscriber after
//! `HostCommand::InstallBrowser`. App-wide and never logged, like `SettingsChanged`.

use serde::{Deserialize, Serialize};

use crate::Count;
use crate::open_enum::open_unit_enum;

/// Where the one Chrome for Testing download stands. A download publishes some `Downloading`,
/// then `Unpacking`, then one `Done` or one `Failed`; a download refused before it starts (an
/// unsupported machine, too little disk, the firm's policy) publishes only its `Failed`. While
/// the network is down it publishes `Offline` between tries. The daily check of a copy that is
/// already current publishes nothing. After `Done` the next
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
        /// Whether this replaces a downloaded copy more than one Stable security release
        /// behind, which no session starts until the new one is in place. False when absent.
        #[serde(default)]
        behind: bool,
    },
    /// The network could not be reached: the host tries again about every minute until it
    /// can, or until the lawyer cancels.
    Offline,
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
        /// The network failed: curl could not connect or resolve the host, the connection broke,
        /// the server failed (an HTTP 5xx), or the transfer stalled (slower than 1 byte a second
        /// for 60 s) or ran past its time bound. The lawyer's download tries again a few times
        /// first, so the app hears it only once those tries are spent.
        Network,
        /// The zip's size did not match its `Content-Length`, the response had none, or it was
        /// larger than the 400 MiB cap.
        SizeMismatch,
        /// Less than about 1 GB is free in the app data folder, or the disk filled up during the
        /// download or the unpacking.
        DiskSpace,
        /// The version list named a URL or version the host does not accept: not `https`, not
        /// Google's host, or a path that does not match the version and platform.
        HostRefused,
        /// Chrome's code signature did not check out: Windows only, and only while the host
        /// requires it. Never on macOS, where Chrome for Testing is ad hoc signed.
        Signature,
        /// The zip did not unpack, held an entry that would land outside its folder or lacked
        /// the browser, or a folder in the app data folder could not be written.
        Unpack,
        /// This machine gets no Chrome for Testing download: Linux, Windows on ARM64, or a
        /// system tool the download needs is missing.
        Unsupported,
        /// The lawyer cancelled, or the app quit, during the download.
        Cancelled,
        /// The secure connection failed: the server's certificate was not trusted, or TLS could
        /// not be set up. Not tried again.
        Tls,
        /// The server refused the request with an HTTP 4xx (such as 403 or 404). Not tried
        /// again.
        ServerRefused,
        /// The firm does not allow it: its Clauseo policy forbids the download
        /// (`browser.allow_download` false, or a policy source that cannot be read), or its
        /// Chrome, Edge or Chromium policy turns Clauseo's browser off.
        FirmForbids,
    }
    test mod browser_install_failure;
}

#[cfg(test)]
mod tests;
