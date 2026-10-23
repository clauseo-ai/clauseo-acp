//! Approvals: the agent's permission requests and elicitations, and who answers them.
//!
//! Permission requests are durable records any client may answer; the first valid answer wins.
//! The UI labels options by [`PermissionKind`]; the agent's own label is secondary text, so an
//! option named "Reject" with kind `allow_always` cannot fool the lawyer.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{CompactString, RawPayload};

open_data_enum! {
    /// Who made a call or an answer. Only the local desktop exists now; scopes come with the
    /// phone, whose principal may carry data, so a kind from a newer Clauseo decodes to
    /// `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum Principal {
        /// The desktop app on this computer.
        LocalDesktop = "local_desktop",
        _ =>
        /// A principal from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod principal with [
        Principal::LocalDesktop,
    ];
}

/// One option the agent offers in a permission request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionOption {
    /// The option's id; an answer must name one the agent offered.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub option_id: CompactString,
    /// What choosing it does; the UI's label comes from this.
    pub kind: PermissionKind,
    /// The agent's own label, shown as secondary text.
    pub agent_label: String,
}

open_unit_enum! {
    /// What a permission option does, as ACP v1 names it.
    pub enum PermissionKind {
        /// Allow this once.
        AllowOnce,
        /// Allow this and every later call like it.
        AllowAlways,
        /// Reject this once.
        RejectOnce,
        /// Reject this and every later call like it.
        RejectAlways,
    }
    test mod permission_kind;
}

open_data_enum! {
    /// How a permission request was answered.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum PermissionChoice {
        /// An offered option was chosen. The kind is copied from the offer, never narrowed.
        Selected = "selected" {
            /// The option's id.
            #[cfg_attr(feature = "specta", specta(type = String))]
            option_id: CompactString,
            /// The option's kind.
            kind: PermissionKind,
        },
        /// The request was cancelled (the turn was cancelled).
        Cancelled = "cancelled",
        _ =>
        /// An answer from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod permission_choice with [
        PermissionChoice::Selected {
            option_id: CompactString::const_new("allow"),
            kind: PermissionKind::AllowAlways,
        },
        PermissionChoice::Cancelled,
    ];
}

open_unit_enum! {
    /// Whether an answer can still reach the agent.
    pub enum Deliverable {
        /// The agent session that asked is live; an answer goes to it.
        Live,
        /// That session is gone (read back after a restart); the request can only expire.
        NotResumable,
    }
    test mod deliverable;
}

open_unit_enum! {
    /// Why a pending request expired unanswered.
    pub enum ExpiryReason {
        /// The agent process exited.
        AgentExited,
        /// The turn ended.
        TurnEnded,
        /// The host restarted.
        HostRestarted,
    }
    test mod expiry_reason;
}

open_data_enum! {
    /// What an elicitation asks for.
    #[derive(Debug, Clone, PartialEq)]
    pub enum ElicitationRequest {
        /// A form, rendered by Clauseo's own components from a JSON schema.
        Form = "form" {
            /// What the agent says about it.
            message: String,
            /// The JSON schema of the values, kept as read.
            schema: RawPayload,
        },
        /// A page the lawyer opens outside the app, after a native confirm.
        Url = "url" {
            /// What the agent says about it.
            message: String,
            /// The page's URL.
            url: String,
        },
        _ =>
        /// A request from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod elicitation_request with [
        ElicitationRequest::Form {
            message: "Your name?".to_owned(),
            schema: RawPayload::Map(
                vec![(
                    RawPayload::Str("type".into()),
                    RawPayload::Str("object".into()),
                )]
                .into(),
            ),
        },
        ElicitationRequest::Url {
            message: "Sign in".to_owned(),
            url: "https://example.com/sign-in".to_owned(),
        },
    ];
}

/// The lawyer's answer to an elicitation, sent in a command. Unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ElicitationResponse {
    /// The lawyer filled the form in.
    Accept {
        /// The values, a JSON object, kept as given.
        values: RawPayload,
    },
    /// The lawyer declined.
    Decline,
    /// The lawyer dismissed it.
    Cancel,
}

open_unit_enum! {
    /// How an elicitation ended. The values are never logged: they can hold secrets, and the
    /// agent already has them.
    pub enum ElicitationOutcome {
        /// The lawyer accepted and sent values.
        Accepted,
        /// The lawyer declined.
        Declined,
        /// The lawyer dismissed it, or the turn was cancelled.
        Cancelled,
    }
    test mod elicitation_outcome;
}
