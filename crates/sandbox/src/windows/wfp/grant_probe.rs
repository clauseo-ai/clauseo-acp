//! What the owner may do to set-up's WFP objects (S15 PR7, for the VM harness, run unelevated
//! as the owner): read them, and nothing else. Set-up grants read only (orchestrator ruling,
//! 2026-11-07), so adding a filter in our sublayer, deleting one of ours, or linking a filter to
//! our sublayer at another layer must each be refused. The adds run in a dynamic session, so
//! one that wrongly succeeds goes when the probe ends; a delete that wrongly succeeds is real,
//! and the row says to run set-up again.

use std::net::Ipv4Addr;

use super::check::WfpError;
use super::engine::{Engine, filters_in};
use super::spec::{Action, Condition, FilterSpec, Layer, Namespace, PERMIT_WEIGHT};

/// One probe: what was tried, whether it should be refused, and what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantProbe {
    /// What was tried.
    pub what: &'static str,
    /// `true` when the owner must be refused.
    pub expect_refused: bool,
    /// What WFP said.
    pub result: Result<(), WfpError>,
}

/// A filter the owner tries to add in our sublayer at `layer`.
fn stray(ns: &Namespace, layer: Layer, n: u16) -> FilterSpec {
    FilterSpec {
        key: ns.filter_key(0x7F00 + n),
        name: format!("{} owner probe {n}", ns.display),
        layer,
        action: Action::Permit,
        weight: PERMIT_WEIGHT,
        conditions: vec![Condition::RemoteV4(Ipv4Addr::LOCALHOST)],
    }
}

/// Runs the probes in `ns` as the calling user.
pub fn probe_owner_grants(ns: &Namespace) -> Vec<GrantProbe> {
    let engine = match Engine::open(true, &ns.display) {
        Ok(engine) => engine,
        Err(error) => {
            return vec![GrantProbe {
                what: "open a WFP session",
                expect_refused: false,
                result: Err(error),
            }];
        }
    };
    let read = filters_in(&engine, ns)
        .map(drop)
        .map_err(|(call, code)| WfpError { call, code });
    let add = engine
        .add_filter(ns, &stray(ns, Layer::ConnectV4, 0))
        .map(drop);
    let link = engine
        .add_filter(ns, &stray(ns, Layer::ConnectV6, 1))
        .map(drop);
    let delete = engine.delete_filter(ns.filter_key(0)).map(drop);
    vec![
        GrantProbe {
            what: "read the filters in our sublayer",
            expect_refused: false,
            result: read,
        },
        GrantProbe {
            what: "add a filter in our sublayer (ALE_AUTH_CONNECT_V4)",
            expect_refused: true,
            result: add,
        },
        GrantProbe {
            what: "link a filter to our sublayer at ALE_AUTH_CONNECT_V6",
            expect_refused: true,
            result: link,
        },
        GrantProbe {
            what: "delete set-up's first block filter (if this works, run set-up again)",
            expect_refused: true,
            result: delete,
        },
    ]
}
