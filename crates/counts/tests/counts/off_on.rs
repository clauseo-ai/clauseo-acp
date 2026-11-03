//! Off and on again when the old folder is still there (security review of #259): nothing from
//! before off goes out under the new id, and an answer to a POST from before off matches no POST
//! made after it.

use clauseo_counts::schema::{Crash, CrashKind, CrashProcess, Event, InstallId, TallyReport};
use clauseo_counts::{Mode, Outcome, UploadPath};

use crate::support::*;

/// Every queued day sent among `sent`, with the id it went under.
fn days_sent(sent: &[(clauseo_counts::schema::Time, Seen)]) -> Vec<String> {
    sent.iter()
        .filter(|(_, seen)| seen.path == UploadPath::Counts)
        .flat_map(|(_, seen)| batch_of(seen).days)
        .map(|day| format!("{} {}", day.day, day.install))
        .collect()
}

#[cfg(unix)]
#[test]
fn on_again_stays_off_while_the_old_folder_cannot_be_deleted() {
    let scratch = Scratch::new("delete-fails");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    let old_id = counts.install_id().unwrap();
    let queue = counts_dir(&scratch).join("queue");
    set_read_only(&queue, true);
    if can_write_in(&queue) {
        set_read_only(&queue, false);
        eprintln!("skipped: running as root, a read-only folder does not stop the delete");
        return;
    }
    assert!(counts.set_enabled(false).is_err());
    assert_eq!(counts.mode(), Mode::Off);
    // On again while the old days are still on disk: refused, and counts stay off.
    assert!(counts.set_enabled(true).is_err());
    assert_eq!(counts.mode(), Mode::Off);
    assert_eq!(counts.install_id(), None);

    // Once the delete can go through, on again works, with a new id and nothing old.
    set_read_only(&queue, false);
    counts.set_enabled(true).unwrap();
    assert_eq!(counts.mode(), Mode::Saving);
    let new_id = counts.install_id().unwrap();
    assert_ne!(new_id, old_id);
    assert_eq!(files(&queue).len(), 0);
    drop(counts);
    let counts = saving(&scratch);
    let upload = FakeUpload::answering(&[Ok(200)]);
    let sent = run(&counts, &upload, at(4, 0), at(4, DAY - 1), 60);
    assert_eq!(days_sent(&sent), Vec::<String>::new());
}

#[test]
fn a_day_or_tally_left_under_another_id_is_removed_and_never_sent() {
    let scratch = Scratch::new("other-id");
    let counts = saving(&scratch);
    counts.tick(at(2, HOUR)).unwrap();
    counts.bump(Event::Crash(Crash {
        process: CrashProcess::Host,
        kind: CrashKind::Panic,
        after_launch: true,
    }));
    fill_days(&counts, 2, 3);
    let id = counts.install_id().unwrap().to_string();
    drop(counts);
    // What an off leaves when it deletes `install_id` and fails on the rest (on Windows, a file
    // another program holds open): the next open makes a new id beside the old files.
    let dir = counts_dir(&scratch);
    let other = InstallId::random().to_string();
    for path in [
        dir.join("queue/2026-11-02.json"),
        dir.join("tally/2026-W45.json"),
    ] {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(&id), "{text}");
        plant(&path, text.replace(&id, &other).as_bytes());
    }
    plant(&dir.join("install_id"), other.as_bytes());
    let counts = saving(&scratch);
    counts.tick(at(4, 60)).unwrap();
    assert_eq!(counts.install_id().map(|id| id.to_string()), Some(other));
    assert_eq!(counts.queued_days(), vec![day(2)]);

    // And the other way round: the files' id is not the folder's.
    drop(counts);
    plant(
        &dir.join("install_id"),
        InstallId::random().to_string().as_bytes(),
    );
    let counts = saving(&scratch);
    counts.tick(at(4, 120)).unwrap();
    assert_eq!(counts.queued_days().len(), 0);
    assert_eq!(files(&dir.join("queue")).len(), 0);
    // The week's tally starts again under the new id, without the old crash.
    let upload = FakeUpload::answering(&[Ok(200)]);
    let sent = run(&counts, &upload, at(4, 180), at(16, 0), 600);
    assert_eq!(days_sent(&sent), Vec::<String>::new());
    let tallies: Vec<_> = sent
        .iter()
        .filter(|(_, seen)| seen.path == UploadPath::Tally)
        .map(|(_, seen)| serde_json::from_slice::<TallyReport>(&gunzip(&seen.body)).unwrap())
        .collect();
    assert_eq!(tallies.len(), 1);
    assert_eq!(tallies[0].cells.crash.len(), 0);
}

#[test]
fn an_answer_to_a_post_from_before_off_matches_no_post_after_on() {
    let scratch = Scratch::new("late-answer");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    let now = at(4, 700);
    counts.tick(now).unwrap();
    let old = counts.due(now).pop().unwrap();
    counts.set_enabled(false).unwrap();
    counts.set_enabled(true).unwrap();
    fill_days(&counts, 4, 5);
    let later = at(6, 700);
    counts.tick(later).unwrap();
    let new = counts.due(later).pop().unwrap();
    assert_ne!(old.id(), new.id());
    let queued = counts.queued_days();
    assert_eq!(queued, vec![day(4), day(5)]);
    counts.sent(old.id(), Outcome::Status(200));
    assert_eq!(counts.queued_days(), queued);
    // The new POST's own answer still counts.
    counts.sent(new.id(), Outcome::Status(200));
    assert_eq!(counts.queued_days().len(), 0);
}

#[test]
fn a_folder_where_a_counts_file_should_be_is_removed() {
    let scratch = Scratch::new("folders");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    drop(counts);
    let dir = counts_dir(&scratch);
    plant(&dir.join("queue/2026-11-01.json/x"), b"{}");
    plant(&dir.join("queue/sub/x.json"), b"{}");
    plant(&dir.join("tally/2026-W44.json/x"), b"{}");
    remove(&dir.join("today.json"));
    plant(&dir.join("today.json/x"), b"{}");
    let counts = saving(&scratch);
    counts.tick(at(4, 60)).unwrap();
    assert_eq!(counts.queued_days(), vec![day(2), day(3)]);
    assert_eq!(
        files(&dir.join("queue")),
        ["2026-11-02.json", "2026-11-03.json"]
    );
    counts.bump(COLD);
    counts.tick(at(4, 120)).unwrap();
    assert!(dir.join("today.json").is_file());
}
