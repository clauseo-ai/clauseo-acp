//! [`Cause`]: an error as the log file may show it, its codes and OS error kinds and nothing
//! of its text. An error's `Display` can hold a path (`matters::Error::Io`, `FolderMissing`,
//! `settings::Error::Read`) or a detail with a title in it, and a path is matter content
//! (decision record 0047), so a log site passes `error = %Cause(&error)`, never `%error`.

use std::error::Error as StdError;
use std::fmt;
use std::io;

use clauseo_protocol::{ErrorCode, HostError};

/// Writes the codes along an error's source chain: each layer this crate knows gives its
/// `ErrorCode` (repeats are left out), a `ProfileError` its `log_text`, and each `io::Error`
/// its kind and OS code. A layer it
/// does not know gives nothing, so nothing unknown ever reaches the log; a chain of none of
/// them writes `unknown`.
pub(crate) struct Cause<'a>(pub(crate) &'a (dyn StdError + 'static));

impl fmt::Display for Cause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut last: Option<ErrorCode> = None;
        let mut wrote = false;
        let mut layer = Some(self.0);
        while let Some(error) = layer {
            if let Some(code) = code_of(error) {
                if last != Some(code) {
                    if wrote {
                        f.write_str(" ")?;
                    }
                    f.write_str(code.as_str())?;
                    wrote = true;
                }
                last = Some(code);
            } else if let Some(profile) = error.downcast_ref::<clauseo_profile::ProfileError>() {
                if wrote {
                    f.write_str(" ")?;
                }
                write!(f, "profile:{}", profile.log_text())?;
                wrote = true;
            } else if let Some(io) = error.downcast_ref::<io::Error>() {
                if wrote {
                    f.write_str(" ")?;
                }
                write!(f, "io:{:?}", io.kind())?;
                if let Some(os) = io.raw_os_error() {
                    write!(f, "/os:{os}")?;
                }
                wrote = true;
            }
            layer = error.source();
        }
        if !wrote {
            f.write_str("unknown")?;
        }
        Ok(())
    }
}

/// The code of one layer, for the error types whose codes the host knows.
fn code_of(error: &(dyn StdError + 'static)) -> Option<ErrorCode> {
    if let Some(error) = error.downcast_ref::<crate::Error>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<HostError>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<clauseo_matters::Error>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<clauseo_store::Error>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<clauseo_acp::Error>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<clauseo_process::Error>() {
        return Some(error.code());
    }
    None
}
