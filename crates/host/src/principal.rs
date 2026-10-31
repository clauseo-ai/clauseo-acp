//! Who may call which host entry point. Every `HostHandle` entry point calls [`authorize`]; the
//! ones that take no principal run it as the local desktop.

use clauseo_protocol::{ErrorCode, Principal};

use crate::error::Error;

/// A host entry point, as an access check names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Dispatch,
    Query,
    Subscribe,
    SubscribeEvents,
    Ack,
    RegisterFolder,
    RegisterFile,
    Shutdown,
}

/// Every `HostHandle` entry point, one each.
pub(crate) const ENTRY_POINTS: &[Access] = &[
    Access::Dispatch,
    Access::Query,
    Access::Subscribe,
    Access::SubscribeEvents,
    Access::Ack,
    Access::RegisterFolder,
    Access::RegisterFile,
    Access::Shutdown,
];

/// Whether `principal` may use `access`: the local desktop may do everything; a principal this
/// version does not know is refused with `transport.unauthorized`.
pub(crate) fn authorize(principal: &Principal, access: Access) -> Result<(), Error> {
    match principal {
        Principal::LocalDesktop => Ok(()),
        Principal::Unknown { .. } => Err(Error::refused(
            ErrorCode::TransportUnauthorized,
            format!("the principal {principal:?} may not {access:?}"),
        )),
    }
}

#[cfg(test)]
mod tests;
