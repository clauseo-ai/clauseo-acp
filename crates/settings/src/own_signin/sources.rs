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
//!   property list with content, the login user's before the computer's, or the Windows
//!   registry's HKLM `Settings`), then the files (`<managed>/managed-settings.json` and each
//!   `<managed>/managed-settings.d/*.json` not starting `.`, in name order). Their `env`
//!   objects are joined as Claude Code joins them: the first tier to set a name (in any case)
//!   wins, and a lower tier's other spelling of it takes the winner's value. Clauseo's own rule
//!   on top, stricter than Claude Code's: once a tier has set a key, token or provider switch, a
//!   lower tier's that would turn the sign-in off is dropped. HKCU's `Settings` applies only
//!   when no other tier has content; with one that has, only a custom endpoint of HKCU's is
//!   kept, with a blank key or token of HKCU's (they can only refuse a launch that was safe).
//!   In each tier after the first with content, a helper key whose value is non-blank text drops
//!   the names it governs from that tier's `env`, in any case, as Claude Code drops them
//!   ([`DROPPED_BY`]); a dropped name does not hide a lower tier's. The files are one tier: a
//!   later file's helper key replaces an earlier one's, and a blank or null one clears it. A
//!   document whose only keys are [`NO_CONTENT_KEYS`] has no content. Claude Code does not
//!   always apply the cached remote settings as read (it may wait to verify them, and use them
//!   meanwhile without their routing names, or demote them below the other tiers), so the read
//!   keeps more readings ([`SettingsEnv::readings`]): without the cache when it sets a checked
//!   name; when it has content, with it as a tier with content but no pairs, and with it last.
//!   A launch is refused if any
//!   reading would offer the sign-in: the cache may add an endpoint, never take one away
//!   (Clauseo's own, stricter rule).
//!
//! A source that is there but cannot be read, is over [`MAX_FILE`] (the cache: over
//! [`MAX_REMOTE_FILE`]) or does not parse, an `env`
//! that is no object, a checked name whose value is no string, number or boolean, or a property
//! list holding a date or data value anywhere makes the read [`unsure`](SettingsEnv::unsure): a
//! base URL may be set. So does a policy helper
//! (`policyHelper` or `policyHelpers`) in a policy document: Claude Code runs it at start and its
//! output takes the policy tier's place, so the tier's own pairs are dropped too. The read keeps
//! why ([`Unreadable`]: its kind, and the setting's name or the file's path). After a property
//! list that is unsure, the next is read too, for the pairs that can only refuse. A source that
//! is not there changes nothing. Only the names the check reads are kept, a key's or token's
//! value only as set or empty.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::unreadable::{REGISTRY_VALUES, Unreadable, UnreadableKind};

mod files;

pub(super) use files::OsReads;
pub use files::claude_managed_dir;
#[cfg(test)]
use files::plist_paths_for;
use files::{holds_date_or_data, managed_files, read_file, read_file_up_to, without_bom};

/// The largest settings file read: a larger one is not read, and makes the read unsure.
pub(crate) const MAX_FILE: u64 = 1 << 20;

/// The largest cached remote settings file read, Claude Code's own limit for it: 8 MiB. Claude
/// Code ignores a larger one; here it makes the read unsure (Clauseo's own, stricter rule).
pub(crate) const MAX_REMOTE_FILE: u64 = 8 << 20;

/// What a registry tier read gives: its settings text (`None` when it is not there), or `Err`
/// when it is there but cannot be read.
pub(crate) type TierRead = Result<Option<String>, ()>;

/// The names whose values are kept only as set (`"set"`) or empty (blank).
const SECRET_NAMES: [&str; 2] = super::KEY_VARS;

/// The keys of a policy document that name a policy helper.
const HELPER_KEYS: [&str; 2] = ["policyHelper", "policyHelpers"];

/// The keys that alone give a document no content, as Claude Code counts it.
const NO_CONTENT_KEYS: [&str; 2] = ["managedSourcesBehavior", "wslInheritsWindowsSettings"];

/// The provider switches an AWS helper key governs.
const AWS_SWITCHES: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_MANTLE",
];

/// Claude Code's helper keys and the names each one drops, set to non-blank text in a policy
/// tier after the first with content, from that tier's `env` (a name in any case: Claude Code
/// upper-cases it first).
const DROPPED_BY: [(&str, &[&str]); 4] = [
    ("apiKeyHelper", &["ANTHROPIC_BASE_URL"]),
    ("awsAuthRefresh", AWS_SWITCHES),
    ("awsCredentialExport", AWS_SWITCHES),
    ("gcpAuthRefresh", &["CLAUDE_CODE_USE_VERTEX"]),
];

/// Why a read is unsure when it does not say.
static UNKNOWN: Unreadable = Unreadable {
    kind: UnreadableKind::Unreadable,
    setting: None,
};

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
    /// Why the read is unsure (with a policy helper, before [`Self::policy`] makes it so).
    pub(super) why: Option<Unreadable>,
    /// Each helper key read ([`DROPPED_BY`]) and whether its last value is non-blank text.
    pub(super) helpers: BTreeMap<&'static str, bool>,
    /// The other readings of the pairs, as Claude Code may apply the cached remote settings
    /// ([`Self::readings`]).
    pub(super) others: Vec<Vec<(OsString, OsString)>>,
}

impl SettingsEnv {
    /// The pairs Claude Code applies over its environment, in order, the policy tier last.
    pub fn pairs(&self) -> &[(OsString, OsString)] {
        &self.pairs
    }

    /// Each reading of the pairs to check: [`Self::pairs`]; without the cached remote settings
    /// when they set a checked name; when they have content, with them as a tier with content
    /// but no pairs, and with them below the other tiers. A launch is refused if any offers the
    /// sign-in.
    pub fn readings(&self) -> impl Iterator<Item = &[(OsString, OsString)]> {
        std::iter::once(self.pairs.as_slice()).chain(self.others.iter().map(Vec::as_slice))
    }

    /// Whether a source was there but could not be read: a base URL may be set.
    pub const fn unsure(&self) -> bool {
        self.unsure
    }

    /// Why the read is unsure, if it is.
    pub fn unreadable(&self) -> Option<&Unreadable> {
        self.unsure.then(|| self.why.as_ref().unwrap_or(&UNKNOWN))
    }

    /// A read that found nothing but could not tell where Claude Code's sources are, as there is
    /// no home folder: unsure.
    pub fn unknown() -> Self {
        let mut env = Self::default();
        env.mark(UnreadableKind::NoHome, None);
        env
    }

    /// Makes the read unsure for `kind`, the setting named when known; the first why stays.
    fn mark(&mut self, kind: UnreadableKind, setting: Option<&str>) {
        self.unsure = true;
        self.why.get_or_insert_with(|| Unreadable {
            kind,
            setting: setting.map(str::to_owned),
        });
    }

    /// Runs `read`, naming `setting` as what could not be read when `read` made this read
    /// unsure first and named nothing itself.
    fn at(&mut self, setting: &str, read: impl FnOnce(&mut Self)) {
        let named = self.why.is_some();
        read(self);
        if !named && let Some(why) = &mut self.why {
            why.setting.get_or_insert_with(|| setting.to_owned());
        }
    }

    /// The JSON settings file at `path` (a `policy` document or not).
    fn add_json_file(&mut self, path: &Path, policy: bool) {
        let read = read_file(path);
        self.at(&path.display().to_string(), |env| {
            env.add_json(read, policy);
        });
    }

    /// Reads `sources`; on a thread that may block on the disk.
    pub fn read(sources: &ClaudeSources<'_>) -> Self {
        Self::read_with(sources, sources.os_policy.then(OsReads::system))
    }

    /// [`Self::read`] with the OS policy tiers' reads given (`None`: not read).
    pub(super) fn read_with(sources: &ClaudeSources<'_>, os_reads: Option<OsReads>) -> Self {
        let mut env = Self::default();
        let read_at = |path: PathBuf| {
            let read = read_file(&path);
            (path, read)
        };
        let legacy = sources
            .config
            .map(|config| read_at(config.join(".config.json")));
        let global = match legacy {
            None | Some((_, Ok(None))) => sources
                .global
                .map(|global| read_at(global.join(".claude.json"))),
            other => other,
        };
        if let Some((path, read)) = global {
            env.at(&path.display().to_string(), |env| env.add_json(read, false));
        }
        if let Some(config) = sources.config.filter(|_| sources.user) {
            env.add_json_file(&config.join("settings.json"), false);
        }
        let mut files = Self::default();
        if let Ok(paths) = managed_files(sources.managed) {
            for path in paths {
                files.add_json_file(&path, true);
            }
        } else {
            let folder = sources.managed.join("managed-settings.d");
            files.mark(
                UnreadableKind::Unreadable,
                Some(&folder.display().to_string()),
            );
        }
        let mut remote = Self::default();
        if let Some(config) = sources.config {
            let path = config.join("remote-settings.json");
            let read = read_file_up_to(&path, MAX_REMOTE_FILE);
            remote.at(&path.display().to_string(), |remote| {
                remote.add_json(read, true);
            });
        }
        let mut os = Self::default();
        if let Some(OsReads { plists, registry }) = os_reads {
            os.add_plists(
                plists
                    .iter()
                    .map(|(path, read)| (Some(path.as_path()), read.clone())),
            );
            let admin = [&files, &remote, &os]
                .iter()
                .any(|tier| tier.content || tier.unsure);
            os.add_registry_tiers(registry, admin);
        }
        let reading = |policy: Self| [env.pairs.clone(), policy.pairs].concat();
        let mut others = Vec::new();
        if !remote.pairs.is_empty() {
            others.push(reading(Self::policy([
                Self::default(),
                os.clone(),
                files.clone(),
            ])));
        }
        if remote.content {
            // Before verifying it, Claude Code may use the cache without its routing names.
            let stripped = Self {
                pairs: Vec::new(),
                ..remote.clone()
            };
            others.push(reading(Self::policy([stripped, os.clone(), files.clone()])));
            // Claude Code may demote the cache below the other tiers.
            others.push(reading(Self::policy([
                os.clone(),
                files.clone(),
                remote.clone(),
            ])));
        }
        env.extend(Self::policy([remote, os, files]));
        env.others = others;
        env
    }

    /// The policy tier's `env` from its tiers, highest first: the first tier to set a name in
    /// any case wins, and a lower tier's other spelling of the name gets the winner's value, as
    /// Claude Code writes it. Clauseo's own rule: once a tier has set a key, token or provider
    /// switch, a lower tier's that would turn the sign-in off is dropped. In each tier after
    /// the first with content, the names its helper keys govern are skipped, as Claude Code
    /// skips them, and hide no lower tier's. A policy helper anywhere drops every pair, as its
    /// output takes the tier's place, and makes the read unsure.
    pub(super) fn policy(tiers: [Self; 3]) -> Self {
        let mut policy = Self::default();
        for tier in &tiers {
            policy.unsure |= tier.unsure;
            policy.content |= tier.content;
            policy.helper |= tier.helper;
            if policy.why.is_none() {
                policy.why.clone_from(&tier.why);
            }
        }
        if policy.helper {
            policy.unsure = true;
            return policy;
        }
        let first = tiers.iter().position(|tier| tier.content);
        // Each name set so far, upper-cased, with the winning tier's value.
        let mut seen: BTreeMap<String, OsString> = BTreeMap::new();
        let mut spelled: BTreeSet<OsString> = BTreeSet::new();
        let mut credential = false;
        for (index, tier) in tiers.into_iter().enumerate() {
            let mut named = BTreeMap::new();
            let mut sets_credential = false;
            let drops = tier.drops();
            let skipped = first.is_some_and(|first| index > first);
            for (name, value) in tier.pairs {
                let upper = name.to_string_lossy().to_ascii_uppercase();
                if let Some(winner) = seen.get(&upper) {
                    if spelled.insert(name.clone()) {
                        policy.pairs.push((name, winner.clone()));
                    }
                    continue;
                }
                // Claude Code's order: a name set above first, then the helper keys' names.
                if skipped && drops.contains(upper.as_str()) {
                    continue;
                }
                let is_credential = super::KEY_VARS.contains(&upper.as_str())
                    || super::PROVIDER_VARS.contains(&upper.as_str());
                sets_credential |= is_credential;
                if is_credential && credential && super::turns_off(&upper, &value) {
                    continue;
                }
                named.entry(upper).or_insert_with(|| value.clone());
                spelled.insert(name.clone());
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
        self.helpers.extend(other.helpers);
        if self.why.is_none() {
            self.why = other.why;
        }
    }

    /// Keeps only the pairs that can only refuse a launch ([`refuse_only`]), and none of the
    /// helper keys' effects: for a source Claude Code may not read.
    fn refuse_only(&mut self) {
        self.pairs.retain(|(name, value)| refuse_only(name, value));
        self.helpers.clear();
        self.helper = false;
    }

    /// The names this tier's helper keys drop from its `env` ([`DROPPED_BY`]): those of each
    /// key whose last value read is non-blank text.
    fn drops(&self) -> BTreeSet<&'static str> {
        DROPPED_BY
            .iter()
            .filter(|(key, _)| self.helpers.get(key) == Some(&true))
            .flat_map(|(_, names)| names.iter().copied())
            .collect()
    }

    /// Notes a policy document's helper keys, `value_of` giving each one's text (`None`: not
    /// there, `Ok(None)`: null, `Err`: of no text kind, which makes the read unsure). A later
    /// document's value replaces an earlier one's, as Claude Code merges the files.
    fn add_helper_keys<'d>(
        &mut self,
        value_of: impl Fn(&str) -> Option<Result<Option<&'d str>, ()>>,
    ) {
        for (key, _) in DROPPED_BY {
            match value_of(key) {
                Some(Ok(text)) => {
                    let set = text.is_some_and(|text| !super::js_trim(text).is_empty());
                    self.helpers.insert(key, set);
                }
                Some(Err(())) => self.mark(UnreadableKind::NotText, Some(key)),
                None => {}
            }
        }
    }

    /// A JSON settings document's `env`; `Err` (there, unreadable) or no JSON is unsure. A
    /// `policy` document may name a policy helper.
    pub(super) fn add_json(&mut self, read: Result<Option<Vec<u8>>, ()>, policy: bool) {
        match read {
            Ok(None) => {}
            Ok(Some(bytes)) => match serde_json::from_slice::<Value>(without_bom(&bytes)) {
                Ok(document) => {
                    let keys = document.as_object();
                    self.content |= keys.is_some_and(|keys| has_content(keys.keys()));
                    if let Some(keys) = keys.filter(|_| policy) {
                        if HELPER_KEYS.iter().any(|key| keys.contains_key(*key)) {
                            self.add_policy_helper();
                        }
                        self.add_helper_keys(|key| {
                            keys.get(key).map(|value| match value {
                                Value::String(text) => Ok(Some(text.as_str())),
                                Value::Null => Ok(None),
                                _ => Err(()),
                            })
                        });
                    }
                    self.add_env(document.get("env"));
                }
                Err(_) => self.mark(UnreadableKind::NotParsed, None),
            },
            Err(()) => self.mark(UnreadableKind::Unreadable, None),
        }
    }

    /// A policy document names a policy helper: [`Self::policy`] drops the tier's pairs and makes
    /// the read unsure.
    fn add_policy_helper(&mut self) {
        self.helper = true;
        self.why.get_or_insert(Unreadable {
            kind: UnreadableKind::PolicyHelper,
            setting: None,
        });
    }

    /// The registry's `Settings` values, HKLM's then HKCU's: HKCU's applies only when neither
    /// HKLM's nor another policy tier (`admin`) has content, else only the pairs of HKCU's that
    /// can only refuse are kept ([`refuse_only`]).
    pub(super) fn add_registry_tiers(&mut self, values: Vec<TierRead>, admin: bool) {
        let mut values = values.into_iter();
        if let Some(hklm) = values.next() {
            self.at(REGISTRY_VALUES[0], |env| env.add_registry(hklm));
        }
        let admin = admin || self.content || self.unsure;
        for hkcu in values {
            let mut user = Self::default();
            user.at(REGISTRY_VALUES[1], |user| user.add_registry(hkcu));
            if admin {
                user.refuse_only();
            }
            self.extend(user);
        }
    }

    /// The managed-preferences property lists at their paths, in Claude Code's order: the first
    /// with content is the tier, and the later ones are not read. After one that is unsure, the
    /// next are read too, for their pairs that can only refuse ([`refuse_only`]), until one
    /// with content: Claude Code may skip the one it cannot read.
    pub(super) fn add_plists<'p>(
        &mut self,
        reads: impl IntoIterator<Item = (Option<&'p Path>, Result<Option<Vec<u8>>, ()>)>,
    ) {
        let mut unsure = false;
        for (path, read) in reads {
            let mut plist = Self::default();
            match path {
                Some(path) => plist.at(&path.display().to_string(), |plist| plist.add_plist(read)),
                None => plist.add_plist(read),
            }
            if unsure {
                plist.refuse_only();
            }
            let content = plist.content;
            unsure |= plist.unsure;
            self.extend(plist);
            if content {
                return;
            }
        }
    }

    /// One registry `Settings` value: JSON text; an empty one is nothing. One starting with
    /// U+FEFF is unsure (Clauseo's own, stricter rule): Claude Code parses the text `reg.exe`
    /// prints with `JSON.parse`, which fails on it, so it may read the tier as empty.
    fn add_registry(&mut self, read: TierRead) {
        match read {
            Ok(Some(text)) if text.starts_with('\u{FEFF}') => {
                self.mark(UnreadableKind::NotParsed, None);
            }
            Ok(Some(text)) if !text.trim().is_empty() => {
                self.add_json(Ok(Some(text.trim_end().as_bytes().to_vec())), true);
            }
            Ok(_) => {}
            Err(()) => self.mark(UnreadableKind::Unreadable, None),
        }
    }

    /// A managed-preferences property list (XML or binary): its top-level `env` dictionary.
    /// An empty file is nothing, as Claude Code reads it; one that is no property list is unsure.
    /// So is one holding a date or data value anywhere, with none of its pairs kept: Claude Code
    /// reads the file through plutil's JSON conversion, which fails on those (Clauseo's own,
    /// stricter rule, as what Claude Code then does is not checked).
    pub(super) fn add_plist(&mut self, read: Result<Option<Vec<u8>>, ()>) {
        let bytes = match read {
            Ok(Some(bytes)) if !bytes.is_empty() => bytes,
            Ok(_) => return,
            Err(()) => return self.mark(UnreadableKind::Unreadable, None),
        };
        let Ok(value) = plist::Value::from_reader(Cursor::new(bytes)) else {
            return self.mark(UnreadableKind::NotParsed, None);
        };
        if holds_date_or_data(&value) {
            return self.mark(UnreadableKind::DateOrData, None);
        }
        let settings = value.as_dictionary();
        self.content |= settings.is_some_and(|keys| has_content(keys.keys()));
        if let Some(keys) = settings {
            if HELPER_KEYS.iter().any(|key| keys.contains_key(key)) {
                self.add_policy_helper();
            }
            self.add_helper_keys(|key| {
                keys.get(key)
                    .map(|value| value.as_string().map(Some).ok_or(()))
            });
        }
        let Some(env) = settings.and_then(|settings| settings.get("env")) else {
            return;
        };
        let Some(env) = env.as_dictionary() else {
            return self.mark(UnreadableKind::EnvNotObject, None);
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
            Some(_) => self.mark(UnreadableKind::EnvNotObject, None),
        }
    }

    /// One `env` entry, kept when the check reads its name; a value of no kind Claude Code
    /// reads as text is unsure.
    fn add_pair(&mut self, name: &str, text: Option<String>) {
        let Some(var) = super::checked_as(name) else {
            return;
        };
        let Some(text) = text else {
            return self.mark(UnreadableKind::NotText, Some(var));
        };
        let secret = SECRET_NAMES
            .iter()
            .any(|secret| name.eq_ignore_ascii_case(secret));
        let value = match (secret, super::blank(OsStr::new(&text))) {
            (true, false) => "set".to_owned(),
            (true, true) => String::new(),
            (false, _) => text,
        };
        self.pairs
            .push((OsString::from(name), OsString::from(value)));
    }
}

/// Whether a document with these keys has content: a key besides [`NO_CONTENT_KEYS`].
fn has_content<'k>(mut keys: impl Iterator<Item = &'k String>) -> bool {
    keys.any(|key| !NO_CONTENT_KEYS.contains(&key.as_str()))
}

/// Whether the pair can only refuse a launch, whatever the launch's own variables: a custom
/// endpoint or socket ([`super::routes`]), or a key or token kept as empty (blank).
fn refuse_only(name: &OsStr, value: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    super::routes(&name, value) || (super::KEY_VARS.contains(&name.as_str()) && value.is_empty())
}

#[cfg(test)]
mod tests;
