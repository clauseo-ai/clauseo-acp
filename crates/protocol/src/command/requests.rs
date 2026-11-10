//! The strict, closed twins of logged types that commands carry: what a restore puts back and
//! how a prompt is delivered while a turn runs. Each has the logged type's wire form.

use serde::{Deserialize, Serialize};

use crate::{Delivery, MatterPath, RestoreScope};

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
