//! Why policy locked a key, when the lawyer could not guess it from the policy itself: a source
//! that cannot be read, a value that does not type, a section this version cannot read, or a
//! `policy.json` that holds a key twice. A plain policy value locks with no [`LockReason`].
//!
//! Structured, never English: the host logs each one, and a later protocol type mirrors it for
//! the app as `LockReason { keys, cause, source }`. Nothing here holds a value or a path
//! (decision record 0047), and a section is named only when it is a plain word (#411's rule).

use crate::error::Error;
use crate::plans::field_shaped;
use crate::policy::{managed_prefs, registry};
use crate::warning::Origin;

/// What made a policy lock a key without setting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LockCause {
    /// The source could not be read: every security key it could have set is locked off.
    SourceUnreadable,
    /// The source's value for the key does not type: the key is locked off, or at its default.
    MalformedValue,
    /// The source names the key's section in a form this version cannot read (`skill`,
    /// `"tools": "off"`, `skills.enbld`): the section's security keys are locked off.
    MisnamedSection,
    /// The `policy.json` holds a key twice in one object: it is dropped whole, as one that
    /// cannot be read, and every security key is locked off. A registry `REG_SZ` JSON value
    /// that does locks only its own section's security keys off.
    DuplicateKey,
}

/// The kind of policy source, never its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SourceKind {
    /// A `policy.json`.
    PolicyJson,
    /// The Windows registry.
    Registry,
    /// macOS managed preferences.
    ManagedPreferences,
}

/// One cause, and the keys it locked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockReason {
    /// The keys it locked, as [`crate::Effective::locked`] names them (`plans.` for every plan).
    pub keys: Vec<String>,
    /// What happened.
    pub cause: LockCause,
    /// Where.
    pub source: SourceKind,
    /// The section the cause is about, as the source wrote it, only when it is a plain word
    /// (`[a-z][a-z0-9_]{0,31}`): `skill`, `skills`. `None` otherwise, and for a source that
    /// cannot be read or a duplicate key, whose section is unknown.
    pub section: Option<String>,
    /// Where in the source, from 1: for a misnamed section or a malformed value, the place of
    /// its entry among the source's entries in document order (the registry's and managed
    /// preferences' values; `policy.json`'s top-level entries, so a key inside a section takes
    /// the section's place); the line of the second copy for a duplicate key in a
    /// `policy.json`; 0 for a source that cannot be read.
    pub place: u32,
}

impl LockCause {
    /// A stable code for log lines: `source_unreadable`, `malformed_value`, `misnamed_section`,
    /// `duplicate_key`.
    pub fn code(self) -> &'static str {
        match self {
            LockCause::SourceUnreadable => "source_unreadable",
            LockCause::MalformedValue => "malformed_value",
            LockCause::MisnamedSection => "misnamed_section",
            LockCause::DuplicateKey => "duplicate_key",
        }
    }
}

impl SourceKind {
    /// The kind's name: `policy.json`, `registry` or `managed preferences`.
    pub fn name(self) -> &'static str {
        match self {
            SourceKind::PolicyJson => "policy.json",
            SourceKind::Registry => registry::SOURCE,
            SourceKind::ManagedPreferences => managed_prefs::SOURCE,
        }
    }

    /// The kind of the source named `name` ([`crate::PolicySource::name`]): the registry and
    /// managed preferences by their names, any other (a `policy.json`, by its path) a file.
    pub(crate) fn of_source(name: &str) -> SourceKind {
        match name {
            registry::SOURCE => SourceKind::Registry,
            managed_prefs::SOURCE => SourceKind::ManagedPreferences,
            _ => SourceKind::PolicyJson,
        }
    }

    /// The kind of source a warning's `origin` is in.
    pub(crate) fn of_origin(origin: &Origin) -> SourceKind {
        match origin {
            Origin::File { .. } => SourceKind::PolicyJson,
            Origin::Store { name } => SourceKind::of_source(name),
        }
    }
}

/// `segment` when it is a plain word, else `None`.
pub(crate) fn shown_section(segment: &str) -> Option<String> {
    field_shaped(segment).then(|| segment.to_owned())
}

/// The reason for the security keys `keys` an unreadable source named `source` locked: a
/// duplicate key at its line, or a source that cannot be read. `None` when it locked none.
pub(crate) fn unreadable(error: &Error, source: &str, keys: Vec<&str>) -> Option<LockReason> {
    let (cause, place) = match error {
        Error::DuplicateKey { line, .. } => (
            LockCause::DuplicateKey,
            u32::try_from(*line).unwrap_or(u32::MAX),
        ),
        _ => (LockCause::SourceUnreadable, 0),
    };
    (!keys.is_empty()).then(|| LockReason {
        keys: keys.into_iter().map(str::to_owned).collect(),
        cause,
        source: SourceKind::of_source(source),
        section: None,
        place,
    })
}

/// The reason for the key `key`, whose value the source named `source` holds `place`th (from 1)
/// does not type.
pub(crate) fn malformed(key: String, source: &str, place: usize) -> LockReason {
    LockReason {
        section: shown_section(key.split('.').next().unwrap_or(&key)),
        keys: vec![key],
        cause: LockCause::MalformedValue,
        source: SourceKind::of_source(source),
        place: u32::try_from(place).unwrap_or(u32::MAX),
    }
}

/// The reasons for the keys one readable source named `source` locked without setting: each
/// misnamed name's (its keys cut to those it locked), then each other `refused` key's, a value
/// that does not type, with its place among the source's values.
pub(crate) fn of_source(
    misnamed: Vec<LockReason>,
    refused: Vec<(String, usize)>,
    source: &str,
) -> Vec<LockReason> {
    let locked = |key: &String| refused.iter().any(|(refused, _)| refused == key);
    let mut reasons: Vec<LockReason> = misnamed
        .into_iter()
        .filter_map(|mut reason| {
            reason.keys.retain(locked);
            (!reason.keys.is_empty()).then_some(reason)
        })
        .collect();
    let named: Vec<String> = reasons.iter().flat_map(|r| r.keys.clone()).collect();
    reasons.extend(
        refused
            .into_iter()
            .filter(|(key, _)| !named.contains(key))
            .map(|(key, place)| malformed(key, source, place)),
    );
    reasons
}
