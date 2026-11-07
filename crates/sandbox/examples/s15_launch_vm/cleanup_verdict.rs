//! The verdict of the `end-cleanup` row (`checks_cleanup.rs`), from the lines `probe items`
//! prints as the pool user: `item <kind> <planted|present|absent|removed|error <why>>`, one per
//! kind in [`KINDS`], the BITS job's `present` followed by its state, and, from a check,
//! `marker bits <present|absent>`: whether the job's notify command ran. No Win32 call here, so
//! the rule has a unit test.
//!
//! The BITS job may be left: BITS holds no logon token for a pool user, who logs on only through
//! `CreateProcessWithLogonW`, so the end sweeper's `Cancel` fails with 1245 and the job is left
//! for the `SYSTEM` stop task (0049). Until that task lands (PR #668) the job passes when it is
//! absent, or present but inert: still there after a second session of the same pool user, its
//! notify command never run (no marker) and its state never `TRANSFERRED` or `ACKNOWLEDGED`.
//! After #668 the row will also require the job to be gone once the system stop has run.

use super::checks::values;
use super::pool_verdict::Verdict;

/// The kinds of item, in the order they are planted: the HKCU `Run` and `RunOnce` values, a
/// Startup-folder entry, a scheduled task and a BITS job.
pub(crate) const KINDS: [&str; 5] = ["run", "runonce", "startup", "task", "bits"];

/// The BITS states of a job that ran: its file transferred, which sets off its notify command.
const RAN: [&str; 2] = ["TRANSFERRED", "ACKNOWLEDGED"];

/// The word `probe items` printed for `kind` in `lines`.
fn item<'l>(lines: &'l [String], kind: &str) -> Option<&'l str> {
    values(lines, &format!("item {kind}")).first().copied()
}

/// Whether the BITS job is still there in `lines`, a check's.
pub(crate) fn bits_left(lines: &[String]) -> bool {
    item(lines, "bits").is_some_and(|word| word.split(' ').next() == Some("present"))
}

/// Whether the first check after the session, `after`, needs no retry: the verdict with that
/// check standing in for the second session's too isn't a FAIL.
pub(crate) fn settled(planted: &[String], after: &[String]) -> bool {
    !matches!(
        end_cleanup(planted, after, Some(after), 0),
        Verdict::Fail(_)
    )
}

/// The BITS job in one check: `Ok(None)` gone, `Ok(Some(state))` present but inert so far, `Err`
/// a failure (it ran) or a check that didn't say.
fn bits_in(lines: &[String], when: &str) -> Result<Option<String>, (bool, String)> {
    match values(lines, "marker bits").first().copied() {
        Some("absent") => {}
        Some("present") => {
            return Err((
                true,
                format!("the BITS job's notify command ran (marker, {when})"),
            ));
        }
        word => {
            return Err((
                false,
                format!("bits marker {when} ({})", word.unwrap_or("no line")),
            ));
        }
    }
    let word = item(lines, "bits").unwrap_or("no line");
    match word.split_once(' ') {
        None if word == "absent" => Ok(None),
        Some(("present", state)) if RAN.contains(&state) => Err((
            true,
            format!("the BITS job ran: its state was {state} {when}"),
        )),
        Some(("present", state)) => Ok(Some(state.to_owned())),
        _ => Err((false, format!("bits {when} ({word})"))),
    }
}

/// `end-cleanup`: `planted`, the lines of the plant before the session; `after`, those of the
/// last check, `waited_ms` after the session ended; `again`, those of the check after a second
/// session of the same pool user, when one ran. PASS when the four other kinds were planted and
/// are now absent, and the BITS job is absent or present but inert (above); FAIL naming the
/// kinds left (or not planted, or not checked), or when the BITS job ran; INFO when only the BITS
/// job couldn't be planted (BITS unavailable) and the rest are gone.
pub(crate) fn end_cleanup(
    planted: &[String],
    after: &[String],
    again: Option<&[String]>,
    waited_ms: u128,
) -> Verdict {
    let (mut left, mut unchecked, mut unplanted, mut gone) = (vec![], vec![], vec![], vec![]);
    for kind in KINDS.into_iter().filter(|kind| *kind != "bits") {
        match item(planted, kind) {
            Some("planted") => {}
            word => {
                unplanted.push(format!("{kind} ({})", word.unwrap_or("no line")));
                continue;
            }
        }
        match item(after, kind) {
            Some("absent") => gone.push(kind.to_owned()),
            Some("present") => left.push(kind),
            word => unchecked.push(format!("{kind} ({})", word.unwrap_or("no line"))),
        }
    }
    if !left.is_empty() {
        return Verdict::Fail(format!(
            "{waited_ms} ms after the session ended the pool user still has: {}{}",
            left.join(", "),
            if unchecked.is_empty() {
                String::new()
            } else {
                format!("; not checked: {}", unchecked.join(", "))
            }
        ));
    }
    let bits_planted = item(planted, "bits") == Some("planted");
    let mut checks = vec![(after, "after the session")];
    if let Some(again) = again {
        checks.push((again, "after a second session"));
    }
    let mut states = Vec::new();
    for (lines, when) in checks {
        match bits_in(lines, when) {
            Err((true, ran)) => return Verdict::Fail(ran),
            Err((false, why)) if bits_planted => unchecked.push(why),
            Err(_) => {}
            Ok(state) => states.push(state),
        }
    }
    let inert = match states.as_slice() {
        _ if !bits_planted => None,
        [None, ..] => {
            gone.push("bits".to_owned());
            None
        }
        [Some(_)] if again.is_none() => {
            unchecked.push("bits (present, not checked after a second session)".to_owned());
            None
        }
        [Some(first), Some(second)] => Some(format!(
            "the BITS job is left but inert (state {first}, then {second} after a second session; \
             its notify command never ran), for the system stop"
        )),
        [Some(_), None] => {
            gone.push("bits (after a second session)".to_owned());
            None
        }
        _ => None,
    };
    if !unplanted.is_empty() || !unchecked.is_empty() {
        return Verdict::Fail(format!(
            "not planted: {unplanted:?}; not checked after the session: {unchecked:?}"
        ));
    }
    let mut note = format!(
        "{} gone {waited_ms} ms after the session ended",
        gone.join(", ")
    );
    if let Some(inert) = inert {
        note = format!("{note}; {inert}");
    }
    match item(planted, "bits") {
        Some("planted") => Verdict::Pass(note),
        why => Verdict::Info(format!(
            "no BITS job could be planted ({}), so that item proves nothing; {note}",
            why.unwrap_or("no line")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, bits_left, end_cleanup, settled};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    /// `base` with each `(at, line)` put in.
    fn with(base: [&'static str; 6], swaps: &[(usize, &'static str)]) -> [&'static str; 6] {
        let mut changed = base;
        for (at, line) in swaps {
            if let Some(slot) = changed.get_mut(*at) {
                *slot = line;
            }
        }
        changed
    }

    const PLANTED: [&str; 6] = [
        "item run planted",
        "item runonce planted",
        "item startup planted",
        "item task planted",
        "item bits planted",
        "",
    ];
    const GONE: [&str; 6] = [
        "item run absent",
        "item runonce absent",
        "item startup absent",
        "item task absent",
        "item bits absent",
        "marker bits absent",
    ];
    /// Every item gone but the BITS job, left inert.
    const INERT: [&str; 6] = [
        "item run absent",
        "item runonce absent",
        "item startup absent",
        "item task absent",
        "item bits present TRANSIENT_ERROR",
        "marker bits absent",
    ];

    fn judged(planted: &[&str], after: &[&str], again: Option<&[&str]>) -> Verdict {
        let again = again.map(lines);
        end_cleanup(&lines(planted), &lines(after), again.as_deref(), 4_000)
    }

    #[test]
    fn end_cleanup_passes_when_every_planted_item_is_gone() {
        let verdict = judged(&PLANTED, &GONE, None);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        assert!(!bits_left(&lines(&GONE)));
    }

    #[test]
    fn end_cleanup_passes_a_bits_job_left_inert_after_a_second_session() {
        assert!(bits_left(&lines(&INERT)));
        assert!(settled(&lines(&PLANTED), &lines(&INERT)));
        let again = with(INERT, &[(4, "item bits present QUEUED")]);
        let Verdict::Pass(note) = judged(&PLANTED, &INERT, Some(&again)) else {
            panic!("not a PASS");
        };
        assert!(note.contains("TRANSIENT_ERROR, then QUEUED"), "{note}");
        // Gone by the second check: a PASS too.
        let verdict = judged(&PLANTED, &INERT, Some(&GONE));
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
    }

    #[test]
    fn end_cleanup_fails_when_the_bits_job_ran() {
        let fired = with(INERT, &[(5, "marker bits present")]);
        for (after, again) in [
            (fired, None),
            (INERT, Some(fired)),
            (with(GONE, &[(5, "marker bits present")]), None),
        ] {
            let verdict = judged(&PLANTED, &after, again.as_ref().map(<[&str; 6]>::as_slice));
            assert!(
                matches!(&verdict, Verdict::Fail(why) if why.contains("notify command ran")),
                "{verdict:?}"
            );
        }
        let transferred = with(INERT, &[(4, "item bits present TRANSFERRED")]);
        let verdict = judged(&PLANTED, &INERT, Some(&transferred));
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("TRANSFERRED")),
            "{verdict:?}"
        );
        assert!(!settled(&lines(&PLANTED), &lines(&transferred)));
    }

    #[test]
    fn end_cleanup_fails_a_bits_job_left_but_not_checked_again() {
        let verdict = judged(&PLANTED, &INERT, None);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("second session")),
            "{verdict:?}"
        );
        let unmarked = with(INERT, &[(5, "")]);
        let verdict = judged(&PLANTED, &INERT, Some(&unmarked));
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("bits marker")),
            "{verdict:?}"
        );
    }

    #[test]
    fn end_cleanup_fails_naming_the_kinds_left() {
        let after = with(
            GONE,
            &[(1, "item runonce present"), (3, "item task present")],
        );
        let Verdict::Fail(why) = judged(&PLANTED, &after, None) else {
            panic!("not a FAIL");
        };
        assert!(why.contains("runonce, task"), "{why}");
        assert!(!why.contains("startup"), "{why}");
    }

    #[test]
    fn end_cleanup_fails_on_an_item_not_planted_or_not_checked() {
        let planted = with(
            PLANTED,
            &[(
                3,
                "item task error schtasks.exe exited 1: ERROR: Access is denied.",
            )],
        );
        let verdict = judged(&planted, &GONE, None);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("task")),
            "{verdict:?}"
        );
        let partial = GONE.get(..2).unwrap_or_default();
        let verdict = judged(&PLANTED, partial, None);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("startup")),
            "{verdict:?}"
        );
    }

    #[test]
    fn end_cleanup_is_info_when_only_bits_couldnt_be_planted() {
        let planted = with(
            PLANTED,
            &[(
                4,
                "item bits error bitsadmin.exe exited 1: BITS is disabled",
            )],
        );
        let verdict = judged(&planted, &GONE, None);
        assert!(
            matches!(&verdict, Verdict::Info(why) if why.contains("BITS")),
            "{verdict:?}"
        );
        // A job a failed plant left half made is no failure either.
        let verdict = judged(&planted, &INERT, None);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        let after = with(GONE, &[(0, "item run present")]);
        let verdict = judged(&planted, &after, None);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
    }
}
