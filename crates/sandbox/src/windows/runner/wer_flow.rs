//! Windows Error Reporting off for the pool user (decision 0049, "Crash reporting (WER)"), apart
//! from its registry calls so the tests hold it on every OS; Windows' calls are `wer.rs`'s.
//!
//! Before the agent starts, the inner runner, as the pool user, opens the pool user's own hive
//! by name (`HKEY_USERS\<pool SID>`, never `HKEY_CURRENT_USER`, which falls back to `.DEFAULT`
//! when the hive isn't loaded; a hive that isn't loaded refuses). It walks down to each of
//! [`WER_SETTINGS`]' keys one part at a time, each part opened with `REG_OPTION_OPEN_LINK` and
//! refused if it is a registry link, making the parts that are missing. It refuses a WER key that
//! is Low-labelled or that a SID other than the pool user, `SYSTEM` or the administrators may
//! write ([`security_fault`]): such a key would let the agent turn WER back on. It sets each DWORD
//! and reads every one back, through the checked handle and again through a plain open of the
//! path, as WER opens it. Any failure refuses the launch ([`super::RunExit::PoolProfile`]): the
//! hive is broken or was changed, a retry doesn't fix it, and set-up's repair ([`repair`]) does.
//! This setting is the only control for what the job's
//! `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` doesn't reach (a fastfail, a nested job, the end
//! sweeper).
//!
//! A WER policy (`HKLM` or the pool user's `Software\Policies\...\Windows Error Reporting`) wins
//! over these values. One that keeps WER on (`Disabled` = 0) doesn't refuse the launch: it is
//! logged, and the job's limit and the sweeps are what remain (`firm-it.md`).
//!
//! A temporary profile refuses too ([`quiet`]). Set-up's repair takes the WER key, when it shuts
//! the administrators out, for them and then deletes it ([`Registry::take`]); it never takes or
//! changes a parent on the way, and one it can't open fails the repair.
//!
//! Another key or value joins by a row in [`WER_SETTINGS`].

use std::fmt;
use std::path::PathBuf;

use super::super::acl_bytes::{self, ALLOWED, Acl, Descriptor, INHERIT_ONLY, LABEL};
use super::super::setup::answers::is_temporary;
use super::super::sids::Sid;

/// A key under the pool user's hive and the DWORD values set in it.
pub(crate) struct Setting {
    /// The key's parts below `HKEY_USERS\<pool SID>`.
    pub(crate) path: &'static [&'static str],
    pub(crate) values: &'static [(&'static str, u32)],
}

/// The per-user WER key: no reporting, no crash dialog, no extra data sent, no WER event log.
pub(crate) const WER_SETTINGS: [Setting; 1] = [Setting {
    path: &[
        "Software",
        "Microsoft",
        "Windows",
        "Windows Error Reporting",
    ],
    values: &[
        ("Disabled", 1),
        ("DontShowUI", 1),
        ("DontSendAdditionalData", 1),
        ("LoggingDisabled", 1),
    ],
}];

/// The WER policy key, under `HKEY_LOCAL_MACHINE` and under the pool user's hive.
pub(crate) const WER_POLICY: &str = r"Software\Policies\Microsoft\Windows\Windows Error Reporting";

/// Where a plain read starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    /// `HKEY_USERS`.
    Users,
    /// `HKEY_LOCAL_MACHINE`.
    Machine,
}

/// How a key part is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum How {
    /// An existing key, to read, set values in and make keys under.
    Write,
    /// As [`How::Write`], made if missing.
    Make,
    /// An existing key, to read and delete.
    Delete,
}

/// The registry calls; each error is a Win32 code.
pub(crate) trait Registry {
    /// An open key.
    type Key;
    /// Opens `name` under `parent` (`None`: `HKEY_USERS`) without following a link at it
    /// (`REG_OPTION_OPEN_LINK`): a link opens as the link itself. `None` when it is missing and
    /// `how` doesn't make it.
    fn open(
        &mut self,
        parent: Option<&Self::Key>,
        name: &str,
        how: How,
    ) -> Result<Option<Self::Key>, u32>;
    /// Whether `key` is a registry link (it holds a `SymbolicLinkValue`).
    fn is_link(&mut self, key: &Self::Key) -> Result<bool, u32>;
    /// `key`'s self-relative security descriptor: its DACL and its mandatory label.
    fn security(&mut self, key: &Self::Key) -> Result<Vec<u8>, u32>;
    /// Sets the DWORD `name` to `value`.
    fn set_dword(&mut self, key: &Self::Key, name: &str, value: u32) -> Result<(), u32>;
    /// The DWORD `name`; `None` when it is missing or not a DWORD.
    fn dword(&mut self, key: &Self::Key, name: &str) -> Result<Option<u32>, u32>;
    /// The DWORD `name` of `root\path`, opened as WER opens it (links followed); `None` when the
    /// key or the value is missing, or the value isn't a DWORD.
    fn read(&mut self, root: Root, path: &str, name: &str) -> Result<Option<u32>, u32>;
    /// Deletes `key` and everything below it, never through a link.
    fn delete(&mut self, key: Self::Key) -> Result<(), u32>;
    /// Makes `name` under `parent` (opened as itself, a link too) the administrators' with full
    /// control, through `SeTakeOwnershipPrivilege` held only for the call: set-up's repair of a
    /// key that shuts the administrators out.
    fn take(&mut self, parent: &Self::Key, name: &str) -> Result<(), u32>;
}

/// The pool user's profile as this logon has it: its `ProfileList` `State` and its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Profile {
    pub(crate) state: Option<u32>,
    pub(crate) path: PathBuf,
}

/// Why WER couldn't be turned off, or the pool hive repaired.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WerError {
    NotAUser {
        sid: String,
    },
    NotLoaded {
        sid: String,
    },
    Temporary {
        path: String,
    },
    Open {
        part: &'static str,
        code: u32,
    },
    LinkCheck {
        part: &'static str,
        code: u32,
    },
    Link {
        part: &'static str,
    },
    Security {
        code: u32,
    },
    Unsafe {
        why: String,
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
    Delete {
        part: &'static str,
        code: u32,
    },
    Take {
        part: &'static str,
        code: u32,
    },
}

impl fmt::Display for WerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("turning off Windows Error Reporting for the sandbox user failed: ")?;
        match self {
            WerError::NotAUser { sid } => write!(f, "{sid} isn't a user account"),
            WerError::NotLoaded { sid } => write!(f, "the hive of {sid} isn't loaded"),
            WerError::Temporary { path } => {
                write!(
                    f,
                    "Windows gave the sandbox user a temporary profile ({path})"
                )
            }
            WerError::Open { part, code } => write!(f, "opening {part} ({code})"),
            WerError::LinkCheck { part, code } => write!(f, "checking {part} for a link ({code})"),
            WerError::Link { part } => write!(f, "{part} is a registry link"),
            WerError::Security { code } => write!(f, "reading the key's security ({code})"),
            WerError::Unsafe { why } => write!(f, "the key {why}"),
            WerError::Write { name, code } => write!(f, "setting {name} ({code})"),
            WerError::Read { name, code } => write!(f, "reading {name} back ({code})"),
            WerError::ReadBack { name, found } => match found {
                Some(found) => write!(f, "{name} reads back as {found}"),
                None => write!(f, "{name} reads back as missing"),
            },
            WerError::Delete { part, code } => write!(f, "deleting {part} ({code})"),
            WerError::Take { part, code } => {
                write!(f, "taking {part} for the administrators ({code})")
            }
        }
    }
}

/// Sets every value of [`WER_SETTINGS`] in `pool`'s hive and reads each back; the lines to log
/// (a policy keeping WER on). A temporary `profile` refuses: Windows throws its hive away at
/// logoff, so another logon of the pool user (the end sweeper's) may get one without the values.
pub(crate) fn quiet<R: Registry>(
    registry: &mut R,
    pool: &Sid,
    profile: &Profile,
) -> Result<Vec<String>, WerError> {
    if is_temporary(profile.state, &profile.path) {
        return Err(WerError::Temporary {
            path: profile.path.display().to_string(),
        });
    }
    for setting in &WER_SETTINGS {
        apply(registry, pool, setting)?;
    }
    Ok(policy_lines(registry, pool))
}

/// Opens `setting`'s key part by part, refusing a link or an unsafe key, sets its values and
/// reads every one back.
pub(crate) fn apply<R: Registry>(
    registry: &mut R,
    pool: &Sid,
    setting: &Setting,
) -> Result<(), WerError> {
    let key =
        walk(registry, pool, setting.path, How::Make)?.ok_or_else(|| WerError::NotLoaded {
            sid: pool.as_str().to_owned(),
        })?;
    let descriptor = registry
        .security(&key)
        .map_err(|code| WerError::Security { code })?;
    if let Some(why) = security_fault(&descriptor, pool) {
        return Err(WerError::Unsafe { why });
    }
    for &(name, value) in setting.values {
        registry
            .set_dword(&key, name, value)
            .map_err(|code| WerError::Write { name, code })?;
    }
    let plain = format!("{}\\{}", pool.as_str(), setting.path.join("\\"));
    for &(name, value) in setting.values {
        let found = registry
            .dword(&key, name)
            .map_err(|code| WerError::Read { name, code })?;
        if found != Some(value) {
            return Err(WerError::ReadBack { name, found });
        }
        let found = registry
            .read(Root::Users, &plain, name)
            .map_err(|code| WerError::Read { name, code })?;
        if found != Some(value) {
            return Err(WerError::ReadBack { name, found });
        }
    }
    Ok(())
}

/// Set-up's repair of `pool`'s hive, loaded: deletes a registry link on the way to each WER key
/// (the link itself, never its target), and a WER key that holds a `SymbolicLinkValue` or that
/// [`security_fault`] refuses, so the next launch makes it again. Whether it deleted anything.
pub(crate) fn repair<R: Registry>(registry: &mut R, pool: &Sid) -> Result<bool, WerError> {
    let mut deleted = false;
    for setting in &WER_SETTINGS {
        deleted |= repair_one(registry, pool, setting)?;
    }
    Ok(deleted)
}

/// [`repair`] of one setting.
pub(crate) fn repair_one<R: Registry>(
    registry: &mut R,
    pool: &Sid,
    setting: &Setting,
) -> Result<bool, WerError> {
    let sid = checked_user(pool)?;
    let Some(mut parent) =
        registry
            .open(None, sid, How::Delete)
            .map_err(|code| WerError::Open {
                part: "the hive",
                code,
            })?
    else {
        return Err(WerError::NotLoaded {
            sid: sid.to_owned(),
        });
    };
    let last = setting.path.len().saturating_sub(1);
    for (at, &part) in setting.path.iter().enumerate() {
        let key = match registry.open(Some(&parent), part, How::Delete) {
            Ok(Some(key)) => key,
            Ok(None) => return Ok(false),
            // The WER key itself shuts the administrators out: it is taken for them only to be
            // deleted, since a key left theirs alone would refuse every launch.
            Err(ACCESS_DENIED) if at == last => {
                registry
                    .take(&parent, part)
                    .map_err(|code| WerError::Take { part, code })?;
                let taken = registry
                    .open(Some(&parent), part, How::Delete)
                    .map_err(|code| WerError::Open { part, code })?
                    .ok_or(WerError::Open { part, code: 2 })?;
                registry
                    .delete(taken)
                    .map_err(|code| WerError::Delete { part, code })?;
                return Ok(true);
            }
            // A parent on the way is never taken or changed: set-up fails, naming it.
            Err(code) => return Err(WerError::Open { part, code }),
        };
        let link = registry
            .is_link(&key)
            .map_err(|code| WerError::LinkCheck { part, code })?;
        let unsafe_key = at == last && !link && {
            let descriptor = registry
                .security(&key)
                .map_err(|code| WerError::Security { code })?;
            security_fault(&descriptor, pool).is_some()
        };
        if link || unsafe_key {
            registry
                .delete(key)
                .map_err(|code| WerError::Delete { part, code })?;
            return Ok(true);
        }
        parent = key;
    }
    Ok(false)
}

/// `ERROR_ACCESS_DENIED`.
pub(crate) const ACCESS_DENIED: u32 = 5;

/// The pool user's SID as a hive name, refused unless it is a user account's (never
/// `.DEFAULT` or a service's).
fn checked_user(pool: &Sid) -> Result<&str, WerError> {
    if !pool.is_user_account() {
        return Err(WerError::NotAUser {
            sid: pool.as_str().to_owned(),
        });
    }
    Ok(pool.as_str())
}

/// Opens `path` under `pool`'s hive one part at a time, each part checked not to be a link.
/// `None` when a part is missing and `how` doesn't make it.
fn walk<R: Registry>(
    registry: &mut R,
    pool: &Sid,
    path: &[&'static str],
    how: How,
) -> Result<Option<R::Key>, WerError> {
    let sid = checked_user(pool)?;
    let Some(mut key) = registry
        .open(None, sid, How::Write)
        .map_err(|code| WerError::Open {
            part: "the hive",
            code,
        })?
    else {
        return Err(WerError::NotLoaded {
            sid: sid.to_owned(),
        });
    };
    for &part in path {
        let Some(next) = registry
            .open(Some(&key), part, how)
            .map_err(|code| WerError::Open { part, code })?
        else {
            return Ok(None);
        };
        match registry.is_link(&next) {
            Ok(false) => {}
            Ok(true) => return Err(WerError::Link { part }),
            Err(code) => return Err(WerError::LinkCheck { part, code }),
        }
        key = next;
    }
    Ok(Some(key))
}

/// The lines for a WER policy that keeps WER on, or that couldn't be read.
fn policy_lines<R: Registry>(registry: &mut R, pool: &Sid) -> Vec<String> {
    let user = format!("{}\\{WER_POLICY}", pool.as_str());
    let places = [
        (
            Root::Machine,
            WER_POLICY.to_owned(),
            "a machine policy (HKLM)",
        ),
        (Root::Users, user, "a policy for the sandbox user"),
    ];
    let mut lines = Vec::new();
    for (root, path, whose) in places {
        match registry.read(root, &path, "Disabled") {
            Ok(Some(0)) => lines.push(format!(
                "clauseo-sandbox-run: {whose} keeps Windows Error Reporting on (Disabled = 0); \
                 a crash outside the job's limit may still start WerFault"
            )),
            Ok(_) => {}
            Err(code) => lines.push(format!(
                "clauseo-sandbox-run: {whose} for Windows Error Reporting couldn't be read ({code})"
            )),
        }
    }
    lines
}

/// The rights that change a key: set a value, make a subkey or a link, delete, change the DACL
/// or the owner, and the generic rights that hold them.
const WRITE_RIGHTS: u32 =
    0x0002 | 0x0004 | 0x0020 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x1000_0000 | 0x4000_0000;
/// `SYSTEM_MANDATORY_LABEL_NO_WRITE_UP`.
const NO_WRITE_UP: u32 = 0x1;
/// The Medium integrity level's relative id.
const MEDIUM: u32 = 0x2000;
/// `SYSTEM` and the administrators, who may write the key besides the pool user.
const TRUSTED_WRITERS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];

/// Why the key's `descriptor` would let the agent (Low, restricted) or another SID write it:
/// a label below Medium or without no-write-up, no DACL, or an allow entry giving a write right
/// to anyone but `pool`, `SYSTEM` and the administrators (or one this check can't read). `None`
/// when only those may write it. Inherit-only entries don't apply to the key itself.
pub(crate) fn security_fault(descriptor: &[u8], pool: &Sid) -> Option<String> {
    let Ok(parsed) = Descriptor::parse(descriptor) else {
        return Some("has a security descriptor that can't be read".to_owned());
    };
    let applies = |ace: &&&[u8]| acl_bytes::flags(ace) & INHERIT_ONLY == 0;
    if let Some(sacl) = parsed.sacl {
        let Ok(sacl) = Acl::parse(sacl) else {
            return Some("has a label that can't be read".to_owned());
        };
        if let Some(label) = sacl
            .aces()
            .iter()
            .filter(applies)
            .find(|ace| acl_bytes::kind(ace) == LABEL)
        {
            let level = acl_bytes::sid(label)
                .and_then(acl_bytes::sid_text)
                .and_then(|sid| sid.as_str().strip_prefix("S-1-16-")?.parse::<u32>().ok());
            let mask = acl_bytes::mask(label).unwrap_or(0);
            match level {
                Some(level) if level >= MEDIUM && mask & NO_WRITE_UP != 0 => {}
                Some(level) if level < MEDIUM => {
                    return Some(format!("is labelled below Medium (S-1-16-{level})"));
                }
                _ => return Some("has a label that doesn't stop writes from below".to_owned()),
            }
        }
    }
    let Some(dacl) = parsed.dacl else {
        return Some("has no DACL, so anyone may write it".to_owned());
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return Some("has a DACL that can't be read".to_owned());
    };
    let pool_bytes = acl_bytes::sid_bytes(pool);
    for ace in dacl.aces().iter().filter(applies) {
        let kind = acl_bytes::kind(ace);
        if matches!(kind, 1 | 6 | 10 | 12) {
            continue;
        }
        if acl_bytes::mask(ace).unwrap_or(u32::MAX) & WRITE_RIGHTS == 0 {
            continue;
        }
        let who = acl_bytes::sid(ace).filter(|_| kind == ALLOWED);
        let trusted = who.is_some_and(|who| {
            who == pool_bytes.as_slice()
                || acl_bytes::sid_text(who)
                    .is_some_and(|sid| TRUSTED_WRITERS.contains(&sid.as_str()))
        });
        if !trusted {
            let name = who.and_then(acl_bytes::sid_text).map_or_else(
                || format!("an entry of type {kind}"),
                |sid| sid.as_str().to_owned(),
            );
            return Some(format!("may be written by {name}"));
        }
    }
    None
}

#[cfg(test)]
mod tests;
