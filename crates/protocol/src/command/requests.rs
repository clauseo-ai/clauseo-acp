//! The small closed enums a command holds: the strict twins of logged values (`RestoreRequest`,
//! `DeliveryRequest`), `RefreshLimits`' reason and a config option's value.

use serde::{Deserialize, Serialize};

use crate::{AgentChoiceId, Delivery, MatterPath, RestoreScope};

/// What a restore puts back, as a command asks for it: the strict, closed twin of the logged
/// [`RestoreScope`], with the same wire form. Unknown fields and kinds are refused, as in every
/// command; the host turns it into the scope it logs with `From`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RestoreRequest {
    /// The whole matter.
    Matter,
    /// These files only. JSON: `{"files": {"paths": [..]}}`.
    Files {
        /// The files, relative to the matter folder.
        paths: Vec<MatterPath>,
    },
}

impl From<RestoreRequest> for RestoreScope {
    fn from(request: RestoreRequest) -> Self {
        match request {
            RestoreRequest::Matter => Self::Matter,
            RestoreRequest::Files { paths } => Self::Files { paths },
        }
    }
}

/// How to deliver a prompt while a turn runs, as a command asks for it: the strict, closed twin
/// of the logged [`Delivery`], with the same wire form, so a mode from a newer client is refused
/// when the command decodes, never run as another mode. The host turns it into the delivery it
/// logs with `From`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum DeliveryRequest {
    /// Wait in the thread's queue; also what a missing field means.
    #[default]
    Queue,
    /// Cancel the running turn, then send this prompt next.
    SendNow,
    /// Add it to the running turn where the agent can, else send it now.
    Steer,
}

impl From<DeliveryRequest> for Delivery {
    fn from(request: DeliveryRequest) -> Self {
        match request {
            DeliveryRequest::Queue => Self::Queue,
            DeliveryRequest::SendNow => Self::SendNow,
            DeliveryRequest::Steer => Self::Steer,
        }
    }
}

/// Why account limits are refreshed, as `RefreshLimits` asks for it: a window got focus, or a
/// meter or the settings page opened (`Shown`), or the lawyer pressed "Try again" (`TryAgain`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RefreshWhy {
    /// A window got focus, or a meter or the settings page opened. Reads only data older than
    /// 5 minutes.
    Shown,
    /// The lawyer asked again. Reads now, unless a read is in flight.
    TryAgain,
}

/// A config option's new value, matching the option's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConfigValue {
    /// The value of one of a select option's choices.
    Select(AgentChoiceId),
    /// A boolean option's value.
    Boolean(bool),
}
