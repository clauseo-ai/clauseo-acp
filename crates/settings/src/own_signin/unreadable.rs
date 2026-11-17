//! Why a read of Claude Code's settings sources is unsure, and the setting it could not read:
//! the launch's refusal is then `agent.settings_unreadable`, whose card names the setting, and
//! the log names only the kind ([`UnreadableKind::word`]).

/// The kind of trouble that made a read of Claude Code's settings sources unsure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnreadableKind {
    /// A source is there but cannot be read or is over the size cap, or its folder cannot be
    /// listed.
    Unreadable,
    /// A source does not parse: no JSON or property list, or a registry value that starts with
    /// U+FEFF.
    NotParsed,
    /// A source's `env` is no object.
    EnvNotObject,
    /// A checked name's value, or a helper key's, is of no kind Claude Code reads as text.
    NotText,
    /// A property list holds a date or data value.
    DateOrData,
    /// A policy document names a policy helper, whose output takes the policy tier's place.
    PolicyHelper,
    /// There is no home folder to find the sources in.
    NoHome,
}

impl UnreadableKind {
    /// A fixed word for the log.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::NotParsed => "not_parsed",
            Self::EnvNotObject => "env_not_object",
            Self::NotText => "not_text",
            Self::DateOrData => "date_or_data",
            Self::PolicyHelper => "policy_helper",
            Self::NoHome => "no_home",
        }
    }
}

/// Why a read is unsure: the first trouble met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Its kind.
    pub kind: UnreadableKind,
    /// The setting that could not be read, when known: a checked variable's or helper key's name
    /// from Clauseo's own list, or the path of the file (or the registry value) holding it.
    /// Never a value.
    pub setting: Option<String>,
}

/// Why a Claude launch would offer Claude Code's own sign-in to a custom endpoint
/// ([`super::own_signin_refusal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal<'a> {
    /// A reading names a custom endpoint, or a socket, and nothing turns the sign-in off.
    Endpoint,
    /// No reading names one, but a source could not be read, so one may be set.
    Unreadable(&'a Unreadable),
}

/// The registry values Claude Code reads its policy from, HKLM's then HKCU's, as the card names
/// them.
pub(super) const REGISTRY_VALUES: [&str; 2] = [
    r"HKEY_LOCAL_MACHINE\SOFTWARE\Policies\ClaudeCode\Settings",
    r"HKEY_CURRENT_USER\SOFTWARE\Policies\ClaudeCode\Settings",
];
