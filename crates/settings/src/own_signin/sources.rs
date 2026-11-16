//! The `env` Claude Code 2.1.293 copies from its settings sources over the environment it was
//! started with (`applyConfigEnvironmentVariables`), for [`super::own_signin_reaches_endpoint`].
//! The sources are those the host's offline probe reads
//! (`host/src/actor/turn/offline/probe/settings.rs`), applied in Claude Code's order, the policy
//! tier last, so a later value wins:
//!
//! - the global config: `<config>/.config.json` when present, else `<global>/.claude.json`
//!   (`<config>` and `<global>` are the launch's `CLAUDE_CONFIG_DIR`; without one, `~/.claude`
//!   and the home folder);
//! - the user source, `<config>/settings.json`, only when the launch's `settingSources` names
//!   `user` (sessions do; the limits helper passes none);
//! - the policy tier, always. Its tiers are the cached remote settings
//!   (`<config>/remote-settings.json`), then the OS's (the first macOS managed-preferences
//!   property list with content, or the Windows registry's HKLM `Settings`), then the files
//!   (`<managed>/managed-settings.json` and each `<managed>/managed-settings.d/*.json` not
//!   starting `.`, in name order). Their `env` objects are joined: the first tier to set a name
//!   (in any case) wins, and once a tier has set a key, token or provider switch, a lower tier's
//!   that would turn the sign-in off is dropped. HKCU's `Settings` applies only when no other
//!   tier has content; with one that has, only a custom endpoint of HKCU's is kept (it can only
//!   refuse a launch that was safe).
//!
//! A source that is there but cannot be read, is over [`MAX_FILE`] or does not parse, an `env`
//! that is no object, or a checked name whose value is no string, number or boolean makes the
//! read [`unsure`](SettingsEnv::unsure): a base URL may be set. So does a policy helper
//! (`policyHelper` or `policyHelpers`) in a policy document: Claude Code runs it at start and its
//! output takes the policy tier's place, so the tier's own pairs are dropped too. A source that
//! is not there changes nothing. Only the names the check reads are kept, a key's or token's
//! value only as set or empty.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Cursor, ErrorKind, Read as _};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The largest settings file read: a larger one is not read, and makes the read unsure.
pub(crate) const MAX_FILE: u64 = 1 << 20;

/// The most `managed-settings.d` documents read; more make the read unsure.
const MAX_DROP_INS: usize = 64;

/// Claude Code's managed-preferences domain.
#[cfg(any(target_os = "macos", test))]
const DOMAIN: &str = "com.anthropic.claudecode";

/// What a registry tier read gives: its settings text (`None` when it is not there), or `Err`
/// when it is there but cannot be read.
pub(crate) type TierRead = Result<Option<String>, ()>;

/// The names whose values are kept only as set (`"set"`) or empty (blank).
const SECRET_NAMES: [&str; 2] = super::KEY_VARS;

/// The keys of a policy document that name a policy helper.
const HELPER_KEYS: [&str; 2] = ["policyHelper", "policyHelpers"];

/// Where one launch's Claude Code finds its settings sources.
#[derive(Debug, Clone, Copy)]
pub struct ClaudeSources<'a> {
    /// The folder of `managed-settings.json` ([`claude_managed_dir`] outside tests).
    pub managed: &'a Path,
    /// Claude Code's config folder: the launch's `CLAUDE_CONFIG_DIR`, else `~/.claude`. With
    /// none, only the policy files under `managed` and the OS tiers are read.
    pub config: Option<&'a Path>,
    /// The folder of the global config's `.claude.json`: the launch's `CLAUDE_CONFIG_DIR`, else
    /// the home folder.
    pub global: Option<&'a Path>,
    /// Whether the launch's `settingSources` names `user`, so `<config>/settings.json` applies.
    pub user: bool,
    /// Whether the OS policy tiers are read (macOS managed preferences, the Windows registry).
    pub os_policy: bool,
}

/// What a launch's Claude Code settings sources add to its environment: the checked names'
/// pairs in the order they apply, and whether a source could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsEnv {
    pub(super) pairs: Vec<(OsString, OsString)>,
    pub(super) unsure: bool,
    /// Whether a document read had any content (an object or dictionary with a key): HKCU's
    /// tier applies only when no other policy source has.
    pub(super) content: bool,
    /// Whether a policy document names a policy helper.
    pub(super) helper: bool,
}

impl SettingsEnv {
    /// The pairs Claude Code applies over its environment, in order, the policy tier last.
    pub fn pairs(&self) -> &[(OsString, OsString)] {
        &self.pairs
    }

    /// Whether a source was there but could not be read: a base URL may be set.
    pub const fn unsure(&self) -> bool {
        self.unsure
    }

    /// A read that found nothing but could not tell where Claude Code's sources are: unsure.
    pub fn unknown() -> Self {
        Self {
            unsure: true,
            ..Self::default()
        }
    }

    /// Reads `sources`; on a thread that may block on the disk.
    pub fn read(sources: &ClaudeSources<'_>) -> Self {
        let mut env = Self::default();
        let legacy = sources
            .config
            .map(|config| read_file(&config.join(".config.json")));
        let global = match legacy {
            None | Some(Ok(None)) => sources
                .global
                .map(|global| read_file(&global.join(".claude.json"))),
            other => other,
        };
        if let Some(global) = global {
            env.add_json(global, false);
        }
        if let Some(config) = sources.config.filter(|_| sources.user) {
            env.add_json(read_file(&config.join("settings.json")), false);
        }
        let mut files = Self::default();
        match managed_files(sources.managed) {
            Ok(paths) => {
                for path in paths {
                    files.add_json(read_file(&path), true);
                }
            }
            Err(()) => files.unsure = true,
        }
        let mut remote = Self::default();
        if let Some(config) = sources.config {
            remote.add_json(read_file(&config.join("remote-settings.json")), true);
        }
        let mut os = Self::default();
        if sources.os_policy {
            os.add_plists(plist_paths().iter().map(|path| read_file(path)));
            let admin = [&files, &remote, &os]
                .iter()
                .any(|tier| tier.content || tier.unsure);
            os.add_registry_tiers(super::claude_registry_settings(), admin);
        }
        env.extend(Self::policy([remote, os, files]));
        env
    }

    /// The policy tier's `env` from its tiers, highest first: the first tier to set a name in
    /// any case wins, and once a tier has set a key, token or provider switch, a lower tier's
    /// that would turn the sign-in off is dropped. A policy helper anywhere drops every pair, as
    /// its output takes the tier's place, and makes the read unsure.
    pub(super) fn policy(tiers: [Self; 3]) -> Self {
        let mut policy = Self::default();
        for tier in &tiers {
            policy.unsure |= tier.unsure;
            policy.content |= tier.content;
            policy.helper |= tier.helper;
        }
        if policy.helper {
            policy.unsure = true;
            return policy;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut credential = false;
        for tier in tiers {
            let mut named = BTreeSet::new();
            let mut sets_credential = false;
            for (name, value) in tier.pairs {
                let upper = name.to_string_lossy().to_ascii_uppercase();
                if seen.contains(&upper) {
                    continue;
                }
                let is_credential = super::KEY_VARS.contains(&upper.as_str())
                    || super::PROVIDER_VARS.contains(&upper.as_str());
                sets_credential |= is_credential;
                if is_credential && credential && super::turns_off(&upper, &value) {
                    continue;
                }
                named.insert(upper);
                policy.pairs.push((name, value));
            }
            seen.extend(named);
            credential |= sets_credential;
        }
        policy
    }

    /// `other`'s pairs after these.
    fn extend(&mut self, other: Self) {
        self.pairs.extend(other.pairs);
        self.unsure |= other.unsure;
        self.content |= other.content;
        self.helper |= other.helper;
    }

    /// A JSON settings document's `env`; `Err` (there, unreadable) or no JSON is unsure. A
    /// `policy` document may name a policy helper.
    pub(super) fn add_json(&mut self, read: Result<Option<Vec<u8>>, ()>, policy: bool) {
        match read {
            Ok(None) => {}
            Ok(Some(bytes)) => match serde_json::from_slice::<Value>(without_bom(&bytes)) {
                Ok(document) => {
                    let keys = document.as_object();
                    self.content |= keys.is_some_and(|keys| !keys.is_empty());
                    self.helper |= policy
                        && keys.is_some_and(|keys| {
                            HELPER_KEYS.iter().any(|key| keys.contains_key(*key))
                        });
                    self.add_env(document.get("env"));
                }
                Err(_) => self.unsure = true,
            },
            Err(()) => self.unsure = true,
        }
    }

    /// The registry's `Settings` values, HKLM's then HKCU's: HKCU's applies only when neither
    /// HKLM's nor another policy tier (`admin`) has content, else only a custom endpoint of
    /// HKCU's is kept.
    pub(super) fn add_registry_tiers(&mut self, values: Vec<TierRead>, admin: bool) {
        let mut values = values.into_iter();
        if let Some(hklm) = values.next() {
            self.add_registry(hklm);
        }
        let admin = admin || self.content || self.unsure;
        for hkcu in values {
            let mut user = Self::default();
            user.add_registry(hkcu);
            if admin {
                user.pairs.retain(|(name, value)| {
                    let name = name.to_string_lossy().to_ascii_uppercase();
                    super::routes(&name, value)
                });
            }
            self.extend(user);
        }
    }

    /// The managed-preferences property lists, in Claude Code's order: the first with content
    /// is the tier, and the later ones are not read. One that is unsure ends the search too.
    pub(super) fn add_plists(
        &mut self,
        reads: impl IntoIterator<Item = Result<Option<Vec<u8>>, ()>>,
    ) {
        for read in reads {
            let mut plist = Self::default();
            plist.add_plist(read);
            let found = plist.content || plist.unsure;
            self.extend(plist);
            if found {
                return;
            }
        }
    }

    /// One registry `Settings` value: JSON text; an empty one is nothing.
    fn add_registry(&mut self, read: TierRead) {
        match read {
            Ok(Some(text)) if !text.trim().is_empty() => {
                self.add_json(Ok(Some(text.trim_end().as_bytes().to_vec())), true);
            }
            Ok(_) => {}
            Err(()) => self.unsure = true,
        }
    }

    /// A managed-preferences property list (XML or binary): its top-level `env` dictionary.
    /// An empty file is nothing, as Claude Code reads it; one that is no property list is unsure.
    pub(super) fn add_plist(&mut self, read: Result<Option<Vec<u8>>, ()>) {
        let bytes = match read {
            Ok(Some(bytes)) if !bytes.is_empty() => bytes,
            Ok(_) => return,
            Err(()) => {
                self.unsure = true;
                return;
            }
        };
        let Ok(value) = plist::Value::from_reader(Cursor::new(bytes)) else {
            self.unsure = true;
            return;
        };
        let settings = value.as_dictionary();
        self.content |= settings.is_some_and(|keys| !keys.is_empty());
        self.helper |=
            settings.is_some_and(|keys| HELPER_KEYS.iter().any(|key| keys.contains_key(key)));
        let Some(env) = settings.and_then(|settings| settings.get("env")) else {
            return;
        };
        let Some(env) = env.as_dictionary() else {
            self.unsure = true;
            return;
        };
        for (name, value) in env {
            let text = match value {
                plist::Value::String(text) => Some(text.clone()),
                plist::Value::Boolean(on) => Some(on.to_string()),
                plist::Value::Integer(number) => Some(number.to_string()),
                plist::Value::Real(number) => Some(number.to_string()),
                _ => None,
            };
            self.add_pair(name, text);
        }
    }

    /// A settings `env`: none is nothing, one that is no object is unsure.
    fn add_env(&mut self, env: Option<&Value>) {
        match env {
            None | Some(Value::Null) => {}
            Some(Value::Object(env)) => {
                for (name, value) in env {
                    let text = match value {
                        Value::String(text) => Some(text.clone()),
                        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
                        Value::Null | Value::Array(_) | Value::Object(_) => None,
                    };
                    self.add_pair(name, text);
                }
            }
            Some(_) => self.unsure = true,
        }
    }

    /// One `env` entry, kept when the check reads its name; a value of no kind Claude Code
    /// reads as text is unsure.
    fn add_pair(&mut self, name: &str, text: Option<String>) {
        if !super::checked(name) {
            return;
        }
        let Some(text) = text else {
            self.unsure = true;
            return;
        };
        let secret = SECRET_NAMES
            .iter()
            .any(|secret| name.eq_ignore_ascii_case(secret));
        let value = match (secret, text.trim().is_empty()) {
            (true, false) => "set".to_owned(),
            (true, true) => String::new(),
            (false, _) => text,
        };
        self.pairs
            .push((OsString::from(name), OsString::from(value)));
    }
}

/// The folder of Claude Code's `managed-settings.json` and `managed-mcp.json` on this OS.
pub fn claude_managed_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Library/Application Support/ClaudeCode");
    #[cfg(windows)]
    return PathBuf::from(r"C:\Program Files\ClaudeCode");
    #[cfg(not(any(windows, target_os = "macos")))]
    return PathBuf::from("/etc/claude-code");
}

/// Claude Code's policy files in `managed`: `managed-settings.json`, then the drop-ins; `Err`
/// when the drop-ins cannot be listed or are too many.
fn managed_files(managed: &Path) -> Result<Vec<PathBuf>, ()> {
    let mut files = vec![managed.join("managed-settings.json")];
    let folder = managed.join("managed-settings.d");
    let entries = match std::fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) if absent(error.kind()) => return Ok(files),
        Err(_) => return Err(()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.map_err(drop)?.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".json") && !name.starts_with('.') {
            names.push(name.into_owned());
        }
        if names.len() > MAX_DROP_INS {
            return Err(());
        }
    }
    names.sort();
    files.extend(names.into_iter().map(|name| folder.join(name)));
    Ok(files)
}

/// The managed-preferences files Claude Code reads on this machine: none but on macOS.
fn plist_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let user = crate::policy::managed_prefs::login_user().ok().flatten();
        plist_paths_for(user.as_deref())
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// The managed-preferences files for the login user `user` (the computer's alone without one,
/// as Claude Code skips the user's file when it finds no name).
#[cfg(any(target_os = "macos", test))]
pub(super) fn plist_paths_for(user: Option<&str>) -> Vec<PathBuf> {
    use crate::policy::managed_prefs::{MANAGED_DIR, file_paths};
    user.and_then(|user| file_paths(DOMAIN, user).ok())
        .unwrap_or_else(|| vec![Path::new(MANAGED_DIR).join(format!("{DOMAIN}.plist"))])
}

/// `path`'s bytes: `None` when it is not there, `Err` when it is there but cannot be read or
/// holds more than [`MAX_FILE`].
pub(super) fn read_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if absent(error.kind()) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(drop)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_FILE) {
        return Err(());
    }
    Ok(Some(bytes))
}

/// Whether an open's error says the file is not there (or its folder is not a folder).
fn absent(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// `bytes` without a leading UTF-8 byte order mark.
fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

#[cfg(test)]
mod tests;
