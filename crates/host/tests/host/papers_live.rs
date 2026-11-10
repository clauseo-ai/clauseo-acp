//! The open matter's folder, watched (decision record 0055): a paper added, renamed, moved out or
//! deleted while the matter is open sends `PapersChanged`, and the listing that follows shows the
//! folder as it is; a folder moved away reads `missing` in the matter's row, and a relocate
//! watches the new folder and reads `found`. Real file-system events with the host's 300 ms
//! debounce: each step waits for the listing it expects, over as many `PapersChanged` as come.

use clauseo_host::AppSubscription;
use clauseo_protocol::{FolderState, HostCommand, HostEvent, HostQuery, MatterId, QueryResult};
use futures::StreamExt as _;

use crate::settings::{decode, subscribe_app};
use crate::support::{Dir, Running, wait};
use crate::write;

/// How many `PapersChanged` one step may take before its listing shows what it did.
const ROUNDS: usize = 20;

fn start(dir: &Dir) -> Running {
    dir.start(0)
}

fn paths(host: &Running, matter: MatterId) -> Vec<String> {
    match host.query(HostQuery::ListPapers { matter }).unwrap() {
        QueryResult::Papers { papers, .. } => papers
            .into_iter()
            .map(|paper| paper.path.as_str().to_owned())
            .collect(),
        other => panic!("ListPapers answered {other:?}"),
    }
}

/// Every app event up to and with the next `PapersChanged` of `matter`.
fn next_change(sub: &mut AppSubscription, matter: MatterId) -> Vec<HostEvent> {
    let mut seen = Vec::new();
    loop {
        let item = wait("papers_changed", sub.next()).expect("the app stream ended");
        let event = decode(&item);
        let done = matches!(&event, HostEvent::PapersChanged { matter: m } if *m == matter);
        seen.push(event);
        if done {
            return seen;
        }
    }
}

/// Waits through `PapersChanged` events until the listing is `want`.
fn until_listed(host: &Running, sub: &mut AppSubscription, matter: MatterId, want: &[&str]) {
    for _ in 0..ROUNDS {
        next_change(sub, matter);
        if paths(host, matter) == want {
            return;
        }
    }
    panic!(
        "the listing never became {want:?}: {:?}",
        paths(host, matter)
    );
}

/// Waits through app events until `matter`'s row reads `state`.
fn until_row(sub: &mut AppSubscription, matter: MatterId, state: FolderState) {
    for _ in 0..ROUNDS * 4 {
        let item = wait("a matter row", sub.next()).expect("the app stream ended");
        if let HostEvent::MattersChanged(changed) = decode(&item)
            && changed
                .matters
                .iter()
                .any(|row| row.matter == matter && row.folder_state == state)
        {
            return;
        }
    }
    panic!("{matter}'s row never read {state:?}");
}

#[test]
fn papers_added_renamed_moved_out_and_deleted_follow_the_folder_live() {
    let dir = Dir::new("papers_live_follow", &[]);
    let host = start(&dir);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4\n")]);
    let matter = host.create_matter("m", &folder);
    let (mut sub, _first) = subscribe_app(&host);
    host.ok(HostCommand::OpenMatter { matter });
    assert_eq!(paths(&host, matter), ["brief.txt"]);

    // Added, then a Word owner file beside it, which the listing never shows.
    write::write(&folder.join("notice.docx"), b"notice");
    write::write(&folder.join("~$notice.docx"), b"owner");
    until_listed(&host, &mut sub, matter, &["brief.txt", "notice.docx"]);

    // Renamed in place, and into a subfolder.
    write::rename(&folder.join("notice.docx"), &folder.join("reply.docx"));
    until_listed(&host, &mut sub, matter, &["brief.txt", "reply.docx"]);
    write::write(&folder.join("drafts/keep.txt"), b"k");
    write::rename(
        &folder.join("reply.docx"),
        &folder.join("drafts/reply.docx"),
    );
    until_listed(
        &host,
        &mut sub,
        matter,
        &["brief.txt", "drafts/keep.txt", "drafts/reply.docx"],
    );

    // Moved out of the folder, then deleted.
    write::rename(
        &folder.join("drafts/reply.docx"),
        &dir.work_path("reply.docx"),
    );
    until_listed(&host, &mut sub, matter, &["brief.txt", "drafts/keep.txt"]);
    write::remove_file(&folder.join("drafts/keep.txt"));
    until_listed(&host, &mut sub, matter, &["brief.txt"]);
    host.shutdown();
}

#[test]
fn a_folder_moved_away_reads_missing_and_a_relocate_watches_it_again() {
    let dir = Dir::new("papers_live_moved", &[]);
    let host = start(&dir);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let (mut sub, _first) = subscribe_app(&host);
    host.ok(HostCommand::OpenMatter { matter });

    let moved = dir.work_path("m-moved");
    write::rename(&folder, &moved);
    until_row(&mut sub, matter, FolderState::Missing);

    // Located where it went: found, and the app is told to list it there.
    let picked = host.folder_ref(&moved);
    host.ok(HostCommand::RelocateMatter {
        matter,
        folder: picked,
    });
    next_change(&mut sub, matter);
    assert_eq!(paths(&host, matter), ["brief.txt"]);

    // The new folder is the one watched.
    write::write(&moved.join("added.txt"), b"a");
    until_listed(&host, &mut sub, matter, &["added.txt", "brief.txt"]);
    host.shutdown();
}

#[test]
fn opening_another_matter_moves_the_watch() {
    let dir = Dir::new("papers_live_other", &[]);
    let host = start(&dir);
    let first = dir.folder("a", &[("a.txt", "a\n")]);
    let second = dir.folder("b", &[("b.txt", "b\n")]);
    let a = host.create_matter("a", &first);
    let b = host.create_matter("b", &second);
    let (mut sub, _first) = subscribe_app(&host);
    host.ok(HostCommand::OpenMatter { matter: a });
    host.ok(HostCommand::OpenMatter { matter: b });

    write::write(&first.join("late.txt"), b"x");
    write::write(&second.join("new.txt"), b"y");
    // Only b is told, over two of its changes: a's would have come by then.
    let mut seen = next_change(&mut sub, b);
    write::write(&second.join("newer.txt"), b"z");
    seen.extend(next_change(&mut sub, b));
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, HostEvent::PapersChanged { matter } if *matter == a)),
        "{seen:?}"
    );
    host.shutdown();
}
