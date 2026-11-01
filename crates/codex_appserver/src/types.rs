//! The app-server messages this crate mirrors, each named as its definition in the vendored
//! schema (`schema/<tag>/`, the tag being `SCHEMA_TAG` in `tests/schema_drift/checker.rs`).
//! Field names, and which fields are required, follow the schema exactly; `tests/schema_drift.rs`
//! fails when they stop matching.
//!
//! Decoding is lenient where the schema can grow: an unknown field is ignored, and an unknown
//! value of any enum here decodes to that enum's `Unknown` variant, which keeps the string. A
//! field the schema marks optional is `Option` (absent and `null` both read as `None`) and is
//! left out when encoding. A required field that may be `null` must still be present.

mod account;
mod initialize;
mod rate_limits;

pub use account::{
    Account, AmazonBedrockAccount, ChatgptAccount, GetAccountParams, GetAccountResponse, PlanType,
};
pub use initialize::{ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse};
pub use rate_limits::{
    AccountRateLimitsUpdatedNotification, CreditsSnapshot, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, RateLimitReachedType, RateLimitResetCredit,
    RateLimitResetCreditStatus, RateLimitResetCreditsSummary, RateLimitResetType,
    RateLimitSnapshot, RateLimitWindow, SpendControlLimitSnapshot,
};

/// Decodes a field the schema lists as required but lets be `null`. A field with
/// `deserialize_with` and no `default` is required by serde, so a missing one fails to decode.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}

/// Declares an enum of string values that keeps the ones it does not name: each listed value
/// gets a variant, and any other string decodes to `Unknown`, which keeps it. `VALUES` lists the
/// named values in the schema's order, for the drift test. Doc comments pass through, and no other
/// attribute is accepted: the expect-allowlist check refuses an attribute whose name comes from a
/// metavariable.
macro_rules! open_enum {
    (
        $(#[doc = $doc:expr])*
        $name:ident {
            $( $(#[doc = $variant_doc:expr])* $variant:ident = $value:literal, )+
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone)]
        pub enum $name {
            $( $(#[doc = $variant_doc])* $variant, )+
            /// A value this crate does not name, kept as it was sent. Decoding and
            /// [`Self::from_wire`] never put a named value here; one built by hand compares
            /// equal to its named variant, since equality is by wire value.
            Unknown(String),
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.as_str() == other.as_str()
            }
        }

        impl Eq for $name {}

        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.as_str().hash(state);
            }
        }

        impl $name {
            /// Every value the vendored schema lists, in its order.
            pub const VALUES: &'static [&'static str] = &[$($value),+];

            /// The value as it appears on the wire.
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $value, )+
                    Self::Unknown(value) => value,
                }
            }

            /// The variant for a wire value; a value this crate does not name becomes
            /// `Unknown`.
            pub fn from_wire(value: &str) -> Self {
                match value {
                    $( $value => Self::$variant, )+
                    other => Self::Unknown(other.to_owned()),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self::from_wire(&value))
            }
        }
    };
}

use open_enum;
