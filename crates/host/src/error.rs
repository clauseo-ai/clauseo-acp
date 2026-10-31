//! The host's one error type: every failure of a host call, with the code a client sees.

use clauseo_protocol::{ErrorCode, HostError};

/// Why a host call failed. [`Error::code`] gives the client's code; `From<Error> for HostError`
/// gives the wire form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A command or query broke a shape rule (`command.invalid`).
    #[error("invalid input: {0}")]
    Invalid(#[from] clauseo_protocol::Error),
    /// A state check refused the call, such as `thread.turn_running`; the code is kept as it is.
    #[error("refused: {0}")]
    Refused(#[from] HostError),
    /// The reducer refused an event: a host bug (`internal`).
    #[error("reducer: {0}")]
    Reducer(#[from] clauseo_reducer::Error),
    /// The store failed; the code is the store's.
    #[error("store: {0}")]
    Store(#[from] clauseo_store::Error),
    /// A matter operation failed; the code is the matter crate's.
    #[error("matter: {0}")]
    Matters(#[from] clauseo_matters::Error),
    /// The agent did not start (`agent.launch_failed`).
    #[error("process: {0}")]
    Process(#[from] clauseo_process::Error),
    /// An ACP call failed; the code is the ACP crate's.
    #[error("acp: {0}")]
    Acp(#[from] clauseo_acp::Error),
    /// Settings failed (`internal`).
    #[error("settings: {0}")]
    Settings(#[from] clauseo_settings::Error),
    /// The host stopped or panicked, so the call has no answer (`internal`).
    #[error("the host is closed")]
    Closed,
    /// A host bug, with a detail for logs (`internal`).
    #[error("internal: {0}")]
    Internal(String),
}

impl Error {
    /// The code a client sees for this failure.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Invalid(_) => ErrorCode::CommandInvalid,
            Self::Refused(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::Matters(error) => error.code(),
            Self::Process(error) => error.code(),
            Self::Acp(error) => error.code(),
            Self::Reducer(_) | Self::Settings(_) | Self::Closed | Self::Internal(_) => {
                ErrorCode::Internal
            }
        }
    }

    /// A state check's refusal with `code` and a detail for logs.
    pub(crate) fn refused(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Refused(HostError::new(code, message))
    }
}

/// A reply channel whose sender was dropped: the host thread is gone.
impl From<futures::channel::oneshot::Canceled> for Error {
    fn from(_: futures::channel::oneshot::Canceled) -> Self {
        Self::Closed
    }
}

/// The wire form: the code, the `Display` text as the message, `retryable` from the code. A
/// refusal is passed on as it was made.
impl From<Error> for HostError {
    fn from(error: Error) -> Self {
        match error {
            Error::Refused(refusal) => refusal,
            Error::Invalid(invalid) => HostError::invalid_command(invalid),
            Error::Reducer(_)
            | Error::Store(_)
            | Error::Matters(_)
            | Error::Process(_)
            | Error::Acp(_)
            | Error::Settings(_)
            | Error::Closed
            | Error::Internal(_) => HostError::new(error.code(), error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
