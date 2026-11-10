//! A matter on a volume the git watch's stream can't watch: allowed, scanned before and after
//! only, when no other path the agent writes is on that volume; refused otherwise. The volume
//! facts are faked (`fake_volume`): no network volume is at hand.

use super::*;
use crate::policy::git_aside::{MatterWatch, Volume, fake_volume};

const NETWORK: Volume = Volume {
    id: 7,
    watchable: false,
};

#[test]
fn a_matter_on_a_local_volume_is_watched_live() {
    let w = World::new("matter-watch-live");
    let policy = w.builder().build().unwrap();
    assert_eq!(policy.matter_watch(), MatterWatch::Live);
}

#[test]
fn a_matter_alone_on_an_unwatchable_volume_is_scanned_before_and_after_only() {
    let w = World::new("matter-watch-after");
    fake_volume(&canon(&w.at("matter")), NETWORK);
    let policy = w
        .builder()
        .notes(w.at("data/agent-state/m1/notes"))
        .build()
        .unwrap();
    assert_eq!(policy.matter_watch(), MatterWatch::AfterOnly);
}

#[test]
fn a_matter_sharing_its_unwatchable_volume_with_a_writable_path_is_refused() {
    let w = World::new("matter-watch-shared");
    fake_volume(&canon(&w.at("matter")), NETWORK);
    // The state folder on the same volume: a folder made there could be renamed in.
    fake_volume(&canon(&w.at("state")), NETWORK);
    refused(
        w.builder().build(),
        "the agent can also write",
        &w.at("state"),
    );
    // The notes folder in a folder on that volume.
    let w = World::new("matter-watch-notes");
    w.dir.dir("other/notes");
    fake_volume(&canon(&w.at("matter")), NETWORK);
    fake_volume(&canon(&w.at("other")), NETWORK);
    let notes = w.at("other/notes");
    refused(
        w.builder().notes(&notes).build(),
        "network or automounted volume",
        &notes,
    );
    // A read-only path on that volume is no way in.
    let w = World::new("matter-watch-read");
    fake_volume(&canon(&w.at("matter")), NETWORK);
    fake_volume(&canon(&w.at("other")), NETWORK);
    let policy = w.builder().read(w.at("other")).build().unwrap();
    assert_eq!(policy.matter_watch(), MatterWatch::AfterOnly);
}
