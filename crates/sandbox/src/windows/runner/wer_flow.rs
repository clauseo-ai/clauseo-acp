//! Windows Error Reporting off for the pool user (decision 0049, "Crash reporting (WER)"), apart
//! from its registry calls so the tests hold it on every OS; Windows' calls are `wer.rs`'s.
//!
//! Before the agent starts, the inner runner, as the pool user and so owning its `HKCU`, opens
//! each of [`WER_SETTINGS`]' keys (made if missing) without following a registry link at its last
//! part, refuses a key that is a link, sets each DWORD and reads every one back. Any failure
//! refuses the launch ([`super::RunExit::Token`]): this setting is the only control for what the
//! job's `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` doesn't reach (a fastfail, a nested job,
//! the end sweeper). Setting the values again changes nothing. Per-user `LocalDumps` settings
//! don't exist (Windows reads `LocalDumps` from `HKLM` only), so none is set.
//!
//! Another key or value joins by a row in [`WER_SETTINGS`].

use std::fmt;

/// A key under `HKEY_CURRENT_USER` and the DWORD values set in it.
pub(crate) struct Setting {
    pub(crate) key: &'static str,
    pub(crate) values: &'static [(&'static str, u32)],
}

/// The per-user WER key: no reporting (`Disabled`), and no crash dialog (`DontShowUI`).
pub(crate) const WER_SETTINGS: [Setting; 1] = [Setting {
    key: r"Software\Microsoft\Windows\Windows Error Reporting",
    values: &[("Disabled", 1), ("DontShowUI", 1)],
}];

/// The registry calls, on keys under `HKEY_CURRENT_USER`; each error is a Win32 code.
pub(crate) trait Registry {
    /// An open key.
    type Key;
    /// Opens `path` for setting and reading values, made if missing, without following a link
    /// at its last part (`REG_OPTION_OPEN_LINK`): a link opens as the link itself.
    fn open(&mut self, path: &str) -> Result<Self::Key, u32>;
    /// Whether `key` is a registry link.
    fn is_link(&mut self, key: &Self::Key) -> Result<bool, u32>;
    /// Sets the DWORD `name` to `value`.
    fn set_dword(&mut self, key: &Self::Key, name: &str, value: u32) -> Result<(), u32>;
    /// The DWORD `name`; `None` when it is missing or not a DWORD.
    fn dword(&mut self, key: &Self::Key, name: &str) -> Result<Option<u32>, u32>;
}

/// Why WER couldn't be turned off.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WerError {
    Open {
        key: &'static str,
        code: u32,
    },
    LinkCheck {
        key: &'static str,
        code: u32,
    },
    Link {
        key: &'static str,
    },
    Write {
        name: &'static str,
        code: u32,
    },
    Read {
        name: &'static str,
        code: u32,
    },
    ReadBack {
        name: &'static str,
        found: Option<u32>,
    },
}

impl fmt::Display for WerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("turning off Windows Error Reporting for the sandbox user failed: ")?;
        match self {
            WerError::Open { key, code } => write!(f, "opening {key} ({code})"),
            WerError::LinkCheck { key, code } => write!(f, "checking {key} for a link ({code})"),
            WerError::Link { key } => write!(f, "{key} is a registry link"),
            WerError::Write { name, code } => write!(f, "setting {name} ({code})"),
            WerError::Read { name, code } => write!(f, "reading {name} back ({code})"),
            WerError::ReadBack { name, found } => match found {
                Some(found) => write!(f, "{name} reads back as {found}"),
                None => write!(f, "{name} reads back as missing"),
            },
        }
    }
}

/// Sets every value of [`WER_SETTINGS`] and reads each back.
pub(crate) fn quiet<R: Registry>(registry: &mut R) -> Result<(), WerError> {
    WER_SETTINGS
        .iter()
        .try_for_each(|setting| apply(registry, setting))
}

/// Opens `setting`'s key, refuses a link, sets its values and reads every one back.
pub(crate) fn apply<R: Registry>(registry: &mut R, setting: &Setting) -> Result<(), WerError> {
    let key = setting.key;
    let opened = registry
        .open(key)
        .map_err(|code| WerError::Open { key, code })?;
    match registry.is_link(&opened) {
        Ok(false) => {}
        Ok(true) => return Err(WerError::Link { key }),
        Err(code) => return Err(WerError::LinkCheck { key, code }),
    }
    for &(name, value) in setting.values {
        registry
            .set_dword(&opened, name, value)
            .map_err(|code| WerError::Write { name, code })?;
    }
    for &(name, value) in setting.values {
        let found = registry
            .dword(&opened, name)
            .map_err(|code| WerError::Read { name, code })?;
        if found != Some(value) {
            return Err(WerError::ReadBack { name, found });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
