//! Data connectors (R24): the paid case-law connector's state as the app sees it, the hint the
//! host offers when a lawyer searches the web for Indian case law, and the API key a window
//! pastes.
//!
//! - [`ConnectorsState`] reaches every app subscriber as `HostEvent::ConnectorsChanged`, app-wide
//!   and never logged, like `SettingsChanged`. It is a view of the host's state, written and read
//!   by the same version of Clauseo, so its enums are closed, as `BrowserInstallState` is.
//! - `HostEvent::CaseLawHint` is an offer, not part of the record: never logged, so a reopened
//!   thread does not show it again.
//! - [`RedactedString`] carries the API key in `ConnectorCommand::AddApiKey`. It encodes and
//!   prints as `[redacted]`, so a command holding it can be hashed, logged or snapshotted without
//!   the key. Connector commands store no receipt (decision D2 of R24's plan).
//!
//! No token, key or account secret is in any type here: the host keeps those (S28's keychain).

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::MAX_SECRET_BYTES;

/// The text a [`RedactedString`] encodes and prints as.
pub const REDACTED_TEXT: &str = "[redacted]";

/// The connectors as the app shows them: whether the firm's switch lets any run, and each known
/// connector that is live or added. The app hides everything when `connectors` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectorsState {
    /// `tools.connectors` as it took effect.
    pub available: bool,
    /// Whether firm policy set `tools.connectors`, or an unreadable policy source locked it.
    /// With `available: false` the app shows "Turned off by your firm"; with `locked: false`, the
    /// lawyer's own off.
    pub locked: bool,
    /// A known connector when it is live; an added one also when it is not live or the liveness
    /// fetch failed, so the lawyer can still sign out or remove it.
    pub connectors: Vec<ConnectorView>,
}

/// One connector, as the Settings section, the setup card and the banner show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectorView {
    /// The connector's id, such as `case-law`.
    pub id: String,
    /// The connector's fallback name; the app shows its own words.
    pub name: String,
    /// The cheapest pack's amount in paise, GST included, such as `2_360` for ₹23.60:
    /// a number, so the app writes it by its own money rule. `None` when the connector is not
    /// live or the price list could not be read.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub price_paise: Option<u32>,
    /// The packs for the "Add credits" choice, from the price list; empty when not live.
    pub packs: Vec<PackView>,
    /// The spend view, from the last account read; `None` before one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recent_spend: Option<RecentSpend>,
    /// Where the connector stands.
    pub status: ConnectorStatus,
    /// The pay link of the last `ConnectorCommand::AddCredits`, which answers `Done`: the host has
    /// checked it is a `https://data.clauseo.chat/pay/` link, and the app opens it through the
    /// native confirm. `None` until then, and again once the recharge is paid or expires.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pay_url: Option<String>,
}

/// One pack of credits the lawyer can buy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PackView {
    /// The pack's id in the price list, which `ConnectorCommand::AddCredits` names.
    pub pack: String,
    /// The credits it adds.
    pub credits: i32,
    /// Its price in paise, GST included, such as `2_360` for ₹23.60 (200 credits). A pack
    /// whose price the host cannot read as rupees and paise is left out of the list.
    pub paise: u32,
}

/// What the account spent lately, from the last account read. The account gives credits; the
/// host prices them in paise, GST included, at the live price list's rate (`credit_inr` plus
/// `gst_rate`), each amount rounded half-up to the paise. Every `paise` is `None` when the
/// connector is not live or the price list's rate does not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RecentSpend {
    /// Credits spent today, by the connector's day.
    pub today_credits: i32,
    /// Today's spend in paise, GST included, such as `2_360` for 200 credits (₹23.60).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub today_paise: Option<u32>,
    /// Credits spent on each recent day, as the account lists them.
    pub days: Vec<DaySpendView>,
}

/// Credits spent on one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DaySpendView {
    /// The day, `YYYY-MM-DD`, as the account writes it.
    pub day: String,
    /// The credits spent that day.
    pub credits: i32,
    /// That day's spend in paise, GST included; `None` as for [`RecentSpend::today_paise`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paise: Option<u32>,
}

/// An account's balance, from the last account read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Balance {
    /// The credits left.
    pub credits: i32,
    /// Their worth in paise, GST included: the credits at the live price list's rate, rounded
    /// half-up, such as `13_924` for 1,180 credits (₹139.24 at ₹0.10 a credit plus 18% GST).
    /// `None` when the connector is not live or the price list's rate does not read. The app
    /// writes it by its own money rule.
    ///
    /// GST-inclusive from R24's spend follow-up on. Before it this was the account's
    /// `balance_inr`, which is net (before GST): a value in an older log or snapshot is net, not
    /// gross. `ConnectorsChanged` is not folded, so no schema version changes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paise: Option<u32>,
}

/// Where a connector stands. Closed: the host and the app are one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConnectorStatus {
    /// Not added: the app offers "Add".
    NotAdded,
    /// The system browser is open on the sign-in page.
    SigningIn,
    /// Signed in, or an API key added.
    SignedIn {
        /// Who is signed in; always `None` at launch, since the account names no person.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        account_label: Option<String>,
        /// The balance, from the last account read; `None` before one.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        balance: Option<Balance>,
        /// Whether the balance is under 10 search pages' worth of credits.
        low: bool,
    },
    /// The account has no credits left: calls fail until it is topped up.
    OutOfCredit {
        /// The connector's pay link for the recharge it made, when it is a checked
        /// `https://data.clauseo.chat/pay/` link; `None` otherwise.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pay_url: Option<String>,
    },
    /// The sign-in ended (signed out, or the connector revoked it): the app offers to sign in.
    SignedOut,
    /// The pasted API key was revoked.
    KeyRevoked,
    /// A sign-in or API key failed (not a cancelled one), or the sign-in could not be saved for
    /// the next start. Lasts until the next connector command.
    Error {
        /// A short English line for the details view; the app writes its own words.
        message: String,
    },
}

/// A secret a window sends once, such as a pasted API key. It decodes from a plain string of 1 to
/// [`MAX_SECRET_BYTES`] bytes, and encodes and prints as [`REDACTED_TEXT`], so a command holding
/// it can pass through a hash, a log line, a `Debug` print or a snapshot without the secret. A
/// decode error never quotes it, and the text is zeroed when dropped. TypeScript sees a plain
/// `string`. Read it with [`RedactedString::expose`] only where it is stored.
///
/// It encodes, unlike [`Secret`](crate::Secret), because `HostCommand` encodes for its receipt
/// hash; a decoded copy of an encoded one holds [`REDACTED_TEXT`], which
/// `CommandEnvelope::validate` refuses as a key.
#[derive(Clone)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct RedactedString(Zeroizing<String>);

impl RedactedString {
    /// Wraps `text`, which must be 1 to [`MAX_SECRET_BYTES`] bytes; `None` otherwise (the text is
    /// zeroed then too).
    pub fn new(text: Zeroizing<String>) -> Option<Self> {
        (1..=MAX_SECRET_BYTES)
            .contains(&text.len())
            .then(|| Self(text))
    }

    /// The text, for the one place that stores it (the keychain).
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The text, still zeroed when dropped.
    pub fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }

    /// Whether this is the text an encoded one decodes back to, not a secret.
    pub fn is_placeholder(&self) -> bool {
        *self.0 == REDACTED_TEXT
    }
}

impl PartialEq for RedactedString {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl Eq for RedactedString {}

impl fmt::Debug for RedactedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED_TEXT)
    }
}

impl Serialize for RedactedString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED_TEXT)
    }
}

impl<'de> Deserialize<'de> for RedactedString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Zeroizing::new(String::deserialize(deserializer)?);
        Self::new(text).ok_or_else(|| {
            D::Error::custom(format_args!(
                "a secret must be 1 to {MAX_SECRET_BYTES} bytes"
            ))
        })
    }
}

#[cfg(test)]
mod tests;
