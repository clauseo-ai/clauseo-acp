//! A launch's fail-closed check of set-up's network objects (S15 PR7), with no Windows call:
//! before a launch adds its permit, set-up's provider, sublayer and every block filter must be
//! there, unchanged, and naming the launch's pool user. Anything else refuses the launch with a
//! named cause ([`NetworkGap`]), so no agent ever runs unfiltered. The reads go through
//! [`SetupReader`]; `engine.rs` reads WFP itself.

use super::spec::{Layer, Namespace, block_filters, sddl_users};
use crate::windows::sids::Sid;

/// Why a tier-B launch can't get its network filtered, so it is refused. Variants are appended
/// from here on: the host maps each (R17 PR-E).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetworkGap {
    /// Set-up's provider or sublayer isn't there: set-up's network step never ran, or the
    /// set-up was removed.
    #[error("the sandbox's network filters aren't set up")]
    NotSetUp,
    /// One of set-up's block filters is missing or not as set-up wrote it; its name.
    #[error("the sandbox's network filter {0} is missing or changed")]
    FilterMissing(String),
    /// Set-up's block filters don't name the launch's pool user.
    #[error("the sandbox's network filters don't cover this matter's sandbox user")]
    PoolUserNotCovered,
    /// Set-up's objects couldn't be read: the owner's read grant is missing, or WFP failed.
    #[error("the sandbox's network filters couldn't be read ({call} failed, error {code:#x})")]
    Unreadable {
        /// The call, as Windows names it.
        call: &'static str,
        /// Its result code.
        code: u32,
    },
    /// The policy asks for any loopback port, which tier B never permits (D10: the browser is
    /// off on tier B).
    #[error("the sandbox can't give an agent every loopback port on this computer")]
    LoopbackAny,
    /// The policy names a loopback port outside 61080-61179, which the firewall blocks.
    #[error("the sandbox can't permit loopback port {0}, outside 61080-61179")]
    PortOutsideRange(u16),
    /// The launch's permit couldn't be added.
    #[error("the sandbox couldn't permit this agent's network ({call} failed, error {code:#x})")]
    PermitFailed {
        /// The call, as Windows names it.
        call: &'static str,
        /// Its result code.
        code: u32,
    },
}

impl NetworkGap {
    /// Whether running set-up again fixes it (`sandbox.setup_required`): set-up's objects are
    /// missing, changed, unreadable, or don't cover the pool user. The rest are the launch's
    /// own failures (`agent.launch_failed`).
    pub fn needs_setup(&self) -> bool {
        matches!(
            self,
            NetworkGap::NotSetUp
                | NetworkGap::FilterMissing(_)
                | NetworkGap::PoolUserNotCovered
                | NetworkGap::Unreadable { .. }
        )
    }
}

/// A WFP call that failed: the call, as Windows names it, and its result code. What set-up's
/// network step and its removal return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{call} failed (error {code:#x})")]
pub struct WfpError {
    /// The call.
    pub call: &'static str,
    /// Its result code.
    pub code: u32,
}

/// Result codes WFP's calls give when the Base Filtering Engine isn't running
/// (`EPT_S_NOT_REGISTERED`, `RPC_S_SERVER_UNAVAILABLE`, `ERROR_SERVICE_NOT_ACTIVE`), as Win32
/// codes and as HRESULTs.
const BFE_DOWN: [u32; 6] = [1753, 1722, 1062, 0x8007_06D9, 0x8007_06BA, 0x8007_0426];

impl WfpError {
    /// The words for set-up's cause: the Base Filtering Engine named when it isn't running,
    /// else the call and its code. Either fails the set-up: WFP is the boundary.
    pub fn cause(&self) -> String {
        if BFE_DOWN.contains(&self.code) {
            format!(
                "the Base Filtering Engine service (BFE) isn't running, and Clauseo's network \
                 protection needs it: IT can set it to start automatically ({self})"
            )
        } else {
            self.to_string()
        }
    }
}

/// One of set-up's block filters as WFP has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFilter {
    /// Its layer; `None` for a layer that isn't one of ours.
    pub layer: Option<Layer>,
    /// Whether its action is block.
    pub block: bool,
    /// Its sublayer's key.
    pub sublayer: u128,
    /// Its `ALE_USER_ID` condition's descriptor, as SDDL; `None` when it has none.
    pub users: Option<String>,
}

/// What a launch reads of set-up's objects. A read that fails is `Err((call, code))`.
pub trait SetupReader {
    /// Whether the provider `key` is there.
    fn provider(&mut self, key: u128) -> Result<bool, (&'static str, u32)>;
    /// Whether the sublayer `key` is there.
    fn sublayer(&mut self, key: u128) -> Result<bool, (&'static str, u32)>;
    /// The filter `key`, when it is there.
    fn filter(&mut self, key: u128) -> Result<Option<FoundFilter>, (&'static str, u32)>;
}

/// Checks set-up's objects in `ns` for a launch of `user`: the provider and the sublayer are
/// there, and each block filter is there, at its layer, blocking, in our sublayer, with an
/// `ALE_USER_ID` descriptor naming `user` and holding nothing [`sddl_users`] doesn't read.
pub fn check_setup(
    ns: &Namespace,
    user: &Sid,
    reader: &mut impl SetupReader,
) -> Result<(), NetworkGap> {
    let unreadable = |(call, code)| NetworkGap::Unreadable { call, code };
    if !reader.provider(ns.provider).map_err(unreadable)?
        || !reader.sublayer(ns.sublayer).map_err(unreadable)?
    {
        return Err(NetworkGap::NotSetUp);
    }
    for spec in block_filters(ns, std::slice::from_ref(user)) {
        let Some(key) = spec.key else { continue };
        let found = reader.filter(key).map_err(unreadable)?;
        let Some(found) = found else {
            return Err(NetworkGap::FilterMissing(spec.name));
        };
        if found.layer != Some(spec.layer) || !found.block || found.sublayer != ns.sublayer {
            return Err(NetworkGap::FilterMissing(spec.name));
        }
        let users = found.users.as_deref().and_then(sddl_users);
        if !users.is_some_and(|users| users.contains(user)) {
            return Err(NetworkGap::PoolUserNotCovered);
        }
    }
    Ok(())
}
