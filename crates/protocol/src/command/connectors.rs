//! The connector commands (R24), in one `HostCommand::Connector` variant:
//! `{"connector": {"add": {"id": "case-law"}}}`.
//!
//! None stores a receipt, as `InstallBrowser` does not: a repeated `command_id` runs the command
//! again, which each of them allows (adding a key twice stores it twice; a second sign-in starts
//! over). So `AddApiKey`'s key never reaches the receipts table, not even as part of a hash
//! (decision D2 of R24's plan). Each answers `Done` or a refusal; the state reaches app
//! subscribers as `HostEvent::ConnectorsChanged`, and `AddCredits`'s pay link as
//! [`ConnectorView::pay_url`](crate::ConnectorView::pay_url) (decision D1).
//!
//! With `tools.connectors` off the host refuses every one of them but `SignOut` and `Remove`,
//! which tidy up what is stored.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, MatterId, RedactedString};

/// Most bytes in a connector's id or a pack's id.
pub const MAX_CONNECTOR_ID_BYTES: usize = 64;

/// What a client asks of the connectors. Unknown fields and kinds are refused, as in every
/// command. A decode error quotes none of the input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConnectorCommand {
    /// The app reached the setup card or opened Settings: the host refreshes its day-old
    /// liveness cache if stale, reads the account if a connector is added (at most once a
    /// minute) and publishes the state. JSON: `{"connector": {"check": {}}}`.
    Check {},
    /// Add a connector by signing in through the system browser. Refused when the switch is
    /// off, the connector unknown, or known not live. A second one while signing in starts over.
    Add {
        /// The connector, such as `case-law`.
        id: String,
    },
    /// Add a connector with an API key the lawyer pasted, instead of signing in.
    AddApiKey {
        /// The connector.
        id: String,
        /// The key, as pasted. It encodes and prints as `[redacted]`.
        key: RedactedString,
    },
    /// Buy a pack of credits: the host makes the recharge and publishes its checked pay link in
    /// `ConnectorView::pay_url`; on payment it reads the account and publishes the balance.
    AddCredits {
        /// The connector.
        id: String,
        /// One of the connector's packs, by its id in the price list.
        pack: String,
    },
    /// Sign out of a connector: the host revokes the sign-in and forgets it. Allowed when off.
    SignOut {
        /// The connector.
        id: String,
    },
    /// Remove a connector: signed out and no longer listed as added. Allowed when off.
    Remove {
        /// The connector.
        id: String,
    },
    /// Read the account's balance again (at most once a minute) and publish it.
    RefreshBalance {
        /// The connector.
        id: String,
    },
    /// The lawyer dismissed the case-law hint in a matter: it does not show there again.
    DismissCaseLawHint {
        /// The matter.
        matter: MatterId,
    },
}

/// The one error a connector command that does not decode gives: it quotes nothing.
const UNREADABLE: &str = "a connector command that could not be read";

/// [`ConnectorCommand`]'s derived twin, which only decodes; its errors never leave this module.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    Check {},
    Add { id: String },
    AddApiKey { id: String, key: RedactedString },
    AddCredits { id: String, pack: String },
    SignOut { id: String },
    Remove { id: String },
    RefreshBalance { id: String },
    DismissCaseLawHint { matter: MatterId },
}

// Decoded through `Wire`, with every error replaced by `UNREADABLE`: serde's own messages quote
// the input, so a misshapen `add_api_key` would echo the key back to the window.
impl<'de> Deserialize<'de> for ConnectorCommand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)
            .map_err(|_quotes_input| <D::Error as serde::de::Error>::custom(UNREADABLE))?;
        Ok(match wire {
            Wire::Check {} => Self::Check {},
            Wire::Add { id } => Self::Add { id },
            Wire::AddApiKey { id, key } => Self::AddApiKey { id, key },
            Wire::AddCredits { id, pack } => Self::AddCredits { id, pack },
            Wire::SignOut { id } => Self::SignOut { id },
            Wire::Remove { id } => Self::Remove { id },
            Wire::RefreshBalance { id } => Self::RefreshBalance { id },
            Wire::DismissCaseLawHint { matter } => Self::DismissCaseLawHint { matter },
        })
    }
}

impl ConnectorCommand {
    /// The shape rules: each connector's and pack's id is 1 to [`MAX_CONNECTOR_ID_BYTES`]
    /// lowercase ASCII letters, digits, `-` or `_` (whether it names a known one is the host's
    /// check), and `AddApiKey`'s key is not `[redacted]`, which is what an encoded command
    /// decodes back to, never a key the lawyer pasted.
    pub(super) fn check(&self) -> Result<(), Error> {
        match self {
            Self::Check {} | Self::DismissCaseLawHint { matter: _ } => Ok(()),
            Self::Add { id }
            | Self::SignOut { id }
            | Self::Remove { id }
            | Self::RefreshBalance { id } => check_id(id),
            Self::AddApiKey { id, key } => {
                check_id(id)?;
                if key.is_placeholder() {
                    return Err(Error::Format {
                        kind: KIND,
                        rule: "the key is the redacted placeholder, not a key",
                    });
                }
                Ok(())
            }
            Self::AddCredits { id, pack } => check_id(id).and_then(|()| check_id(pack)),
        }
    }
}

/// The kind a shape error names.
const KIND: &str = "ConnectorCommand";

/// A connector's or a pack's id by its shape.
fn check_id(id: &str) -> Result<(), Error> {
    let shaped = (1..=MAX_CONNECTOR_ID_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if shaped {
        Ok(())
    } else {
        Err(Error::Format {
            kind: KIND,
            rule: "an id is 1 to 64 lowercase letters, digits, - or _",
        })
    }
}
