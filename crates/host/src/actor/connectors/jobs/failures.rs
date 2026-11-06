//! A failed call's refusal, in fixed words, and its kind for the log.

use clauseo_connectors::{AuthError, Error as Failed, SignInError};
use clauseo_protocol::ErrorCode;

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
