// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/setup_provisioning/firewall.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! Set-up's Windows Firewall rules (S15 PR7), through `INetFwPolicy2`, after Codex's
//! `setup_provisioning/firewall.rs` (`PROVENANCE.md`): each rule found by its name, made when
//! missing, every field written again on each run so a re-run or an extend leaves it current,
//! and its `LocalUserAuthorizedList` read back to check every pool user is named.
//!
//! The firewall is the second layer; WFP's filters enforce without it. So nothing here fails the
//! set-up: when Group Policy keeps local rules from taking effect, or a rule can't be written,
//! the outcome says so ([`FirewallOutcome::Ineffective`]) and set-up goes on.

#![expect(unsafe_code, reason = "FFI to the Windows Firewall COM interfaces")]

use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_OK, VARIANT_TRUE};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule3, INetFwRules, NET_FW_ACTION_BLOCK, NET_FW_IP_PROTOCOL_ANY,
    NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_PROTOCOL_UDP, NET_FW_MODIFY_STATE, NET_FW_MODIFY_STATE_OK,
    NET_FW_PROFILE2_ALL, NET_FW_RULE_DIR_IN, NET_FW_RULE_DIR_OUT, NetFwPolicy2, NetFwRule,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::core::{BSTR, Interface};

use super::spec::{Direction, FIREWALL_UNDER_POLICY, FirewallOutcome, RuleProtocol, RuleSpec};

/// COM initialised on this thread for the firewall calls; uninitialised on drop when this
/// initialised it (a thread already in another apartment keeps it).
struct Apartment {
    ours: bool,
}

impl Apartment {
    fn enter() -> Result<Apartment, String> {
        // SAFETY: no reserved pointer; balanced by Drop when it succeeds.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_err() && result != RPC_E_CHANGED_MODE {
            return Err(format!("CoInitializeEx failed ({result:?})"));
        }
        Ok(Apartment {
            ours: result.is_ok(),
        })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.ours {
            // SAFETY: balances this thread's successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

/// The firewall's rule list, with COM held for it.
fn policy() -> Result<(Apartment, INetFwPolicy2), String> {
    let apartment = Apartment::enter()?;
    // SAFETY: COM is initialised on this thread; the class and interface are the firewall's.
    let policy: INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("the firewall's policy can't be opened ({error})"))?;
    Ok((apartment, policy))
}

/// Writes every rule of `rules`. `Applied` when each is written and local rules take effect in
/// every current profile; else why not.
pub(super) fn ensure(rules: &[RuleSpec]) -> FirewallOutcome {
    let (_apartment, policy) = match policy() {
        Ok(opened) => opened,
        Err(why) => return FirewallOutcome::Ineffective(why),
    };
    let mut why = Vec::new();
    if let Err(policy_why) = local_rules_apply(&policy) {
        why.push(policy_why);
    }
    // SAFETY: `policy` is a live firewall policy on this thread.
    match unsafe { policy.Rules() } {
        Ok(list) => {
            for rule in rules {
                if let Err(rule_why) = ensure_rule(&list, rule) {
                    why.push(format!("{}: {rule_why}", rule.name));
                }
            }
        }
        Err(error) => why.push(format!("the firewall's rules can't be read ({error})")),
    }
    if why.is_empty() {
        FirewallOutcome::Applied
    } else {
        FirewallOutcome::Ineffective(why.join("; "))
    }
}

/// Removes each rule named in `names` that is there; why for the first that can't be removed.
pub(super) fn remove(names: &[String]) -> Result<(), String> {
    let (_apartment, policy) = policy()?;
    // SAFETY: `policy` is a live firewall policy on this thread.
    let list = unsafe { policy.Rules() }
        .map_err(|error| format!("the firewall's rules can't be read ({error})"))?;
    for name in names {
        let name = BSTR::from(name.as_str());
        // SAFETY: `list` is live; `name` is a BSTR for the calls.
        if unsafe { list.Item(&name) }.is_ok() {
            // SAFETY: as above.
            unsafe { list.Remove(&name) }
                .map_err(|error| format!("the firewall rule {name} can't be removed ({error})"))?;
        }
    }
    Ok(())
}

/// `Ok` when local rules take effect in every current profile; why not otherwise (Group Policy
/// ignores them, or answers for only some profiles).
fn local_rules_apply(policy: &INetFwPolicy2) -> Result<(), String> {
    let mut state = NET_FW_MODIFY_STATE::default();
    // The wrapper drops `S_FALSE` (an answer for only some profiles), so the call goes through
    // the vtable, as Codex's does.
    // SAFETY: `policy` is a live interface; `state` is a live out-pointer.
    let result = unsafe {
        (Interface::vtable(policy).LocalPolicyModifyState)(
            Interface::as_raw(policy),
            &raw mut state,
        )
    };
    if result == S_OK && state == NET_FW_MODIFY_STATE_OK {
        Ok(())
    } else {
        Err(format!(
            "{FIREWALL_UNDER_POLICY} (LocalPolicyModifyState {result:?}, state {})",
            state.0
        ))
    }
}

/// Finds or makes `spec`'s rule and writes every field, then checks its users.
fn ensure_rule(list: &INetFwRules, spec: &RuleSpec) -> Result<(), String> {
    let name = BSTR::from(spec.name.as_str());
    let com = |what: &str, error: windows::core::Error| format!("{what} failed ({error})");
    // SAFETY: `list` is live; `name` is a BSTR for the call.
    let rule: INetFwRule3 = if let Ok(found) = unsafe { list.Item(&name) } {
        found.cast().map_err(|e| com("reading the rule", e))?
    } else {
        {
            // SAFETY: COM is initialised on this thread; the class is the firewall's rule.
            let made: INetFwRule3 =
                unsafe { CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|e| com("making the rule", e))?;
            // SAFETY: `made` is a live rule; `name` is a BSTR for the call.
            unsafe { made.SetName(&name) }.map_err(|e| com("SetName", e))?;
            // Every field before it is added, so no half-written rule is ever in force.
            configure(&made, spec)?;
            // SAFETY: `list` and `made` are live.
            unsafe { list.Add(&made) }.map_err(|e| com("adding the rule", e))?;
            made
        }
    };
    configure(&rule, spec)?;
    // SAFETY: `rule` is live.
    let users = unsafe { rule.LocalUserAuthorizedList() }
        .map_err(|e| com("reading LocalUserAuthorizedList", e))?
        .to_string();
    let wanted = spec
        .users
        .split(";;;")
        .skip(1)
        .filter_map(|ace| ace.split(')').next());
    if let Some(missing) = wanted.into_iter().find(|sid| !users.contains(sid)) {
        return Err(format!("its users read back as {users}, without {missing}"));
    }
    Ok(())
}

/// Writes every field of `spec` to `rule`: protocol before ports, which only TCP and UDP take.
fn configure(rule: &INetFwRule3, spec: &RuleSpec) -> Result<(), String> {
    let com = |what: &str, error: windows::core::Error| format!("{what} failed ({error})");
    let protocol = match spec.protocol {
        RuleProtocol::Any => NET_FW_IP_PROTOCOL_ANY,
        RuleProtocol::Tcp => NET_FW_IP_PROTOCOL_TCP,
        RuleProtocol::Udp => NET_FW_IP_PROTOCOL_UDP,
    };
    let direction = match spec.direction {
        Direction::In => NET_FW_RULE_DIR_IN,
        Direction::Out => NET_FW_RULE_DIR_OUT,
    };
    // SAFETY: `rule` is a live rule on this thread; each BSTR lives through its call.
    unsafe {
        rule.SetDescription(&BSTR::from(spec.description.as_str()))
            .map_err(|e| com("SetDescription", e))?;
        rule.SetDirection(direction)
            .map_err(|e| com("SetDirection", e))?;
        rule.SetAction(NET_FW_ACTION_BLOCK)
            .map_err(|e| com("SetAction", e))?;
        rule.SetEnabled(VARIANT_TRUE)
            .map_err(|e| com("SetEnabled", e))?;
        rule.SetProfiles(NET_FW_PROFILE2_ALL.0)
            .map_err(|e| com("SetProfiles", e))?;
        rule.SetProtocol(protocol.0)
            .map_err(|e| com("SetProtocol", e))?;
        rule.SetRemoteAddresses(&BSTR::from(spec.remote_addresses))
            .map_err(|e| com("SetRemoteAddresses", e))?;
        if let Some(ports) = &spec.remote_ports {
            rule.SetRemotePorts(&BSTR::from(ports.as_str()))
                .map_err(|e| com("SetRemotePorts", e))?;
        }
        rule.SetLocalUserAuthorizedList(&BSTR::from(spec.users.as_str()))
            .map_err(|e| com("SetLocalUserAuthorizedList", e))?;
    }
    Ok(())
}
