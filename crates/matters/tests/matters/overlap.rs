//! `after_turn_named` with an [`Overlap`]: another conversation's answer ran at the same time,
//! so its before-turn checkpoint may lie inside this turn. The changes are measured from the
//! turn's own before-turn checkpoint, and a paper only the other answer changed is left out, and
//! listed in `beside`; one an answer that has ended changed is left out only while it holds
//! what that answer's card left it with.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_matters::{CheckpointOutcome, Matter, Overlap, TurnMeta};
use clauseo_protocol::{ContentId, FileChange, MatterPath, ThreadId};

use crate::fixture::{Fixture, added, blob, cid, created, meta, modified, path, tools};

/// The other conversation, B.
fn other() -> ThreadId {
    "01920000-0000-7000-8000-000000000002".parse().unwrap()
}

/// Checkpoint `n` of a turn in conversation B.
fn meta_b(n: u64) -> TurnMeta {
    TurnMeta {
        thread: other(),
        ..meta(n)
    }
}

/// `others` naming each of `paths` as changed by B.
fn by_b(paths: &[&str]) -> BTreeMap<MatterPath, BTreeSet<ThreadId>> {
    paths
        .iter()
        .map(|at| (path(at), BTreeSet::from([other()])))
        .collect()
}

/// A's after-turn checkpoint `n`, its tool calls having written `named`, measured from `since`,
/// with B running still.
fn after_a(
    matter: &mut Matter,
    n: u64,
    named: &[&str],
    since: u64,
    others: &BTreeMap<MatterPath, BTreeSet<ThreadId>>,
) -> clauseo_matters::Checkpoint {
    after_a_left(matter, n, named, since, others, &BTreeMap::new())
}

/// [`after_a`], with the papers of `others` only answers that have ended named, and what their
/// cards left them with.
fn after_a_left(
    matter: &mut Matter,
    n: u64,
    named: &[&str],
    since: u64,
    others: &BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    left: &BTreeMap<MatterPath, Option<ContentId>>,
) -> clauseo_matters::Checkpoint {
    let overlap = Overlap {
        since: cid(since),
        others,
        left,
    };
    matter
        .after_turn_named(&meta(n), &tools(named), &BTreeSet::new(), Some(&overlap))
        .unwrap()
}

/// A turn of A started at checkpoint `since` == the last checkpoint, with nobody else named,
/// reports what `None` reports.
#[test]
fn from_the_last_checkpoint_with_no_others_is_as_without() {
    let run = |name: &str, with_overlap: bool| {
        let fx = Fixture::new(name);
        fx.write("a.docx", "one");
        fx.write("y.docx", "lawyer's");
        let mut matter = fx.import();
        fx.write("outside.docx", "between turns");
        assert_eq!(created(matter.before_turn(&meta(2)).unwrap(), 2).len(), 1);
        fx.write("a.docx", "two");
        fx.write("y.docx", "lawyer's, edited");
        fx.write("new.docx", "fresh");
        let named = tools(&["a.docx", "new.docx"]);
        let empty = BTreeMap::new();
        let left = BTreeMap::new();
        let overlap = Overlap {
            since: cid(2),
            others: &empty,
            left: &left,
        };
        matter
            .after_turn_named(
                &meta(3),
                &named,
                &BTreeSet::new(),
                with_overlap.then_some(&overlap),
            )
            .unwrap()
    };
    let without = run("overlap-same-none", false);
    let with = run("overlap-same-some", true);
    assert_eq!(with, without);
    assert_eq!(
        with.changes,
        [
            modified("a.docx", b"one", b"two"),
            added("new.docx", b"fresh"),
            modified("y.docx", b"lawyer's", b"lawyer's, edited").with_lawyer_edits(),
        ]
    );
}

/// A writes a.docx, B's turn starts (its before-turn checkpoint takes a.docx in), B writes
/// b.docx, A ends: A's answer changed a.docx only, unflagged, though before B's checkpoint.
#[test]
fn changes_since_the_turns_own_start_without_the_other_answers() {
    let fx = Fixture::new("overlap-own-start");
    fx.write("keep.docx", "kept");
    let mut matter = fx.import();
    assert_eq!(
        matter.before_turn(&meta(2)).unwrap().outcome,
        CheckpointOutcome::Reused(cid(1))
    );
    fx.write("a.docx", "A's");
    assert_eq!(
        created(matter.before_turn(&meta_b(3)).unwrap(), 3),
        [added("a.docx", b"A's")]
    );
    fx.write("b.docx", "B's");
    let others = by_b(&["b.docx"]);
    let changes = created(after_a(&mut matter, 4, &["a.docx"], 1, &others), 4);
    assert_eq!(changes, [added("a.docx", b"A's")]);
    assert!(!changes[0].may_include_lawyer_edits());
}

/// A paper both answers changed, before and after B's checkpoint, that A wrote: kept, naming B.
#[test]
fn a_paper_both_wrote_is_kept_naming_the_other() {
    let fx = Fixture::new("overlap-both");
    fx.write("x.docx", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("x.docx", "two");
    created(matter.before_turn(&meta_b(3)).unwrap(), 3);
    fx.write("x.docx", "three");
    let others = by_b(&["x.docx"]);
    let changes = created(after_a(&mut matter, 4, &["x.docx"], 1, &others), 4);
    assert_eq!(
        changes,
        [modified("x.docx", b"one", b"three").with_also_changed_by(vec![other()])]
    );
    assert!(!changes[0].may_include_lawyer_edits());
}

/// A paper neither answer named, changed during A's turn: kept and flagged, as without an
/// overlap; a paper only B named is left out.
#[test]
fn a_paper_nobody_named_is_flagged() {
    let fx = Fixture::new("overlap-unnamed");
    fx.write("y.docx", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("y.docx", "two");
    created(matter.before_turn(&meta_b(3)).unwrap(), 3);
    fx.write("y.docx", "three");
    fx.write("b.docx", "B's");
    let others = by_b(&["b.docx"]);
    let checkpoint = after_a(&mut matter, 4, &[], 1, &others);
    assert_eq!(checkpoint.beside, [path("b.docx")]);
    let changes = created(checkpoint, 4);
    assert_eq!(
        changes,
        [modified("y.docx", b"one", b"three").with_lawyer_edits()]
    );
    assert!(
        changes
            .iter()
            .all(|change| change.also_changed_by().is_empty())
    );
}

/// B ended having added b.docx, while A ran: b.docx is left out of A's card while it holds what
/// B's card left it with, and listed in `beside`.
#[test]
fn an_ended_answers_paper_as_it_left_it_is_left_out() {
    let fx = Fixture::new("overlap-ended-same");
    fx.write("keep.docx", "kept");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    matter.before_turn(&meta_b(3)).unwrap();
    fx.write("b.docx", "B's");
    let left = BTreeMap::from([(path("b.docx"), Some(blob(b"B's")))]);
    let others = by_b(&["b.docx"]);
    let checkpoint = after_a_left(&mut matter, 4, &[], 1, &others, &left);
    assert_eq!(checkpoint.beside, [path("b.docx")]);
    assert_eq!(created(checkpoint, 4), Vec::<FileChange>::new());
}

/// B ended having added b.docx, then the lawyer edited it while A ran: b.docx no longer holds
/// what B's card left it with, so it is on A's card, flagged, naming B; `beside` is empty.
#[test]
fn an_ended_answers_paper_changed_since_is_kept_flagged() {
    let fx = Fixture::new("overlap-ended-changed");
    fx.write("keep.docx", "kept");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    matter.before_turn(&meta_b(3)).unwrap();
    fx.write("b.docx", "B's, then the lawyer's");
    let left = BTreeMap::from([(path("b.docx"), Some(blob(b"B's")))]);
    let others = by_b(&["b.docx"]);
    let checkpoint = after_a_left(&mut matter, 4, &[], 1, &others, &left);
    assert_eq!(checkpoint.beside, Vec::<MatterPath>::new());
    assert_eq!(
        created(checkpoint, 4),
        [added("b.docx", b"B's, then the lawyer's")
            .with_lawyer_edits()
            .with_also_changed_by(vec![other()])]
    );
}

/// An ended answer deleted d.docx: left out of A's card while it is still gone.
#[test]
fn an_ended_answers_deletion_is_left_out_while_the_paper_is_gone() {
    let fx = Fixture::new("overlap-ended-deleted");
    fx.write("d.docx", "doomed");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    matter.before_turn(&meta_b(3)).unwrap();
    crate::support::remove_file(fx.file("d.docx"));
    let left = BTreeMap::from([(path("d.docx"), None)]);
    let others = by_b(&["d.docx"]);
    let checkpoint = after_a_left(&mut matter, 4, &[], 1, &others, &left);
    assert_eq!(checkpoint.beside, [path("d.docx")]);
    assert_eq!(created(checkpoint, 4), Vec::<FileChange>::new());
}

/// Nothing changed after B's checkpoint, so A's after-turn checkpoint reuses it; a.docx, which
/// A wrote before it, is still A's change.
#[test]
fn a_reused_checkpoint_reports_changes_since_the_turns_start() {
    let fx = Fixture::new("overlap-reused");
    fx.write("keep.docx", "kept");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.docx", "A's");
    matter.before_turn(&meta_b(3)).unwrap();
    let checkpoint = after_a(&mut matter, 4, &["a.docx"], 1, &BTreeMap::new());
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(3)));
    assert_eq!(checkpoint.changes, [added("a.docx", b"A's")]);
    assert_eq!(matter.timeline().len(), 2);

    // Without the overlap, the same reused checkpoint reports nothing.
    let without = matter.after_turn(&meta(5), &tools(&["a.docx"])).unwrap();
    assert_eq!(without.changes, Vec::<FileChange>::new());
}
