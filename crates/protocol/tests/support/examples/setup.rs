//! The install and sign-in codes of decision record 0048, each in a `TurnFailed`, which
//! `later_variants` in the parent module chains on.

use clauseo_protocol::{DomainEvent, ErrorCode, TurnFailed};

use crate::fixtures::turn_id;

/// The install and sign-in codes of decision record 0048.
fn codes() -> [ErrorCode; 11] {
    [
        ErrorCode::AgentInstallOffline,
        ErrorCode::AgentInstallBlocked,
        ErrorCode::AgentInstallDiskFull,
        ErrorCode::AgentInstallFailed,
        ErrorCode::SigninCancelled,
        ErrorCode::SigninLinkExpired,
        ErrorCode::SigninNoPlan,
        ErrorCode::SigninDeviceCodeOff,
        ErrorCode::SigninPortBusy,
        ErrorCode::SigninFailed,
        ErrorCode::SigninPolicyUnreadable,
    ]
}

/// Each of [`codes`] in a `TurnFailed` of turn 3, with the detail every setup failure has.
pub(super) fn turns() -> [DomainEvent; 11] {
    codes().map(|code| {
        TurnFailed {
            turn: turn_id(3),
            code,
            detail: Some("an install or sign-in stopped".to_owned()),
        }
        .into()
    })
}
