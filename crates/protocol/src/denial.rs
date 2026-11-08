//! Denials and widening (R18): what the jail or the egress proxy refused, the lawyer's answers
//! to the native dialogs, and the answers a matter remembers.
//!
//! Logged (in `DenialRecorded`, `EscalationAnswered` and `AllowanceRecorded`), so every enum
//! here is open: a newer Clauseo's kind, source, answer, target or mode decodes to `Unknown`.
//! [`EscalationRequest`] is a command's, so it is closed, as every command's enum is.
//!
//! A path here is display text only, the host's canonical form, shown bidi-isolated; commands
//! never name one. A single file is its own kind (`File`), beside a folder (`Folder`): a file
//! refused directly in a place the host never widens whole (the home folder, another matter)
//! gets a card for that one file, read-only.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{AllowanceId, DenialId, MatterId, ThreadId, Timestamp, ToolCallId};

/// Another matter, as a denial or a read grant names it: its id and its name when the host
/// logged it (display only, shown bidi-isolated; the name may have changed since).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OtherMatter {
    /// The matter.
    pub matter: MatterId,
    /// Its name.
    pub name: String,
}

open_data_enum! {
    /// What was refused. A kind from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DenialKind {
        /// A folder outside the matter: the refused path's folder.
        Folder = "folder" {
            /// The folder.
            path: String,
            /// The other matter the folder is in, when it is in one.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            other_matter: Option<OtherMatter>,
        },
        /// One file, where the host offers that file alone: a file directly in a folder it
        /// never widens whole (the home folder), or another matter's file.
        File = "file" {
            /// The file.
            path: String,
            /// The other matter the file is in, when it is in one.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            other_matter: Option<OtherMatter>,
        },
        /// A site the egress proxy refused.
        Site = "site" {
            /// The exact host.
            host: String,
            /// Whether the host is a shared-hosting one, which gets a stronger warning line.
            #[serde(default)]
            shared_hosting: bool,
        },
        /// The network's own proxy asked for a sign-in Clauseo can't give: a line, no card.
        UpstreamAuth = "upstream_auth" {
            /// The upstream proxy's host.
            upstream: String,
        },
        _ =>
        /// A kind from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod denial_kind with [
        DenialKind::Folder { path: "/Users/a/Deals".into(), other_matter: None },
        DenialKind::File { path: "/Users/a/notes.docx".into(), other_matter: None },
        DenialKind::Site { host: "example.com".into(), shared_hosting: false },
        DenialKind::UpstreamAuth { upstream: "proxy.firm.example".into() },
    ];
}

open_data_enum! {
    /// Who reported the refusal. A source from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DenialSource {
        /// The OS sandbox's own report (the Seatbelt log on macOS).
        Sandbox = "sandbox",
        /// The egress proxy.
        Proxy = "proxy",
        /// The agent's tool call failed with access denied on a path outside the jail, which
        /// the host read as a refusal (Windows, where the sandbox reports no refused paths).
        ToolCall = "tool_call" {
            /// The tool call.
            tool_call: ToolCallId,
        },
        _ =>
        /// A source from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod denial_source with [
        DenialSource::Sandbox,
        DenialSource::Proxy,
        DenialSource::ToolCall {
            tool_call: ToolCallId::new(uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0007)).unwrap(),
        },
    ];
}

open_unit_enum! {
    /// The lawyer's answer to a native dialog. Esc or closing the dialog is `Refuse`. An answer
    /// from a newer Clauseo decodes to `Unknown`, which a reader treats as a refusal.
    pub enum EscalationAnswer {
        /// Allowed for this time: until the turn ends.
        AllowOnce,
        /// Allowed, and remembered for the matter.
        AllowForMatter,
        /// Refused.
        Refuse,
        /// Refused, and remembered for the matter: no card for it again.
        RefuseForMatter,
    }
    test mod escalation_answer;
}

open_data_enum! {
    /// What a remembered answer covers. A target from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AllowTarget {
        /// A folder outside every matter.
        Folder = "folder" {
            /// The folder.
            path: String,
        },
        /// One file outside every matter.
        File = "file" {
            /// The file.
            path: String,
        },
        /// A folder or a file of another matter, never the whole matter.
        OtherMatterPath = "other_matter_path" {
            /// The other matter.
            matter: MatterId,
            /// The folder or file.
            path: String,
            /// Whether `path` is a file. Required: a missing field would read as a folder, the
            /// wider grant, so an entry without it does not decode.
            is_file: bool,
        },
        /// The exact host of a site.
        Site = "site" {
            /// The host.
            host: String,
        },
        _ =>
        /// A target from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod allow_target with [
        AllowTarget::Folder { path: "/Users/a/Deals".into() },
        AllowTarget::File { path: "/Users/a/notes.docx".into() },
        AllowTarget::OtherMatterPath {
            matter: MatterId::new(uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002)).unwrap(),
            path: "/Users/a/Matters/Smith/brief.docx".into(),
            is_file: true,
        },
        AllowTarget::Site { host: "example.com".into() },
    ];
}

open_unit_enum! {
    /// How far a remembered allow reaches. Read-only only: Clauseo never offers to write
    /// outside the matter. A mode from a newer Clauseo decodes to `Unknown`, which a reader
    /// treats as nothing granted.
    pub enum GrantMode {
        /// The agent may read, never write.
        ReadOnly,
    }
    test mod grant_mode;
}

/// One remembered answer, as `HostQuery::ListAllowances` and a matter's header list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterAllowance {
    /// The matter that remembers it.
    pub matter: MatterId,
    /// Its id, for `ForgetAllowance`.
    pub allowance: AllowanceId,
    /// What it covers.
    pub target: AllowTarget,
    /// How far an allow reaches.
    pub mode: GrantMode,
    /// Allowed (`true`) or refused (`false`).
    pub allowed: bool,
    /// When it was recorded (display only; the `AllowanceRecorded` envelope's `ts`).
    pub at: Timestamp,
}

/// What `RequestEscalation` asks the host to put to the lawyer in a native dialog. It grants
/// nothing itself: the dialog's answer does. Closed, as every command's enum is, so a kind from
/// a newer window is refused as the command decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum EscalationRequest {
    /// A denial's card: "Allow access to this folder?" or "Allow this site?". Only a denial
    /// logged with `offered: true` and still pending; else `escalation.not_pending`, or
    /// `escalation.not_offerable`. `thread` names the thread whose card it is, so the host
    /// checks the thread before it looks the denial up. JSON:
    /// `{"denial": {"thread": "<id>", "denial": "<id>"}}`.
    Denial {
        /// The thread whose card it is.
        thread: ThreadId,
        /// The denial.
        denial: DenialId,
    },
    /// Switch protection off for `matter`: its agents run without the Safe workspace from their
    /// next launch. Refused before any dialog with `sandbox.required` or
    /// `sandbox.unprotected_auto_forbidden` when the firm's settings forbid it.
    SwitchProtectionOff {
        /// The matter.
        matter: MatterId,
    },
    /// "Use files from another matter": the host opens a native picker at `from`'s folder,
    /// then the dialog, and `matter`'s agents may read what was picked, read-only. The window
    /// picks `from` from its own list of matters. A picked path the rules refuse is
    /// `escalation.not_offerable`.
    FilesFromOtherMatter {
        /// The matter whose agents would read.
        matter: MatterId,
        /// The other matter, whose files they would read.
        from: MatterId,
    },
}
