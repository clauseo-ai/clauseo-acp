//! `folder_state` in the app rows (0023): a matter whose folder `ListMatters` read as missing
//! stays `missing` in every later `MattersChanged` row and in a new subscription's first frames,
//! and a relocate sends it `found` again.

use clauseo_protocol::{FieldChange, FolderState, HostCommand, HostEvent, MatterId};

use futures::StreamExt as _;

use crate::settings::{decode, ready, subscribe_app};
use crate::support::{Dir, Running, wait};
use crate::write;

/// `matter`'s `folder_state` in each `MattersChanged` row of `events`, in order.
fn states(events: &[HostEvent], matter: MatterId) -> Vec<FolderState> {
    events
        .iter()
        .filter_map(|event| match event {
            HostEvent::MattersChanged(changed) => Some(changed),
            _ => None,
        })
        .flat_map(|changed| changed.matters.iter())
        .filter(|row| row.matter == matter)
        .map(|row| row.folder_state)
        .collect()
}

/// `matter`'s `folder_state` as `ListMatters` reads it now.
fn listed(host: &Running, matter: MatterId) -> FolderState {
    host.matters()
        .into_iter()
        .find(|row| row.matter == matter)
        .unwrap_or_else(|| panic!("{matter} is not listed"))
        .folder_state
}

/// The next event on `sub`, waiting for it: a list's folder states reach the actor after the
/// list has answered, so the row they send may come a moment later.
fn next(sub: &mut clauseo_host::AppSubscription) -> HostEvent {
    let item = wait("an app event", sub.next()).expect("the app stream ended");
    decode(&item)
}

fn pin(matter: MatterId, pinned: bool) -> HostCommand {
    HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        pinned: Some(pinned),
    }
}

#[test]
fn a_folder_a_list_found_missing_stays_missing_in_app_rows_until_a_relocate() {
    let dir = Dir::new("lists_folder_state", &[]);
    let host = dir.start(0);
    let old = dir.folder("a", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("a", &old);
    let (mut sub, first) = subscribe_app(&host);
    assert_eq!(states(&first, matter), [FolderState::Found], "{first:?}");

    // Moved away: the next list reads it missing, and the app row follows.
    let moved = dir.work_path("a-moved");
    write::rename(&old, &moved);
    assert_eq!(listed(&host, matter), FolderState::Missing);
    let sent = [next(&mut sub)];
    assert_eq!(states(&sent, matter), [FolderState::Missing], "{sent:?}");

    // A later change of another field keeps what the list read.
    host.ok(pin(matter, true));
    let sent = [next(&mut sub)];
    assert_eq!(states(&sent, matter), [FolderState::Missing], "{sent:?}");
    // So does a new subscription's first frame.
    let (_late, first) = subscribe_app(&host);
    assert_eq!(states(&first, matter), [FolderState::Missing], "{first:?}");

    // A relocate to where the folder went sends it found.
    let folder = host.folder_ref(&moved);
    host.ok(HostCommand::RelocateMatter { matter, folder });
    let sent = [next(&mut sub)];
    assert_eq!(states(&sent, matter), [FolderState::Found], "{sent:?}");
    assert_eq!(listed(&host, matter), FolderState::Found);
    host.settle();
    assert_eq!(ready(&mut sub), [], "the list agrees, so nothing more goes");
    host.shutdown();
}
