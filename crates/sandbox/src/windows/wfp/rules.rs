// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/setup_provisioning/firewall.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! Set-up's Windows Firewall rules as plain data (S15 PR7), built and tested on every OS: what
//! isn't loopback blocked both ways, loopback UDP blocked, and loopback TCP blocked outside
//! [`TIER_B_PORTS`], the one place the range is excepted (`spec.rs` says why). `firewall.rs`
//! writes them on Windows.

use super::sddl::aces;
use super::spec::Namespace;
use crate::ports::TIER_B_PORTS;
use crate::windows::sids::Sid;

/// The remote addresses the firewall rules call loopback (Codex's literal).
pub const LOOPBACK_ADDRESSES: &str = "127.0.0.0/8,::/127";
/// Every remote address that isn't loopback (Codex's literal).
pub const NON_LOOPBACK_ADDRESSES: &str = "0.0.0.0-126.255.255.255,128.0.0.0-255.255.255.255,::,\
     ::2-ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff";

/// A firewall rule's direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `NET_FW_RULE_DIR_IN`.
    In,
    /// `NET_FW_RULE_DIR_OUT`.
    Out,
}

/// A firewall rule's protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleProtocol {
    /// `NET_FW_IP_PROTOCOL_ANY`.
    Any,
    /// `NET_FW_IP_PROTOCOL_TCP`.
    Tcp,
    /// `NET_FW_IP_PROTOCOL_UDP`.
    Udp,
}

/// One Windows Firewall block rule, found and kept up to date by its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    /// Its name: [`Namespace::rules`] and a suffix.
    pub name: String,
    /// Its description in Windows' tools.
    pub description: String,
    /// Its direction.
    pub direction: Direction,
    /// Its protocol.
    pub protocol: RuleProtocol,
    /// Its remote addresses.
    pub remote_addresses: &'static str,
    /// Its remote ports, when it has any; every port when not.
    pub remote_ports: Option<String>,
    /// Its `LocalUserAuthorizedList`: every pool user ([`rule_users`]).
    pub users: String,
}

/// The firewall rules: what isn't loopback, both ways; loopback UDP; loopback TCP outside
/// [`TIER_B_PORTS`], the one place the range is excepted.
pub fn rules(ns: &Namespace, pool: &[Sid]) -> Vec<RuleSpec> {
    let users = rule_users(pool);
    let rule = |suffix: &str, description: &str, direction, protocol, remote, ports| RuleSpec {
        name: format!("{}_{suffix}", ns.rules),
        description: format!("{} - {description}", ns.display),
        direction,
        protocol,
        remote_addresses: remote,
        remote_ports: ports,
        users: users.clone(),
    };
    vec![
        rule(
            "block_outbound",
            "Block Non-Loopback Outbound",
            Direction::Out,
            RuleProtocol::Any,
            NON_LOOPBACK_ADDRESSES,
            None,
        ),
        rule(
            "block_inbound",
            "Block Non-Loopback Inbound",
            Direction::In,
            RuleProtocol::Any,
            NON_LOOPBACK_ADDRESSES,
            None,
        ),
        rule(
            "block_loopback_udp",
            "Block Loopback UDP",
            Direction::Out,
            RuleProtocol::Udp,
            LOOPBACK_ADDRESSES,
            None,
        ),
        rule(
            "block_loopback_tcp",
            "Block Loopback TCP Outside 46100-46199",
            Direction::Out,
            RuleProtocol::Tcp,
            LOOPBACK_ADDRESSES,
            Some(outside_range()),
        ),
    ]
}

/// The names of every firewall rule set-up writes in `ns`: what removal deletes.
pub fn rule_names(ns: &Namespace) -> Vec<String> {
    [
        "block_outbound",
        "block_inbound",
        "block_loopback_udp",
        "block_loopback_tcp",
    ]
    .iter()
    .map(|suffix| format!("{}_{suffix}", ns.rules))
    .collect()
}

/// A firewall rule's `LocalUserAuthorizedList`: one allow per SID, as Codex writes it.
pub fn rule_users(sids: &[Sid]) -> String {
    format!("O:LSD:{}", aces('A', sids))
}

/// Every TCP port outside [`TIER_B_PORTS`], as the firewall writes port ranges.
pub fn outside_range() -> String {
    format!(
        "1-{},{}-65535",
        TIER_B_PORTS.start() - 1,
        TIER_B_PORTS.end() + 1
    )
}

/// The words for firm IT when Group Policy keeps local firewall rules from taking effect.
pub const FIREWALL_UNDER_POLICY: &str = "Group Policy keeps this computer's own firewall rules \
     from taking effect, so the second layer of Clauseo's network protection is off; its network \
     filters still enforce. IT can add the same rules by Group Policy, or let local rules apply \
     (AllowLocalPolicyMerge)";

/// How the second layer's loss ends, in every cause's words.
const STILL_ENFORCED: &str = "the second layer of Clauseo's network protection is off; its \
     network filters still enforce";

/// HRESULTs a firewall call gives when the Windows Defender Firewall service isn't running:
/// `ERROR_SERVICE_NOT_ACTIVE`, `EPT_S_NOT_REGISTERED`, `RPC_S_SERVER_UNAVAILABLE`.
const SERVICE_STOPPED: [u32; 3] = [0x8007_0426, 0x8007_06D9, 0x8007_06BA];

/// Why the firewall rules don't all take effect, each named for firm IT. None fails the set-up:
/// WFP's filters, which did succeed, are the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallTrouble {
    /// Group Policy keeps local rules from taking effect; what `LocalPolicyModifyState` said.
    GroupPolicy(String),
    /// The Windows Defender Firewall service (`mpssvc`) isn't running.
    ServiceStopped,
    /// Windows Firewall is off for a current network profile, as when another firewall product
    /// owns the computer's firewall.
    TurnedOff,
    /// A firewall call failed for another reason: which, and its code.
    Com {
        /// The call, or what it was for.
        what: String,
        /// Its HRESULT.
        code: u32,
    },
    /// A rule's `LocalUserAuthorizedList` read back as other than exactly the pool.
    RuleUsers {
        /// The rule's name.
        rule: String,
        /// What it read back as.
        read_back: String,
    },
}

impl FirewallTrouble {
    /// A failed firewall call `what` with HRESULT `code`: [`FirewallTrouble::ServiceStopped`]
    /// when the code says the service isn't running.
    pub fn from_call(what: &str, code: u32) -> FirewallTrouble {
        if SERVICE_STOPPED.contains(&code) {
            FirewallTrouble::ServiceStopped
        } else {
            FirewallTrouble::Com {
                what: what.to_owned(),
                code,
            }
        }
    }

    /// The words for firm IT.
    pub fn words(&self) -> String {
        match self {
            FirewallTrouble::GroupPolicy(detail) => format!("{FIREWALL_UNDER_POLICY} ({detail})"),
            FirewallTrouble::ServiceStopped => format!(
                "the Windows Defender Firewall service (mpssvc) isn't running, so \
                 {STILL_ENFORCED}. IT can set the service to start automatically"
            ),
            FirewallTrouble::TurnedOff => format!(
                "Windows Firewall is turned off for the current network, as it is when another \
                 firewall product owns this computer's firewall, so {STILL_ENFORCED}. IT can add \
                 the same rules in that firewall, or turn Windows Firewall back on"
            ),
            FirewallTrouble::Com { what, code } => {
                format!("a firewall call failed ({what}, error {code:#x}), so {STILL_ENFORCED}")
            }
            FirewallTrouble::RuleUsers { rule, read_back } => format!(
                "the firewall rule {rule} reads back with other users than the pool's \
                 ({read_back}), so {STILL_ENFORCED}"
            ),
        }
    }
}

/// What set-up's firewall step came to. The firewall is the second layer, so neither fails the
/// set-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallOutcome {
    /// Every rule is written, and local rules take effect in every current profile.
    Applied,
    /// The rules don't all take effect, and why, each cause in [`FirewallTrouble::words`]:
    /// Group Policy, the service stopped, the firewall turned off, or a failed call.
    Ineffective(String),
}
