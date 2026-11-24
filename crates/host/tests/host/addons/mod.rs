//! `ListAddons`, the one list of skills and tools, and `AddonsChanged`: the skills in
//! `skills.rs`, the lawyer's MCP servers in `tools.rs`, case law's statuses and the checks a read
//! starts in `case_law.rs`. The skill packs' own commands' tests are here too
//! (`skill_packs.rs`, `skill_packs_cap.rs`), and scopes and per-matter switches with what each
//! matter's launch gets (`scope.rs`, `scope_launch.rs`).

use clauseo_host::AppSubscription;
use clauseo_protocol::{Addon, AddonKind, HostEvent, HostQuery, QueryResult};
use futures::FutureExt as _;
use futures::StreamExt as _;

use crate::support::{Running, wait};

mod case_law;
pub(crate) mod scope;
mod scope_launch;
mod skill_packs;
mod skill_packs_cap;
mod skills;
mod tools;

/// The list, with no matter.
pub(crate) fn addons(host: &Running) -> Vec<Addon> {
    match host.query(HostQuery::ListAddons { matter: None }).unwrap() {
        QueryResult::Addons(list) => list.items,
        other => panic!("ListAddons answered {other:?}"),
    }
}

/// The row of `kind` with `id`.
pub(crate) fn item<'a>(items: &'a [Addon], kind: AddonKind, id: &str) -> &'a Addon {
    items
        .iter()
        .find(|item| item.kind == kind && item.id == id)
        .unwrap_or_else(|| panic!("{id} is not listed: {items:?}"))
}

/// Waits for the next `AddonsChanged` on `sub`, skipping every other event.
pub(crate) fn addons_changed(sub: &mut AppSubscription) {
    wait("an AddonsChanged", async {
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::AddonsChanged {} = crate::settings::decode(&item) {
                return;
            }
        }
    });
}

/// Reads away every event `host` has sent `sub` so far (the start-up's among them), so the next
/// `AddonsChanged` read is one the test caused.
pub(crate) fn drain(host: &Running, sub: &mut AppSubscription) -> Vec<HostEvent> {
    host.settle();
    let mut events = Vec::new();
    while let Some(Some(item)) = sub.next().now_or_never() {
        events.push(crate::settings::decode(&item));
    }
    events
}
