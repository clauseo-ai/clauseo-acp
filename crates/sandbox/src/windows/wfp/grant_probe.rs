//! What the owner may do to set-up's WFP objects (S15 PR7, for the VM harness, run unelevated
//! as the owner): read them, and nothing else. Set-up grants read only (orchestrator ruling,
//! 2026-11-07), so adding a filter in our sublayer, deleting one of ours, linking a filter to
//! our sublayer at another layer, or changing one's permissions must each be refused. The adds
//! run in a dynamic session, so one that wrongly succeeds goes when the probe ends; a delete or
//! permission change that wrongly succeeds is real, and the row says to run set-up again.

use std::net::Ipv4Addr;

use super::check::WfpError;
use super::engine::{Engine, SET_SECURITY, filters_in, set_dacl};
use super::spec::{Action, Condition, FilterSpec, Layer, Namespace, Object, PERMIT_WEIGHT};

/// One probe: what was tried, whether it should be refused, and what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantProbe {
    /// What was tried.
    pub what: &'static str,
    /// `true` when the owner must be refused.
    pub expect_refused: bool,
    /// The call under test, as a refusal names it.
    pub call: &'static str,
    /// What WFP said.
    pub result: Result<(), WfpError>,
}

impl GrantProbe {
    /// Whether the probe went as it should: a refusal must be access denied from the call
    /// under test ([`WfpError::is_access_denied_from`]), not any failure; an allowed call must
    /// succeed.
    pub fn passed(&self) -> bool {
        match &self.result {
            Ok(()) => !self.expect_refused,
            Err(error) => self.expect_refused && error.is_access_denied_from(self.call),
        }
    }
}

/// A filter the owner tries to add in our sublayer at `layer`.
fn stray(ns: &Namespace, layer: Layer, n: u16) -> FilterSpec {
    FilterSpec {
        key: ns.filter_key(0x7F00 + n),
        name: format!("{} owner probe {n}", ns.display),
        layer,
        action: Action::Permit,
        weight: PERMIT_WEIGHT,
        // A condition the layer takes, so a refusal can only be the owner's missing right.
        conditions: vec![match layer {
            Layer::ConnectV4 => Condition::RemoteV4(Ipv4Addr::LOCALHOST),
            Layer::ConnectV6
            | Layer::RecvAcceptV4
            | Layer::RecvAcceptV6
            | Layer::AssignV4
            | Layer::AssignV6 => Condition::Protocol(6),
            Layer::ListenV4 | Layer::ListenV6 => Condition::LocalPorts(1, 1),
        }],
    }
}

/// Adds (elevated) a persistent permit in our sublayer that set-up didn't write, for the VM row
/// that checks a launch refuses it (does the owner's enumeration see it?); with `remove`,
/// deletes it. Running set-up again deletes it too.
pub fn foreign_test_filter(ns: &Namespace, remove: bool) -> Result<(), WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let spec = stray(ns, Layer::ConnectV4, 2);
    if remove {
        engine.delete_filter(spec.key).map(drop)
    } else {
        engine.add_filter(ns, &spec, None).map(drop)
    }
}

/// Runs the probes in `ns` as the calling user. Its adds pass no descriptor: one naming
/// Administrators as owner could be refused for the owner, not for the missing add right. The
/// foreign test filter takes WFP's default too: it stands for a filter set-up didn't write.
pub fn probe_owner_grants(ns: &Namespace) -> Vec<GrantProbe> {
    let engine = match Engine::open(true, &ns.display) {
        Ok(engine) => engine,
        Err(error) => {
            return vec![GrantProbe {
                what: "open a WFP session",
                expect_refused: false,
                call: "FwpmEngineOpen0",
                result: Err(error),
            }];
        }
    };
    let read = filters_in(&engine, ns)
        .map(drop)
        .map_err(|(call, code)| WfpError { call, code });
    let add = engine
        .add_filter(ns, &stray(ns, Layer::ConnectV4, 0), None)
        .map(drop);
    let link = engine
        .add_filter(ns, &stray(ns, Layer::ConnectV6, 1), None)
        .map(drop);
    // A DACL written with no read first needs WRITE_DAC alone, which the owner lacks.
    let write_dac = set_dacl(
        &engine,
        ns,
        Object::Filter(ns.filter_key(0)),
        "D:(A;;GA;;;SY)(A;;GA;;;BA)",
    );
    let delete = engine.delete_filter(ns.filter_key(0)).map(drop);
    vec![
        GrantProbe {
            what: "read the filters in our sublayer",
            expect_refused: false,
            call: "FwpmFilterEnum0",
            result: read,
        },
        GrantProbe {
            what: "add a filter in our sublayer (ALE_AUTH_CONNECT_V4)",
            expect_refused: true,
            call: "FwpmFilterAdd0",
            result: add,
        },
        GrantProbe {
            what: "link a filter to our sublayer at ALE_AUTH_CONNECT_V6",
            expect_refused: true,
            call: "FwpmFilterAdd0",
            result: link,
        },
        GrantProbe {
            what: "change the permissions of set-up's first block filter (WRITE_DAC; if this \
                   works, run set-up again)",
            expect_refused: true,
            call: SET_SECURITY,
            result: write_dac,
        },
        GrantProbe {
            what: "delete set-up's first block filter (if this works, run set-up again)",
            expect_refused: true,
            call: "FwpmFilterDeleteByKey0",
            result: delete,
        },
    ]
}
