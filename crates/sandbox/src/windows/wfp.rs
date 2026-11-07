//! Tier B's network (S15 PR7): a pool user's agent reaches the launch's egress proxy and its
//! listed loopback ports, and nothing else.
//!
//! - **Set-up** (elevated, the setup exe's network step, [`install`]): Clauseo's WFP provider and
//!   sublayer; block filters naming every pool user (all connects, the 61080-61179 range
//!   included, ICMP, and inbound that isn't loopback); the owner's three delegation grants from
//!   PR0 (c) (`ADD_LINK` on the sublayer, `ADD` on the filter container, `ADD_LINK` on the
//!   permit's layer) and read on the provider and filters; and Windows Firewall rules that block
//!   what isn't loopback and loopback TCP outside the range. A re-run or an extend rewrites
//!   them all for the whole pool. Why the range is excepted in the firewall rules and not in the
//!   block filters: `spec.rs`.
//! - **Each launch** ([`LaunchPermit`], unelevated, as the desktop user): checks set-up's
//!   objects ([`check_setup`]) and adds, in a dynamic session, a permit from the matter's pool
//!   user to `127.0.0.1` on `Policy::proxy_ports` and `Loopback::Ports` (R24's relay port
//!   included). Dropping it ends the session and WFP deletes the permit. A missing or stale
//!   set-up refuses the launch ([`NetworkGap`]); no agent runs unfiltered.
//! - **Removal** ([`remove_network`], elevated): S15 `PR4j`'s `--remove` deletes it all.
//!
//! The browser is off on tier B (D10), so no listener rule: `Loopback::Any` is refused.
//!
//! **For the host (R17 PR-E)**, per tier-B launch, after the policy is built and before the
//! runner starts: `let permit = LaunchPermit::open(&pool_user_sid, &policy)?;`, then hold
//! `permit` until the launch's whole process tree has exited, then `permit.close()` (or drop
//! it). On `Err(Error::NetworkUnfiltered { cause })` the agent must not start;
//! `cause.needs_setup()` maps to `sandbox.setup_required` (`Error::code` does so already).
//!
//! **Residual** (0049): name resolution runs in the DNS Client service as NETWORK SERVICE, which
//! no `ALE_USER_ID` condition sees, so a pool user can still resolve names. Direct DNS on port 53
//! is blocked like all other traffic.
//!
//! The filter specs, the check and the grants are plain data, built and tested on every OS; the
//! WFP and firewall calls need Windows.

mod check;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod firewall;
#[cfg(windows)]
mod install;
#[cfg(windows)]
mod permit;
pub mod rules;
pub mod spec;

pub use check::{FoundFilter, NetworkGap, SetupReader, WfpError, check_setup};
#[cfg(windows)]
pub use install::{NetworkRemoved, install, remove};
#[cfg(windows)]
pub use permit::LaunchPermit;
pub use spec::{FirewallOutcome, Namespace, NetworkReport, NetworkSetup};

/// Removes Clauseo's network set-up ([`remove`] in [`Namespace::production`]), elevated, with
/// the grants of `owner` (the machine record's `OwnerSid`) taken back. For S15 `PR4j`'s
/// `--remove`, with every session closed; safe to run again after a partial removal.
#[cfg(windows)]
pub fn remove_network(
    owner: Option<&crate::windows::sids::Sid>,
) -> Result<NetworkRemoved, WfpError> {
    remove(&Namespace::production(), owner)
}

#[cfg(test)]
#[cfg(windows)]
mod elevated_tests;
#[cfg(test)]
mod tests;
