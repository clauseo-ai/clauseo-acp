//! Whether a policy, rather than this computer's own settings, defines a logon right (the
//! pool accounts' logon rights, 0049's "Logon rights"). A right a policy defines is written
//! over by Windows at every policy refresh, so set-up grants a right only when no policy
//! defines it (`logon_grant.rs`).
//!
//! Two sources, read in this order:
//!
//! 1. the Group Policy security-template cache, `%windir%\security\templates\policies\gpt*.inf`
//!    and `gpt*.dom`: each applied GPO's template, whose `[Privilege Rights]` section lists the
//!    rights it defines ([`inf_rights`]);
//! 2. Intune (MDM): the `UserRights` CSP's values ([`MDM_NAMES`]) under [`MDM_KEY`] and under
//!    each provider's own key ([`provider_key`]), where a setting can stand alone.
//!
//! A right listed by any of them is [`RightPolicy::Defined`], and includes the pool only when
//! every list that defines it does, since which one wins isn't known from here. Any read that
//! fails is [`RightPolicy::Unknown`], which no caller treats as granted.
//!
//! [`PolicyCache`] keeps the parsed lists in memory for a launch's check, keyed by the cache
//! files' last-write times and the MDM key's (`RegQueryInfoKey`), re-read when one changes or
//! after [`CACHE_TTL`]. No WMI is read here. Pure apart from [`PolicySource`]
//! (`rights_source.rs` on Windows), so it is tested on every OS.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::answers::{DENY_BATCH_RIGHT, DENY_INTERACTIVE_RIGHT, DENY_SERVICE_RIGHT};
use super::answers::{LogonKind, pool_sids_for};
use super::logon_grant::COVERING_HOLDERS;
use super::machine::MachineError;
use crate::windows::pool::MachineRecord;
use crate::windows::pool::{POOL_GROUP, pool_number};
use crate::windows::sids::Sid;

/// "Allow log on locally".
pub const INTERACTIVE_RIGHT: &str = "SeInteractiveLogonRight";
/// "Log on as a batch job".
pub const BATCH_RIGHT: &str = "SeBatchLogonRight";
/// "Log on as a service".
pub const SERVICE_RIGHT: &str = "SeServiceLogonRight";

/// The Intune `UserRights` CSP's merged settings, as `PolicyManager` stores them.
pub const MDM_KEY: &str = r"SOFTWARE\Microsoft\PolicyManager\current\device\UserRights";
/// Where `PolicyManager` keeps each MDM provider's own settings: a subkey per provider (a GUID).
pub const MDM_PROVIDERS: &str = r"SOFTWARE\Microsoft\PolicyManager\providers";

/// The provider `provider`'s `UserRights` key (`providers\<GUID>\default\Device\UserRights`).
/// An Intune setting can stand here alone, with nothing under [`MDM_KEY`].
pub fn provider_key(provider: &str) -> String {
    format!(r"{MDM_PROVIDERS}\{provider}\default\Device\UserRights")
}

/// Each right this module reads, with the `UserRights` CSP's name for it (Microsoft Learn,
/// "Policy CSP - `UserRights`").
pub const MDM_NAMES: [(&str, &str); 6] = [
    (INTERACTIVE_RIGHT, "AllowLocalLogOn"),
    (DENY_INTERACTIVE_RIGHT, "DenyLocalLogOn"),
    (BATCH_RIGHT, "LogOnAsBatchJob"),
    (DENY_BATCH_RIGHT, "DenyLogOnAsBatchJob"),
    (SERVICE_RIGHT, "LogOnAsService"),
    (DENY_SERVICE_RIGHT, "DenyLogOnAsService"),
];

/// How long [`PolicyCache`] trusts what it read while no timestamp changes.
pub const CACHE_TTL: Duration = Duration::from_mins(10);

/// What policy says of a right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPolicy {
    /// A GPO or Intune defines it; `includes_pool` when every list that defines it names a
    /// principal the pool accounts' tokens hold.
    Defined {
        /// The lists include the pool accounts.
        includes_pool: bool,
    },
    /// No policy defines it: this computer's own setting stands.
    NotDefined,
    /// A source couldn't be read: never treated as granted, and checked again next time.
    Unknown,
}

/// What set-up and the SYSTEM refresh last found policy says of "Allow log on locally", kept in
/// the record (`LogonPolicy`, with `LogonPolicyAt`) where the desktop user's launch can read it:
/// the template cache itself is readable by administrators and SYSTEM only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyState {
    /// A policy defines it and includes the pool.
    DefinedWithPool,
    /// A policy defines it without the pool: setting up again doesn't fix a 1385.
    DefinedWithoutPool,
    /// No policy defines it: set-up grants it, so setting up again fixes a 1385.
    NotDefined,
    /// The policy couldn't be read.
    Unknown,
    /// No policy defines it, yet the right in effect no longer admits the pool: someone took
    /// set-up's grant (or Users) off locally, so setting up again fixes a 1385. Only the SYSTEM
    /// refresh's backstop writes it.
    GrantRemoved,
}

impl PolicyState {
    /// Every state, in order.
    pub const ALL: [PolicyState; 5] = [
        PolicyState::DefinedWithPool,
        PolicyState::DefinedWithoutPool,
        PolicyState::NotDefined,
        PolicyState::Unknown,
        PolicyState::GrantRemoved,
    ];

    /// The state of what was read.
    pub fn of(policy: RightPolicy) -> PolicyState {
        match policy {
            RightPolicy::Defined {
                includes_pool: true,
            } => PolicyState::DefinedWithPool,
            RightPolicy::Defined {
                includes_pool: false,
            } => PolicyState::DefinedWithoutPool,
            RightPolicy::NotDefined => PolicyState::NotDefined,
            RightPolicy::Unknown => PolicyState::Unknown,
        }
    }

    /// Its text in the record.
    pub fn text(self) -> &'static str {
        match self {
            PolicyState::DefinedWithPool => "defined_with_pool",
            PolicyState::DefinedWithoutPool => "defined_without_pool",
            PolicyState::NotDefined => "not_defined",
            PolicyState::Unknown => "unknown",
            PolicyState::GrantRemoved => "grant_removed",
        }
    }

    /// The state `text` names; `None` for any other text.
    pub fn parse(text: &str) -> Option<PolicyState> {
        PolicyState::ALL
            .into_iter()
            .find(|state| state.text() == text)
    }
}

/// One entry of a right's list: a SID (`*S-1-…` in a template), or a name no lookup resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// A SID.
    Sid(Sid),
    /// A name, as written.
    Name(String),
}

/// The pool, as the lists are compared against it: the group's SID once it exists, and the
/// pool accounts' SIDs.
#[derive(Debug, Clone, Copy)]
pub struct Pool<'a> {
    /// `ClauseoSandboxUsers`' SID, when the group exists.
    pub group: Option<&'a Sid>,
    /// The pool accounts' SIDs.
    pub users: &'a [Sid],
}

/// Where the policy is read from: the files and the registry on Windows, a fake in the tests.
pub trait PolicySource {
    /// Every file in the template cache's folder with its last-write time (any unit, compared
    /// only for change); a missing folder is none.
    fn cache_files(&mut self) -> io::Result<Vec<(PathBuf, u64)>>;
    /// The bytes of the cache file `path`.
    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>>;
    /// Every Intune `UserRights` key there is, with its last-write time: [`MDM_KEY`]
    /// (`current\device`), and each provider's (`provider_key`), which is where the CSP's
    /// value can be alone; a key that is missing is left out.
    fn mdm_keys(&mut self) -> io::Result<Vec<(String, u64)>>;
    /// The `REG_SZ` value `name` under `HKLM\<key>`; `None` when it is missing.
    fn mdm_value(&mut self, key: &str, name: &str) -> io::Result<Option<String>>;
    /// The SID of the account or group `name`; `None` when no lookup finds it.
    fn resolve(&mut self, name: &str) -> Option<Sid>;
}

/// Whether `name` is a template the cache holds: `gpt*.inf` or `gpt*.dom`, without case.
pub fn is_cache_file(name: &str) -> bool {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    stem.get(..3)
        .is_some_and(|start| start.eq_ignore_ascii_case("gpt"))
        && (extension.eq_ignore_ascii_case("inf") || extension.eq_ignore_ascii_case("dom"))
}

/// A template's text: UTF-16 after its byte-order mark (little-endian, as `secedit` writes
/// them, or big-endian), else UTF-8 (a lone bad byte replaced). Text that isn't a template is
/// caught by [`inf_rights`].
pub fn inf_text(bytes: &[u8]) -> String {
    let utf16 = |body: &[u8], read: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| read(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(body, u16::from_le_bytes);
    }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(body, u16::from_be_bytes);
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(body).into_owned()
}

/// The entry `text` of a list: `*S-1-…` is a SID, anything else a name; quotes and spaces
/// around it are dropped; an empty entry is none.
pub fn principal_of(text: &str) -> Option<Principal> {
    let text = text.trim().trim_matches('"').trim();
    if text.is_empty() {
        return None;
    }
    if let Some(sid) = text.strip_prefix('*') {
        return Some(match Sid::parse(sid.trim()) {
            Ok(sid) => Principal::Sid(sid),
            Err(_not_a_sid) => Principal::Name(text.to_owned()),
        });
    }
    Some(Principal::Name(text.to_owned()))
}

/// Why a cache file isn't a template [`inf_rights`] can read: its rights are then unknown,
/// never "not defined".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InfError {
    /// A NUL or a replacement character: UTF-16 without its byte-order mark, or binary.
    #[error("the template isn't text")]
    NotText,
    /// No `[section]` line at all: every template has `[Unicode]` and `[Version]`.
    #[error("the template has no section")]
    NoSection,
    /// A line in `[Privilege Rights]` that isn't `Right = list`.
    #[error("a line in the template's [Privilege Rights] isn't a right")]
    BadLine,
}

/// The rights a template's `[Privilege Rights]` section defines, each with its list. A right
/// with an empty value is defined, for no one; a template with no such section defines none.
/// Text that isn't a template is an [`InfError`].
pub fn inf_rights(text: &str) -> Result<BTreeMap<String, Vec<Principal>>, InfError> {
    if text.contains(['\0', '\u{FFFD}']) {
        return Err(InfError::NotText);
    }
    let mut rights = BTreeMap::new();
    let (mut inside, mut sections) = (false, 0);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            sections += 1;
            inside = line.eq_ignore_ascii_case("[Privilege Rights]");
            continue;
        }
        if !inside {
            continue;
        }
        let (name, value) = line.split_once('=').ok_or(InfError::BadLine)?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(InfError::BadLine);
        }
        let list = value.split(',').filter_map(principal_of).collect();
        rights.insert(name.to_owned(), list);
    }
    if sections == 0 {
        return Err(InfError::NoSection);
    }
    Ok(rights)
}

/// A `UserRights` CSP value's list: entries split by U+F000 (the CSP's separator) or commas, a
/// `<![CDATA[…]]>` wrapper and `&#xF000;` written as text read the same.
pub fn mdm_principals(value: &str) -> Vec<Principal> {
    let value = value
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&#xF000;", "\u{F000}");
    value
        .split(['\u{F000}', ',', '\0'])
        .flat_map(|chunk| {
            // `*S-1-5-32-544*S-1-5-11`, two SIDs whose separator was lost on the way.
            let starts: Vec<usize> = chunk.match_indices('*').map(|(at, _)| at).collect();
            if starts.len() > 1 && starts.first() == Some(&0) {
                let mut ends = starts.clone();
                ends.remove(0);
                ends.push(chunk.len());
                starts
                    .iter()
                    .zip(ends)
                    .map(|(&start, end)| chunk.get(start..end).unwrap_or_default().to_owned())
                    .collect()
            } else {
                vec![chunk.to_owned()]
            }
        })
        .filter_map(|entry| principal_of(&entry))
        .collect()
}

/// The right's logon kind and whether it is a deny right; `None` for a right not read here.
fn kind_of(right: &str) -> Option<LogonKind> {
    match right {
        INTERACTIVE_RIGHT | DENY_INTERACTIVE_RIGHT => Some(LogonKind::Interactive),
        BATCH_RIGHT | DENY_BATCH_RIGHT => Some(LogonKind::Batch),
        SERVICE_RIGHT | DENY_SERVICE_RIGHT => Some(LogonKind::Service),
        _ => None,
    }
}

/// Whether `principal` stands for the pool accounts in a `kind` logon: a SID their token holds
/// ([`pool_sids_for`]), the group's, or one of theirs; or, for a name no lookup resolved,
/// `ClauseoSandboxUsers`, `Users`, `Everyone`, `Authenticated Users` or a pool account's name,
/// with or without its domain, compared without case.
pub fn names_pool(principal: &Principal, kind: LogonKind, pool: Pool<'_>) -> bool {
    match principal {
        Principal::Sid(sid) => {
            pool_sids_for(kind).contains(&sid.as_str())
                || Some(sid) == pool.group
                || pool.users.contains(sid)
        }
        Principal::Name(name) => {
            let leaf = name.rsplit('\\').next().unwrap_or(name);
            [POOL_GROUP, "Users", "Everyone", "Authenticated Users"]
                .iter()
                .any(|known| leaf.eq_ignore_ascii_case(known))
                || pool_number(leaf).is_some()
        }
    }
}

/// What every source said, each right with one list per source that defines it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    lists: BTreeMap<String, Vec<Vec<Principal>>>,
}

impl Snapshot {
    /// What policy says of `right` for `pool`.
    pub fn right(&self, right: &str, pool: Pool<'_>) -> RightPolicy {
        let Some(lists) = self.lists.get(right).filter(|lists| !lists.is_empty()) else {
            return RightPolicy::NotDefined;
        };
        let Some(kind) = kind_of(right) else {
            return RightPolicy::Unknown;
        };
        let includes_pool = lists
            .iter()
            .all(|list| list.iter().any(|entry| names_pool(entry, kind, pool)));
        RightPolicy::Defined { includes_pool }
    }

    fn add(&mut self, right: &str, list: Vec<Principal>) {
        self.lists.entry(right.to_owned()).or_default().push(list);
    }
}

/// What the cache is keyed by: each template's path and last-write time, and the MDM key's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    files: Vec<(PathBuf, u64)>,
    mdm: Vec<(String, u64)>,
}

/// The stamp now: the templates sorted by path, others in the folder left out.
pub fn stamp(source: &mut dyn PolicySource) -> io::Result<Stamp> {
    let mut files: Vec<(PathBuf, u64)> = source
        .cache_files()?
        .into_iter()
        .filter(|(path, _)| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_cache_file)
        })
        .collect();
    files.sort();
    let mut mdm = source.mdm_keys()?;
    mdm.sort();
    Ok(Stamp { files, mdm })
}

/// Reads every source `stamp` names, names resolved to SIDs where a lookup finds them.
pub fn load(source: &mut dyn PolicySource, stamp: &Stamp) -> io::Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    let watched = MDM_NAMES.map(|(right, _)| right);
    for (path, _) in &stamp.files {
        let text = inf_text(&source.read_file(path)?);
        let rights =
            inf_rights(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        for (right, list) in rights {
            if let Some(right) = watched.iter().find(|r| r.eq_ignore_ascii_case(&right)) {
                snapshot.add(right, resolved(source, list));
            }
        }
    }
    for (key, _) in &stamp.mdm {
        for (right, name) in MDM_NAMES {
            if let Some(value) = source.mdm_value(key, name)? {
                let list = resolved(source, mdm_principals(&value));
                snapshot.add(right, list);
            }
        }
    }
    Ok(snapshot)
}

fn resolved(source: &mut dyn PolicySource, list: Vec<Principal>) -> Vec<Principal> {
    list.into_iter()
        .map(|entry| match entry {
            Principal::Name(name) => source
                .resolve(&name)
                .map_or(Principal::Name(name), Principal::Sid),
            sid @ Principal::Sid(_) => sid,
        })
        .collect()
}

/// What policy says of `right` now, read afresh (set-up's check); a failed read is
/// [`RightPolicy::Unknown`].
pub fn read_policy(source: &mut dyn PolicySource, right: &str, pool: Pool<'_>) -> RightPolicy {
    let read = stamp(source).and_then(|stamp| load(source, &stamp));
    match read {
        Ok(snapshot) => snapshot.right(right, pool),
        Err(error) => {
            tracing::warn!(%error, right, "the logon-right policy couldn't be read");
            RightPolicy::Unknown
        }
    }
}

/// The lists in memory between launches, as the module says.
#[derive(Debug)]
pub struct PolicyCache {
    ttl: Duration,
    held: Option<(Stamp, Duration, Snapshot)>,
}

impl PolicyCache {
    /// An empty cache whose reads are trusted for `ttl`.
    pub fn new(ttl: Duration) -> PolicyCache {
        PolicyCache { ttl, held: None }
    }

    /// What policy says of `right` for `pool` at `now` (monotonic time since any fixed start,
    /// as the caller's clock gives it): the held lists while the stamp is the
    /// same and younger than the TTL, else read again. A failed read is
    /// [`RightPolicy::Unknown`] and drops what was held.
    pub fn right(
        &mut self,
        source: &mut dyn PolicySource,
        right: &str,
        pool: Pool<'_>,
        now: Duration,
    ) -> RightPolicy {
        let stamp = match stamp(source) {
            Ok(stamp) => stamp,
            Err(error) => {
                tracing::warn!(%error, "the logon-right policy's timestamps couldn't be read");
                self.held = None;
                return RightPolicy::Unknown;
            }
        };
        if let Some((held, at, snapshot)) = &self.held
            && *held == stamp
            && now.saturating_sub(*at) < self.ttl
        {
            return snapshot.right(right, pool);
        }
        match load(source, &stamp) {
            Ok(snapshot) => {
                let answer = snapshot.right(right, pool);
                self.held = Some((stamp, now, snapshot));
                answer
            }
            Err(error) => {
                tracing::warn!(%error, "the logon-right policy couldn't be read");
                self.held = None;
                RightPolicy::Unknown
            }
        }
    }
}

/// The record as the SYSTEM refresh updates its `LogonPolicy`: the registry on Windows
/// (`rights_source.rs`), a fake in the tests.
pub trait LaunchRecord {
    /// The record; `None` when the key is missing or holds no record.
    fn read(&mut self) -> Result<Option<MachineRecord>, MachineError>;
    /// What policy says now of "Allow log on locally" for `pool`.
    fn policy(&mut self, pool: Pool<'_>) -> RightPolicy;
    /// Who holds "Allow log on locally" in effect now (`LsaEnumerateAccountsWithUserRight`).
    fn holders(&mut self) -> Result<Vec<Sid>, MachineError>;
    /// Writes `LogonPolicy` as `state` and `LogonPolicyAt` as `at`, into the key only when
    /// it is there.
    fn write(&mut self, state: PolicyState, at: u64) -> Result<(), MachineError>;
}

/// What [`update_launch_record`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchUpdate {
    /// No record, or one being removed: nothing written, no key made.
    NoRecord,
    /// The policy couldn't be read: the last known state, and its time, kept.
    KeptLast,
    /// Written.
    Written(PolicyState),
}

/// The SYSTEM refresh's update of the record's `LogonPolicy`, found at `at`: what policy says
/// now for the record's group and pool, written only into a record that is there and not being
/// removed, and never `unknown` over what set-up or an earlier refresh found. The backstop: when
/// the right in effect (the LSA's holders) no longer admits the pool (none of the group,
/// `Users`, `Everyone` or `Authenticated Users`), something replaced the list set-up left. With
/// no definition anywhere that was a local admin, and setting up again grants it back:
/// `grant_removed`. Otherwise it is a policy (perhaps one this read can't see):
/// `defined_without_pool`, whatever the read says.
pub fn update_launch_record(
    record: &mut dyn LaunchRecord,
    at: u64,
) -> Result<LaunchUpdate, MachineError> {
    let Some(found) = record.read()?.filter(|found| !found.removing) else {
        return Ok(LaunchUpdate::NoRecord);
    };
    let pool = Pool {
        group: Some(&found.group),
        users: &found.pool,
    };
    let read = record.policy(pool);
    let mut state = PolicyState::of(read);
    match record.holders() {
        Ok(holders) if !admits_pool(&holders, &found.group) => {
            state = if read == RightPolicy::NotDefined {
                PolicyState::GrantRemoved
            } else {
                PolicyState::DefinedWithoutPool
            };
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "who holds \"Allow log on locally\" couldn't be read"),
    }
    if state == PolicyState::Unknown {
        return Ok(LaunchUpdate::KeptLast);
    }
    record.write(state, at)?;
    Ok(LaunchUpdate::Written(state))
}

/// Whether `holders` of "Allow log on locally" admit the pool: the group `group`, or one of
/// [`COVERING_HOLDERS`] (`Users`, `Everyone`, `Authenticated Users`).
pub fn admits_pool(holders: &[Sid], group: &Sid) -> bool {
    holders
        .iter()
        .any(|sid| sid == group || COVERING_HOLDERS.contains(&sid.as_str()))
}

#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod tests;
