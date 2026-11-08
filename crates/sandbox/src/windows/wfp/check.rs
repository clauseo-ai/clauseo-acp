//! A launch's fail-closed check of set-up's network objects (S15 PR7), with no Windows call:
//! before a tier-B launch starts its agent, set-up's provider and sublayer must be there, and
//! every filter in our sublayer must be one of set-up's, exactly as set-up writes it: the block
//! filters catching the launch's pool user, and every filter of its slot naming it (the slot is
//! what the check returns, so the launch's ports follow set-up's filters). Any other filter in
//! the sublayer, an earlier set-up's shared permit, any change, or a read the owner is refused,
//! refuses the launch with a named cause ([`NetworkGap`]), so no agent runs unfiltered. The reads
//! go through [`SetupReader`]; `engine/read.rs` reads WFP itself. Set-up uses [`classify`] to
//! find foreign filters.

use std::collections::BTreeMap;

use super::slot_filters::slot_of_key;
use super::spec::{
    Action, BLOCK_COUNT, Condition, FilterSpec, Flags, Layer, Match, Namespace, SLOT_FILTERS,
    SUBLAYER_WEIGHT, allowed_users, block_filters, denied_users, is_account, is_old_permit_key,
    old_permit_filter, slot_filters,
};
use crate::ports::Slot;
use crate::windows::sids::Sid;

/// Why a tier-B launch can't get its network filtered, so it is refused. The host maps each
/// (R17 PR-E).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetworkGap {
    /// Set-up's provider or sublayer isn't there: set-up's network step never ran, or the
    /// set-up was removed.
    #[error("the sandbox's network filters aren't set up")]
    NotSetUp,
    /// One of set-up's filters is missing or not as set-up wrote it; its name.
    #[error("the sandbox's network filter {0} is missing or changed")]
    FilterMissing(String),
    /// Set-up's filters don't catch the launch's pool user, or its slot's filters are missing.
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
    /// The policy names a loopback port that isn't one of the pool user's slot's
    /// (`ports::Slot`), which its filters leave closed.
    #[error("the sandbox can't permit loopback port {0}, outside this matter's sandbox ports")]
    PortOutsideRange(u16),
    /// WFP couldn't be opened to check (the Base Filtering Engine isn't running, say).
    #[error("the sandbox couldn't check its network filters ({call} failed, error {code:#x})")]
    EngineUnavailable {
        /// The call, as Windows names it.
        call: &'static str,
        /// Its result code.
        code: u32,
    },
    /// A filter in our sublayer that set-up didn't write; its name. Running set-up again
    /// deletes it.
    #[error("a network filter set-up didn't write is in the sandbox's sublayer: {0}")]
    ForeignFilter(String),
    /// Windows' TCP dynamic port range (first and last port) overlaps `ports::TIER_B_PORTS`, so
    /// the OS can hand those ports to any program.
    #[error(
        "Windows' dynamic port range ({first}-{last}) overlaps the sandbox's ports 46100-46611: \
         IT can move it back to its default with \
         `netsh int ipv4 set dynamicport tcp start=49152 num=16384`"
    )]
    DynamicPortsOverlap {
        /// Its first port.
        first: u16,
        /// Its last port.
        last: u16,
    },
    /// A port of `ports::TIER_B_PORTS` that Windows refuses to bind: an excluded port range.
    #[error(
        "port {0} of the sandbox's ports 46100-46611 is reserved on this computer (Hyper-V, WSL \
         and Docker reserve port ranges; see `netsh int ipv4 show excludedportrange \
         protocol=tcp`)"
    )]
    PortsReserved(u16),
    /// A TCP listener in `ports::TIER_B_PORTS` that isn't this app's: its port and its program.
    #[error(
        "another program ({program}) listens on port {port}, inside the sandbox's ports \
         46100-46611, where a sandboxed agent could reach it"
    )]
    ForeignListener {
        /// Its port.
        port: u16,
        /// Its program file, or its process id.
        program: String,
    },
    /// The machine's ports couldn't be read, so the launch or set-up fails closed.
    #[error("the sandbox couldn't check its ports ({call} failed, error {code:#x})")]
    PortsUnchecked {
        /// The call, as Windows names it.
        call: &'static str,
        /// Its result code.
        code: u32,
    },
    /// An earlier set-up's shared loopback permit, which let every sandbox user reach every
    /// other's loopback ports; its name. Running set-up again replaces it with the per-slot
    /// filters.
    #[error(
        "the sandbox's network filters are from an earlier version of set-up ({0}); run set-up \
         again"
    )]
    OldSetup(String),
}

impl NetworkGap {
    /// Whether running set-up again fixes it (`sandbox.setup_required`): set-up's objects are
    /// missing, changed, unreadable, joined by a foreign filter, or don't cover the pool user.
    /// The rest are the launch's own failures (`agent.launch_failed`), the machine's ports
    /// included: running set-up again doesn't change them.
    pub fn needs_setup(&self) -> bool {
        matches!(
            self,
            NetworkGap::NotSetUp
                | NetworkGap::FilterMissing(_)
                | NetworkGap::PoolUserNotCovered
                | NetworkGap::Unreadable { .. }
                | NetworkGap::ForeignFilter(_)
                | NetworkGap::OldSetup(_)
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

/// The [`WfpError::call`] when one of set-up's WFP objects isn't owned by Administrators or
/// SYSTEM (with [`ERROR_INVALID_OWNER`]): its owner may rewrite its permissions.
pub(in crate::windows::wfp) const OWNER_CHECK: &str =
    "the owner check of the sandbox's network objects";

/// `ERROR_INVALID_OWNER`, the code of a failed [`OWNER_CHECK`].
pub(in crate::windows::wfp) const ERROR_INVALID_OWNER: u32 = 1307;

/// Whether `owner`, the owner of one of set-up's WFP objects, may own it: Administrators or
/// SYSTEM, never the lawyer, whose implicit owner rights would let them rewrite its DACL.
#[cfg(any(windows, test))]
pub(in crate::windows::wfp) fn owner_allowed(owner: &str) -> bool {
    matches!(owner, "S-1-5-32-544" | "S-1-5-18")
}

impl WfpError {
    /// Whether this is access denied (`ERROR_ACCESS_DENIED`, as a Win32 code or an HRESULT)
    /// from `call`: what the owner probe's refusals must be, so none passes for another reason.
    pub fn is_access_denied_from(&self, call: &str) -> bool {
        self.call == call && matches!(self.code, 5 | 0x8007_0005)
    }

    /// The words for set-up's cause: the Base Filtering Engine named when it isn't running,
    /// objects of ours owned by someone other than Administrators or SYSTEM named, else the
    /// call and its code. Each fails the set-up: WFP is the boundary.
    pub fn cause(&self) -> String {
        if self.call == OWNER_CHECK && self.code == ERROR_INVALID_OWNER {
            "Clauseo's network filters came out owned by an account other than Administrators \
             or SYSTEM, though set-up asks for Administrators, so that account could change \
             them: IT should check the policy \"System objects: Default owner for objects \
             created by members of the Administrators group\" and that nothing else on this \
             computer manages Windows Filtering Platform permissions, then run Clauseo's set-up \
             again"
                .to_owned()
        } else if BFE_DOWN.contains(&self.code) {
            format!(
                "the Base Filtering Engine service (BFE) isn't running, and Clauseo's network \
                 protection needs it: IT can set it to start automatically ({self})"
            )
        } else {
            self.to_string()
        }
    }
}

/// One condition of a filter as WFP holds it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FoundCondition {
    /// An `ALE_USER_ID` condition: its match type and its descriptor's DACL as SDDL.
    Users {
        /// Its match type.
        matching: Match,
        /// The descriptor's DACL, as SDDL.
        sddl: String,
    },
    /// `FWPM_CONDITION_FLAGS`, `FLAGS_NONE_SET`, `IS_LOOPBACK`.
    NotLoopback,
    /// `FWPM_CONDITION_IP_PROTOCOL`, `EQUAL`.
    Protocol(u8),
    /// `FWPM_CONDITION_IP_REMOTE_ADDRESS`, `EQUAL`, an IPv4 address in host order.
    RemoteV4(u32),
    /// `FWPM_CONDITION_IP_REMOTE_PORT`, `RANGE`.
    RemotePorts(u16, u16),
    /// Anything else.
    Other,
    /// `FWPM_CONDITION_IP_LOCAL_PORT`, `RANGE`.
    LocalPorts(u16, u16),
}

/// A filter in our sublayer as WFP holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFilter {
    /// Its key.
    pub key: u128,
    /// Its WFP id, for deleting it.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// Its layer; `None` for a layer that isn't one of ours.
    pub layer: Option<Layer>,
    /// Its action; `None` for one that is neither block nor permit.
    pub action: Option<Action>,
    /// Its weight, when it is a `FWP_UINT8`.
    pub weight: Option<u8>,
    /// Its flags.
    pub flags: Flags,
    /// Its flag bits other than those in [`Flags`] (`FWPM_FILTER_FLAG_DISABLED`, say): set-up
    /// sets none, so any refuses it.
    pub other_flags: u32,
    /// Its provider's key, if it has one.
    pub provider: Option<u128>,
    /// Its conditions.
    pub conditions: Vec<FoundCondition>,
}

/// What a launch reads of set-up's objects. A read that fails is `Err((call, code))`.
pub trait SetupReader {
    /// Whether the provider `key` is there.
    fn provider(&mut self, key: u128) -> Result<bool, (&'static str, u32)>;
    /// The sublayer `key`'s weight; `None` when it isn't there.
    fn sublayer(&mut self, key: u128) -> Result<Option<u16>, (&'static str, u32)>;
    /// Every filter in the sublayer `key` the reader may see.
    fn filters_in(&mut self, key: u128) -> Result<Vec<FoundFilter>, (&'static str, u32)>;
}

/// What a filter in our sublayer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Set-up's block filter `n`, as set-up writes it, catching these SIDs (group and pool).
    Block(usize, Vec<Sid>),
    /// An earlier set-up's shared loopback permit for this pool user, as that set-up wrote it.
    /// A launch refuses it ([`NetworkGap::OldSetup`]); set-up deletes it.
    Permit(Sid),
    /// Anything else: not set-up's, or changed.
    Foreign,
    /// Filter `n` of this slot, as set-up writes it, for this pool user.
    Slot(Slot, u16, Sid),
}

/// `spec`'s conditions as WFP would hold them, the user descriptor left out, sorted.
fn plain(conditions: &[Condition]) -> Vec<FoundCondition> {
    let mut plain: Vec<FoundCondition> = conditions
        .iter()
        .filter_map(|c| match c {
            Condition::Users(_) | Condition::NotPool { .. } | Condition::NotUsers(_) => None,
            Condition::NotLoopback => Some(FoundCondition::NotLoopback),
            Condition::Protocol(p) => Some(FoundCondition::Protocol(*p)),
            Condition::RemoteV4(a) => Some(FoundCondition::RemoteV4(u32::from(*a))),
            Condition::RemotePorts(lo, hi) => Some(FoundCondition::RemotePorts(*lo, *hi)),
            Condition::LocalPorts(lo, hi) => Some(FoundCondition::LocalPorts(*lo, *hi)),
        })
        .collect();
    plain.sort();
    plain
}

/// Whether `found` has `spec`'s key, layer, action, weight, flags and plain conditions, and
/// exactly one user condition with `matching`; that condition's SDDL when it does.
fn shaped_like<'f>(found: &'f FoundFilter, spec: &FilterSpec, matching: Match) -> Option<&'f str> {
    let mut users = found.conditions.iter().filter_map(|c| {
        if let FoundCondition::Users { matching: m, sddl } = c
            && *m == matching
        {
            Some(sddl.as_str())
        } else {
            None
        }
    });
    let sddl = users.next()?;
    let mut rest: Vec<FoundCondition> = found
        .conditions
        .iter()
        .filter(|c| !matches!(c, FoundCondition::Users { .. }))
        .cloned()
        .collect();
    rest.sort();
    let same = found.key == spec.key
        && found.layer == Some(spec.layer)
        && found.action == Some(spec.action)
        && found.weight == Some(spec.weight)
        && found.flags == spec.flags()
        && found.conditions.len() == rest.len() + 1
        && users.next().is_none()
        && rest == plain(&spec.conditions);
    same.then_some(sddl)
}

/// What `found`, a filter in our sublayer in `ns`, is: one of set-up's blocks or static permits
/// exactly as set-up writes them, or foreign.
pub fn classify(ns: &Namespace, found: &FoundFilter) -> Kind {
    if found.other_flags != 0 || found.provider != Some(ns.provider) {
        return Kind::Foreign;
    }
    let placeholder = Sid::parse("S-1-5-21-0-0-0-1000");
    let Ok(placeholder) = placeholder else {
        return Kind::Foreign;
    };
    let blocks = block_filters(ns, &placeholder, std::slice::from_ref(&placeholder));
    if let Some(n) = blocks.iter().position(|b| b.key == found.key) {
        let denied = blocks
            .get(n)
            .and_then(|spec| shaped_like(found, spec, Match::NotEqual))
            .and_then(denied_users);
        return match denied {
            Some(denied) if denied.iter().all(is_account) => Kind::Block(n, denied),
            _ => Kind::Foreign,
        };
    }
    if let Some((slot, n)) = slot_of_key(ns, found.key) {
        let specs = slot_filters(ns, slot, &placeholder);
        let user = specs.get(usize::from(n)).and_then(|spec| {
            let matching = if spec.action == Action::Permit {
                Match::Equal
            } else {
                Match::NotEqual
            };
            shaped_like(found, spec, matching).and_then(allowed_users)
        });
        return match user.as_deref() {
            Some([user]) if is_account(user) => Kind::Slot(slot, n, user.clone()),
            _ => Kind::Foreign,
        };
    }
    if !is_old_permit_key(ns, found.key) {
        return Kind::Foreign;
    }
    let permit = (0u16..)
        .map(|i| old_permit_filter(ns, i, &placeholder))
        .take(usize::from(crate::windows::pool::POOL_CAP))
        .find(|spec| spec.key == found.key);
    let users = permit
        .as_ref()
        .and_then(|spec| shaped_like(found, spec, Match::Equal))
        .and_then(allowed_users);
    match users.as_deref() {
        Some([user]) if is_account(user) => Kind::Permit(user.clone()),
        _ => Kind::Foreign,
    }
}

/// Checks set-up's objects in `ns` for a launch of `user`: the provider and the sublayer are
/// there, the sublayer at set-up's weight; every filter in the sublayer is one of set-up's
/// ([`classify`]), and none is an earlier set-up's shared permit; all the blocks are there and
/// each catches `user`; every slot with a filter has all of them, each naming the same user, a
/// user the blocks catch (the pool set-up recorded), and no user has two slots; and `user` has
/// a slot. That slot, whose ports the launch then uses.
pub fn check_setup(
    ns: &Namespace,
    user: &Sid,
    reader: &mut impl SetupReader,
) -> Result<Slot, NetworkGap> {
    let unreadable = |(call, code)| NetworkGap::Unreadable { call, code };
    if !reader.provider(ns.provider).map_err(unreadable)? {
        return Err(NetworkGap::NotSetUp);
    }
    match reader.sublayer(ns.sublayer).map_err(unreadable)? {
        None => return Err(NetworkGap::NotSetUp),
        Some(weight) if weight != SUBLAYER_WEIGHT => {
            return Err(NetworkGap::FilterMissing(format!(
                "{} sublayer",
                ns.display
            )));
        }
        Some(_) => {}
    }
    let mut blocks: Vec<Option<Vec<Sid>>> = vec![None; BLOCK_COUNT];
    let mut slots: BTreeMap<Slot, Vec<Option<(Sid, String)>>> = BTreeMap::new();
    for found in reader.filters_in(ns.sublayer).map_err(unreadable)? {
        match classify(ns, &found) {
            Kind::Block(n, denied) => {
                if let Some(slot) = blocks.get_mut(n) {
                    *slot = Some(denied);
                }
            }
            Kind::Slot(slot, n, sid) => {
                let filters = slots
                    .entry(slot)
                    .or_insert_with(|| vec![None; usize::from(SLOT_FILTERS)]);
                if let Some(entry) = filters.get_mut(usize::from(n)) {
                    *entry = Some((sid, found.name));
                }
            }
            Kind::Permit(_) => return Err(NetworkGap::OldSetup(found.name)),
            Kind::Foreign => return Err(NetworkGap::ForeignFilter(found.name)),
        }
    }
    let Ok(placeholder) = Sid::parse("S-1-5-21-0-0-0-1000") else {
        return Err(NetworkGap::NotSetUp);
    };
    let mut owners: BTreeMap<Sid, Slot> = BTreeMap::new();
    for (slot, filters) in &slots {
        let specs = slot_filters(ns, *slot, &placeholder);
        let mut named: Option<&Sid> = None;
        for (spec, entry) in specs.iter().zip(filters) {
            let Some((sid, name)) = entry else {
                return Err(NetworkGap::FilterMissing(spec.name.clone()));
            };
            if named.is_some_and(|first| first != sid) {
                return Err(NetworkGap::ForeignFilter(name.clone()));
            }
            named = Some(sid);
        }
        if let Some(sid) = named
            && owners.insert(sid.clone(), *slot).is_some()
        {
            return Err(NetworkGap::ForeignFilter(spec_name(filters)));
        }
    }
    for (spec, denied) in block_filters(ns, &placeholder, &[]).iter().zip(&blocks) {
        let Some(denied) = denied else {
            return Err(NetworkGap::FilterMissing(spec.name.clone()));
        };
        if !denied.contains(user) {
            return Err(NetworkGap::PoolUserNotCovered);
        }
        if let Some((_, slot)) = owners.iter().find(|(sid, _)| !denied.contains(sid)) {
            let name = slots.get(slot).map(|f| spec_name(f)).unwrap_or_default();
            return Err(NetworkGap::ForeignFilter(name));
        }
    }
    owners
        .get(user)
        .copied()
        .ok_or(NetworkGap::PoolUserNotCovered)
}

/// The name of a slot's first filter found, for a refusal.
fn spec_name(filters: &[Option<(Sid, String)>]) -> String {
    filters
        .iter()
        .flatten()
        .map(|(_, name)| name.clone())
        .next()
        .unwrap_or_default()
}
