//! The verdict of the `end-cleanup` row (`checks_cleanup.rs`), from the lines `probe items`
//! prints as the pool user: `item <kind> <planted|present|absent|removed|error <why>>`, one per
//! kind in [`KINDS`], the BITS job's `present` followed by its state, and, from a check,
//! `marker bits <present|absent>`: whether the job's notify command ran. No Win32 call here, so
//! the rule has a unit test.
//!
//! The items are planted before the session, so that session is the next one for them. Its
//! inner runner's first act removes the HKCU `Run` and `RunOnce` values and the Startup entries
//! ([`FIRST_ACT`]); at its end the outer asks for the `SYSTEM` task `\Clauseo\SandboxSweep`,
//! which deletes the pool user's scheduled tasks and cancels its BITS jobs ([`SWEEP`]); the inner
//! touches neither. A check after the session's end sees both.
//!
//! BITS may refuse the job outright: `AddFile` (or `Resume`) fails with `ERROR_NOT_LOGGED_ON`
//! (0x800704dd), as it did on the test VMs. That refusal proves the job can't run: no job of the
//! pool user's can get a file, so none can transfer or set off its notify command. Such a plant
//! passes, the marker still required absent, and a half-made job still there is information;
//! any other plant error is INFO.

use super::cancel_verdict::sweep_task;
use super::checks::values;
use super::pool_verdict::Verdict;

/// The kinds of item, in the order they are planted: the HKCU `Run` and `RunOnce` values, a
/// Startup-folder entry, a scheduled task and a BITS job.
pub(crate) const KINDS: [&str; 5] = ["run", "runonce", "startup", "task", "bits"];

/// The kinds the next session's first act removes.
const FIRST_ACT: [&str; 3] = ["run", "runonce", "startup"];

/// The kinds besides BITS that the sandbox sweep task removes after a session's end.
const SWEEP: [&str; 1] = ["task"];

/// The BITS states of a job that ran: its file transferred, which sets off its notify command.
const RAN: [&str; 2] = ["TRANSFERRED", "ACKNOWLEDGED"];

/// The HRESULT of `ERROR_NOT_LOGGED_ON`, as `bitsadmin` prints it and as its exit code.
const NOT_LOGGED_ON: [&str; 2] = ["0x800704dd", "-2147023651"];

/// The BITS plant steps whose refusal for want of a logon proves the job can't run.
const REFUSED_STEPS: [&str; 2] = ["addfile:", "resume:"];

/// Whether `word`, the BITS plant's, is `AddFile` or `Resume` refused with exactly
/// `ERROR_NOT_LOGGED_ON`.
fn refused(word: &str) -> bool {
    let Some(why) = word.strip_prefix("error ") else {
        return false;
    };
    let why = why.to_ascii_lowercase();
    REFUSED_STEPS.iter().any(|step| why.starts_with(step))
        && NOT_LOGGED_ON.iter().any(|code| why.contains(code))
}

/// The word `probe items` printed for `kind` in `lines`.
fn item<'l>(lines: &'l [String], kind: &str) -> Option<&'l str> {
    values(lines, &format!("item {kind}")).first().copied()
}

/// Whether the check after the session, `after`, needs no retry: its verdict isn't a FAIL.
pub(crate) fn settled(planted: &[String], after: &[String]) -> bool {
    !matches!(end_cleanup(planted, after, &[], 0), Verdict::Fail(_))
}

/// The BITS job in the check: `Ok(None)` gone, `Ok(Some(state))` present and not run, `Err` a
/// failure (it ran) or a check that didn't say.
fn bits_in(lines: &[String]) -> Result<Option<String>, (bool, String)> {
    match values(lines, "marker bits").first().copied() {
        Some("absent") => {}
        Some("present") => {
            return Err((
                true,
                "the BITS job's notify command ran (marker)".to_owned(),
            ));
        }
        word => {
            return Err((
                false,
                format!("bits marker ({})", word.unwrap_or("no line")),
            ));
        }
    }
    let word = item(lines, "bits").unwrap_or("no line");
    match word.split_once(' ') {
        None if word == "absent" => Ok(None),
        Some(("present", state)) if RAN.contains(&state) => {
            Err((true, format!("the BITS job ran: its state was {state}")))
        }
        Some(("present", state)) => Ok(Some(state.to_owned())),
        _ => Err((false, format!("bits ({word})"))),
    }
}

/// `end-cleanup`: `planted`, the lines of the plant before the session; `after`, those of the
/// last check, `waited_ms` after the session ended; `stderr`, the session's outer's. PASS when
/// the four other kinds were planted and are now absent, and the BITS job is absent (cancelled
/// by the sweep task) or BITS refused it (above); FAIL naming the kinds left (or not planted, or
/// not checked), or when the BITS job ran; INFO when only the BITS job couldn't be planted
/// otherwise (BITS unavailable) and the rest are gone. Each verdict names the outer's lines about
/// the sandbox sweep task, as information.
pub(crate) fn end_cleanup(
    planted: &[String],
    after: &[String],
    stderr: &[String],
    waited_ms: u128,
) -> Verdict {
    let said = sweep_task(stderr);
    let (mut left, mut unchecked, mut unplanted) = (vec![], vec![], vec![]);
    let (mut first_act, mut swept) = (vec![], vec![]);
    for kind in FIRST_ACT.into_iter().chain(SWEEP) {
        match item(planted, kind) {
            Some("planted") => {}
            word => {
                unplanted.push(format!("{kind} ({})", word.unwrap_or("no line")));
                continue;
            }
        }
        let by = if FIRST_ACT.contains(&kind) {
            "the session's first act"
        } else {
            "the sandbox sweep task"
        };
        match item(after, kind) {
            Some("absent") if FIRST_ACT.contains(&kind) => first_act.push(kind),
            Some("absent") => swept.push(kind),
            Some("present") => left.push(format!("{kind} (left by {by})")),
            word => unchecked.push(format!("{kind} ({})", word.unwrap_or("no line"))),
        }
    }
    let bits_word = item(planted, "bits");
    let bits_planted = bits_word == Some("planted");
    let bits_refused = bits_word.is_some_and(refused);
    let mut there = None;
    match bits_in(after) {
        Err((true, ran)) => return Verdict::Fail(format!("{ran}; {said}")),
        Err((false, why)) if bits_planted || bits_refused => unchecked.push(why),
        Err(_) | Ok(None) => {}
        Ok(Some(state)) if bits_planted => left.push(format!(
            "bits (state {state}; not cancelled by the sandbox sweep task)"
        )),
        Ok(Some(state)) => {
            there = Some(format!(
                "a half-made BITS job is still there (state {state}), unable to run"
            ));
        }
    }
    if bits_planted && left.iter().all(|kind| !kind.starts_with("bits")) {
        swept.push("bits");
    }
    if !left.is_empty() {
        return Verdict::Fail(format!(
            "{waited_ms} ms after the session ended the pool user still has: {}{}; {said}",
            left.join(", "),
            if unchecked.is_empty() {
                String::new()
            } else {
                format!("; not checked: {}", unchecked.join(", "))
            }
        ));
    }
    if !unplanted.is_empty() || !unchecked.is_empty() {
        return Verdict::Fail(format!(
            "not planted: {unplanted:?}; not checked after the session: {unchecked:?}; {said}"
        ));
    }
    let mut note = format!(
        "{} gone by the session's first act, {} gone by the sandbox sweep task, as checked {} ms \
         after the session ended",
        first_act.join(", "),
        swept.join(", "),
        waited_ms
    );
    for info in there.into_iter().chain([said]) {
        note = format!("{note}; {info}");
    }
    match bits_word {
        Some("planted") => Verdict::Pass(note),
        _ if bits_refused => Verdict::Pass(format!(
            "BITS refuses the pool user (0x800704dd), so no job of theirs can run; {note}"
        )),
        why => Verdict::Info(format!(
            "no BITS job could be planted ({}), so that item proves nothing; {note}",
            why.unwrap_or("no line")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, end_cleanup, settled};

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
    /// Every item gone but the BITS job, left without having run.
    const LEFT: [&str; 6] = [
        "item run absent",
        "item runonce absent",
        "item startup absent",
        "item task absent",
        "item bits present TRANSIENT_ERROR",
        "marker bits absent",
    ];

    const FAILED: &str = "clauseo-sandbox-run: warning: the sandbox sweep task ended 0x80070005";

    fn judged(planted: &[&str], after: &[&str]) -> Verdict {
        end_cleanup(&lines(planted), &lines(after), &lines(&[FAILED]), 4_000)
    }

    #[test]
    fn end_cleanup_passes_naming_who_removed_each_kind() {
        let Verdict::Pass(note) = judged(&PLANTED, &GONE) else {
            panic!("not a PASS");
        };
        assert!(settled(&lines(&PLANTED), &lines(&GONE)));
        for part in [
            "run, runonce, startup gone by the session's first act",
            "task, bits gone by the sandbox sweep task",
            "4000 ms",
            "ended 0x80070005",
        ] {
            assert!(note.contains(part), "{note} doesn't name {part}");
        }
    }

    #[test]
    fn end_cleanup_fails_a_bits_job_the_sweep_task_left() {
        assert!(!settled(&lines(&PLANTED), &lines(&LEFT)));
        let verdict = judged(&PLANTED, &LEFT);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("bits (state TRANSIENT_ERROR; \
                not cancelled by the sandbox sweep task)")),
            "{verdict:?}"
        );
    }

    #[test]
    fn end_cleanup_fails_when_the_bits_job_ran() {
        for after in [
            with(LEFT, &[(5, "marker bits present")]),
            with(GONE, &[(5, "marker bits present")]),
        ] {
            let verdict = judged(&PLANTED, &after);
            assert!(
                matches!(&verdict, Verdict::Fail(why) if why.contains("notify command ran")),
                "{verdict:?}"
            );
        }
        let transferred = with(LEFT, &[(4, "item bits present TRANSFERRED")]);
        let verdict = judged(&PLANTED, &transferred);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("TRANSFERRED")),
            "{verdict:?}"
        );
        let unmarked = with(GONE, &[(5, "")]);
        let verdict = judged(&PLANTED, &unmarked);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("bits marker")),
            "{verdict:?}"
        );
    }

    /// What the test VMs printed: `AddFile` refused for want of a logon.
    const REFUSED: &str = "item bits error addfile: bitsadmin.exe exited -2147023651: Unable to \
                           add file to job - 0x800704dd";

    #[test]
    fn end_cleanup_passes_when_bits_refuses_the_pool_users_job() {
        let suspended = with(LEFT, &[(4, "item bits present SUSPENDED")]);
        let resume = "item bits error resume: bitsadmin.exe exited -2147023651: 0x800704DD";
        for word in [REFUSED, resume] {
            let planted = with(PLANTED, &[(4, word)]);
            for after in [GONE, suspended] {
                let Verdict::Pass(note) = judged(&planted, &after) else {
                    panic!("not a PASS: {word}");
                };
                assert!(
                    note.starts_with("BITS refuses the pool user (0x800704dd)"),
                    "{note}"
                );
                assert!(
                    note.contains("task gone by the sandbox sweep task"),
                    "{note}"
                );
                // A half-made job still there is recorded, and decides nothing.
                let there = note.contains("half-made BITS job is still there (state SUSPENDED)");
                assert_eq!(there, after == suspended, "{note}");
            }
        }
    }

    #[test]
    fn end_cleanup_fails_a_refused_bits_job_whose_marker_appears_or_isnt_checked() {
        let planted = with(PLANTED, &[(4, REFUSED)]);
        let fired = with(GONE, &[(5, "marker bits present")]);
        let verdict = judged(&planted, &fired);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("notify command ran")),
            "{verdict:?}"
        );
        let unmarked = with(GONE, &[(5, "")]);
        let verdict = judged(&planted, &unmarked);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("bits marker")),
            "{verdict:?}"
        );
    }

    #[test]
    fn end_cleanup_fails_naming_the_kinds_left_and_who_left_them() {
        let after = with(
            GONE,
            &[(1, "item runonce present"), (3, "item task present")],
        );
        let Verdict::Fail(why) = judged(&PLANTED, &after) else {
            panic!("not a FAIL");
        };
        assert!(
            why.contains(
                "runonce (left by the session's first act), task (left by the sandbox \
                 sweep task)"
            ),
            "{why}"
        );
        assert!(!why.contains("startup"), "{why}");
        assert!(why.contains("ended 0x80070005"), "{why}");
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
        let verdict = judged(&planted, &GONE);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("task")),
            "{verdict:?}"
        );
        let partial = GONE.get(..2).unwrap_or_default();
        let verdict = judged(&PLANTED, partial);
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
        let verdict = judged(&planted, &GONE);
        assert!(
            matches!(&verdict, Verdict::Info(why) if why.contains("BITS")),
            "{verdict:?}"
        );
        // A job a failed plant left half made is no failure either; nor is a refusal by another
        // step, or with another code.
        for other in [
            "item bits error create: bitsadmin.exe exited -2147023651: 0x800704dd",
            "item bits error addfile: bitsadmin.exe exited 1: Unable to add file - 0x80070005",
        ] {
            let planted = with(PLANTED, &[(4, other)]);
            let verdict = judged(&planted, &LEFT);
            assert!(matches!(verdict, Verdict::Info(_)), "{other}: {verdict:?}");
        }
        let after = with(GONE, &[(0, "item run present")]);
        let verdict = judged(&planted, &after);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
    }
}
