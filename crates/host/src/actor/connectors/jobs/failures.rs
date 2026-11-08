//! A failed call's refusal, in fixed words, and its kind for the log; and how a settled
//! recharge ended, for the state.

use clauseo_connectors::{AuthError, Error as Failed, RechargeStatus, SignInError};
use clauseo_protocol::{ErrorCode, RechargeOutcome};

use super::Refusal;

/// A refusal for a failed call, in fixed words.
pub(super) fn refusal(error: &Failed) -> Refusal {
    match error {
        Failed::BadKeyFormat => Refusal::new(ErrorCode::CommandInvalid, "not a connector API key"),
        Failed::Auth(AuthError::KeyRevoked) => {
            Refusal::new(ErrorCode::SigninFailed, "the connector refused the API key")
        }
        Failed::Auth(_) => Refusal::new(ErrorCode::SigninFailed, "the connector's sign-in ended"),
        Failed::Network(_) => {
            Refusal::new(ErrorCode::Internal, "the connector could not be reached")
        }
        Failed::Store(_) => Refusal::new(ErrorCode::Internal, "the keychain refused the sign-in"),
        Failed::Http { .. }
        | Failed::Parse(_)
        | Failed::InsufficientCredits(_)
        | Failed::NotOurPayLink
        | Failed::RechargeTimedOut
        | Failed::Cancelled
        | Failed::BadOrigin => {
            Refusal::new(ErrorCode::Internal, "the connector's answer was not usable")
        }
    }
}

/// A failed call's kind, for the log.
pub(in crate::actor::connectors) fn kind(error: &Failed) -> &'static str {
    match error {
        Failed::Auth(_) => "auth",
        Failed::Http { .. } => "http",
        Failed::Network(_) => "network",
        Failed::Parse(_) => "parse",
        Failed::InsufficientCredits(_) => "out_of_credit",
        Failed::NotOurPayLink => "not_our_pay_link",
        Failed::BadKeyFormat => "bad_key_format",
        Failed::RechargeTimedOut => "recharge_timed_out",
        Failed::Cancelled => "cancelled",
        Failed::Store(_) => "store",
        Failed::BadOrigin => "bad_origin",
    }
}

/// A failed sign-in's kind, for the log.
pub(super) fn sign_in_kind(error: &SignInError) -> &'static str {
    match error {
        SignInError::Discovery(_) => "discovery",
        SignInError::Cancelled => "cancelled",
        SignInError::TimedOut => "timed_out",
        SignInError::StateMismatch => "state_mismatch",
        SignInError::IssuerMismatch => "issuer_mismatch",
        SignInError::Exchange(_) => "exchange",
        SignInError::Store(_) => "store",
        SignInError::Busy => "busy",
    }
}

/// How a settled recharge ended, by the connector's status: `None` for one the host cannot read
/// (logged without its text).
pub(super) fn recharge_outcome(status: &RechargeStatus) -> Option<RechargeOutcome> {
    match status {
        RechargeStatus::Paid => Some(RechargeOutcome::Paid),
        RechargeStatus::Other(other) if other == "expired" => Some(RechargeOutcome::Expired),
        RechargeStatus::Other(other) if other == "cancelled" || other == "canceled" => {
            Some(RechargeOutcome::Cancelled)
        }
        RechargeStatus::AwaitingPayment | RechargeStatus::Other(_) => {
            tracing::info!(
                connector = "case-law",
                "a recharge ended in a status the host does not know"
            );
            None
        }
    }
}
