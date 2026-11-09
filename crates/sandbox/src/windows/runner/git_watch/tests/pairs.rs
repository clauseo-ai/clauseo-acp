//! The ruling's pairing of a folder made just now with its file ID leaving another watched
//! place: a forged creation time doesn't hide a folder moved in from the agent's temp folder,
//! whichever watch reads its record first, within 10 s either way; a `mkdir` whose file ID left
//! nowhere is held, then dropped by the creation-time rule, as is a folder's own leaving from
//! where it was made. What happens when the pairing loses track is `lost.rs`'s.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::pairs::{PAIR_WINDOW, Paired, Pairs, promote};
use crate::windows::set_aside::Live;

pub(super) const NOW: Duration = Duration::from_secs(100);
const MS: Duration = Duration::from_millis(1);

pub(super) fn matter(pairs: &Arc<Pairs>) -> Paired {
    Paired {
        pairs: Arc::clone(pairs),
        root: PathBuf::from("matter"),
    }
}

/// `x`, made just now with file ID 7, gone before its scan.
pub(super) fn gone_fresh() -> (Live, Vec<(PathBuf, u64)>) {
    let live = Live {
        unpaired: vec![PathBuf::from("x")],
        ..Live::default()
    };
    (live, vec![(PathBuf::from("x"), 7)])
}

/// `x` (file ID 7) leaving the agent's temp folder at `at`.
pub(super) fn left_temp(pairs: &Pairs, at: Duration) {
    pairs.note(at, Path::new("temp"), &[(PathBuf::from("x"), 7)]);
}

/// What `x`'s check at `at` counts gone.
pub(super) fn checked(pairs: &Arc<Pairs>, at: Duration) -> Vec<PathBuf> {
    let (mut live, fresh) = gone_fresh();
    promote(&mut live, &fresh, Some(&matter(pairs)), at);
    assert_eq!(live.unpaired, Vec::<PathBuf>::new());
    live.gone
}

fn x() -> Vec<PathBuf> {
    vec![PathBuf::from("x")]
}

fn none() -> Vec<PathBuf> {
    Vec::new()
}

/// A forged creation time: the folder looks made just now, but its file ID left the agent's
/// temp folder first: it counts gone at its check.
#[test]
fn a_fresh_folder_whose_file_id_left_the_agents_temp_counts_gone() {
    let pairs = Arc::new(Pairs::default());
    left_temp(&pairs, NOW);
    assert_eq!(checked(&pairs, NOW + PAIR_WINDOW), x());
    assert!(!pairs.lost(NOW));
}

/// The temp folder's watch reads its record after the matter's check, up to 10 s after: the
/// held folder pairs then, and the matter's watch takes it on its next record or idle tick.
#[test]
fn a_leaving_record_read_after_the_check_within_the_window_pairs() {
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), none());
    left_temp(&pairs, NOW + PAIR_WINDOW);
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW), x());
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW), none(), "taken once");

    // A leaving record a moment past the window pairs with nothing.
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), none());
    left_temp(&pairs, NOW + PAIR_WINDOW + MS);
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW + MS), none());
}

/// A `mkdir` whose file ID left nowhere: held for the window, then dropped by the creation-time
/// rule (made just now), so a leaving record after that pairs with nothing. With no pairing at
/// all, none counts.
#[test]
fn a_fresh_folder_nothing_pairs_with_by_the_windows_end_falls_back_to_the_time_rule() {
    let pairs = Arc::new(Pairs::default());
    pairs.note(NOW, Path::new("temp"), &[(PathBuf::from("other"), 8)]);
    assert_eq!(checked(&pairs, NOW), none());
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW), none());
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW + MS), none());
    // Dropped: a record noted as if still within the window no longer finds it.
    left_temp(&pairs, NOW + PAIR_WINDOW);
    assert_eq!(matter(&pairs).take(NOW + PAIR_WINDOW), none());

    // At the watch's stop, what is still held is dropped the same way.
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), none());
    assert_eq!(matter(&pairs).finish(), none());
    left_temp(&pairs, NOW + MS);
    assert_eq!(matter(&pairs).take(NOW + MS), none());

    let (mut live, fresh) = gone_fresh();
    promote(&mut live, &fresh, None, NOW);
    assert_eq!((live.gone, live.unpaired), (none(), none()));
}

/// A folder's own leaving from where it was made (`rm -r tmp`) never pairs, before its check
/// or after; leaving another folder of the matter does.
#[test]
fn a_folders_own_leaving_doesnt_pair_but_another_place_in_the_matter_does() {
    let pairs = Arc::new(Pairs::default());
    pairs.note(NOW, Path::new("matter"), &[(PathBuf::from("x"), 7)]);
    assert!(!pairs.paired(NOW, 7, &Path::new("matter").join("x")));
    assert_eq!(checked(&pairs, NOW), none());
    pairs.note(NOW, Path::new("matter"), &[(PathBuf::from("x"), 7)]);
    assert_eq!(matter(&pairs).take(NOW), none());
    pairs.note(
        NOW,
        Path::new("matter"),
        &[(PathBuf::from("a").join("x"), 7)],
    );
    assert!(pairs.paired(NOW, 7, &Path::new("matter").join("x")));
    assert_eq!(matter(&pairs).take(NOW), x(), "the held one paired");
}

/// What left more than [`PAIR_WINDOW`] ago pairs no more, and its going loses nothing.
#[test]
fn a_file_id_pairs_only_within_the_window() {
    let pairs = Pairs::default();
    left_temp(&pairs, NOW);
    let x = Path::new("matter").join("x");
    assert!(pairs.paired(NOW + PAIR_WINDOW, 7, &x));
    assert!(!pairs.paired(NOW + PAIR_WINDOW + MS, 7, &x));
    pairs.note(NOW + PAIR_WINDOW + MS, Path::new("temp"), &[]);
    assert!(!pairs.lost(NOW + PAIR_WINDOW + MS));
}

/// Each watch takes only the folders it held: the notes folder's watch doesn't take the
/// matter's.
#[test]
fn each_watch_takes_only_what_it_held() {
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), none());
    left_temp(&pairs, NOW);
    let notes = Paired {
        pairs: Arc::clone(&pairs),
        root: PathBuf::from("notes"),
    };
    assert_eq!(notes.take(NOW), none());
    assert_eq!(matter(&pairs).take(NOW), x());
}
