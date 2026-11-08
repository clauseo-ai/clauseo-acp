//! Tier B's network (S15 PR7): a pool user's agent reaches `127.0.0.1` on 46100-46199
//! (`ports::TIER_B_PORTS`), where its launch's egress proxy and listed ports are, and nothing
//! else. Set-up and each launch check the machine's dynamic and excluded port ranges against
//! it, and each launch the listeners in it ([`check_ports`]).
//!
//! - **Set-up** (elevated, the setup exe's network step, [`install`]): Clauseo's WFP provider and
//!   sublayer; block filters that catch every token holding a pool user's or the pool group's
//!   SID, enabled or deny-only (all connects, the range included, ICMP, and inbound that isn't
//!   loopback); one static permit per pool user for `127.0.0.1` TCP on the range; read-only
//!   grants for the owner; and Windows Firewall rules that block what isn't loopback and
//!   loopback TCP outside the range. A re-run or an extend empties our sublayer and rewrites it
//!   for the whole pool, deleting any filter set-up didn't write.
//! - **Each launch** ([`check_launch`], unelevated, as the desktop user) checks set-up's objects
//!   ([`check_setup`]): exactly set-up's filters in our sublayer, catching the launch's pool user,
//!   with its static permit. Anything else refuses the launch ([`NetworkGap`]); no agent runs
//!   unfiltered. Nothing is added per launch: within the range, each listener's per-launch
//!   secret keeps matters apart.
//! - **Removal** ([`remove_network`], elevated): S15 `PR4j`'s `--remove` deletes it all.
//!
//! The browser is off on tier B (D10), so no listener rule: `Loopback::Any` is refused.
//!
//! **For the host (R17 PR-E)**, per tier-B launch, after the policy is built and before the
//! runner starts: `check_launch(&pool_user_sid, &policy)?`. On
//! `Err(Error::NetworkUnfiltered { cause })` the agent must not start; `Error::code` maps
//! `cause.needs_setup()` to `sandbox.setup_required`, the rest to `agent.launch_failed`.
//!
//! **Residuals** (0049): name resolution runs in the DNS Client service as `NETWORK SERVICE`, SMB
//! in the System process and `WebDAV` in the `WebClient` service as `LOCAL SERVICE`, none of which
//! an `ALE_USER_ID` condition sees. Agent-made listeners in the range are a collusion channel.
//!
//! The filter specs, the check and the grants are plain data, built and tested on every OS; the
//! WFP and firewall calls need Windows.

mod check;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod firewall;
#[cfg(windows)]
mod grant_probe;
#[cfg(windows)]
mod install;
#[cfg(windows)]
mod os_ports;
#[cfg(windows)]
mod permit;
mod port_check;
pub mod rules;
mod sddl;
mod slot_filters;
pub mod spec;
#[cfg(any(windows, test))]
mod writes;

pub use check::{
    FoundCondition, FoundFilter, Kind, NetworkGap, SetupReader, WfpError, check_setup, classify,
};
#[cfg(windows)]
pub use grant_probe::{GrantProbe, foreign_test_filter, probe_owner_grants};
#[cfg(windows)]
pub use install::{NetworkRemoved, install, remove};
#[cfg(windows)]
pub use os_ports::{check_ports, dynamic_port_ranges};
#[cfg(windows)]
pub(crate) use os_ports::{listener_rows, ports_in_use, process_in_job};
#[cfg(windows)]
pub use permit::{check_launch, check_launch_in, check_slot};
pub use port_check::{Listener, PortsSeen, dynamic_span, is_ours, ports_usable};
pub use spec::{FirewallOutcome, FirewallTrouble, Namespace, NetworkReport, NetworkSetup};

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
