//! The verdict of the `end-cleanup` row (`checks_cleanup.rs`), from the lines `probe items`
//! prints as the pool user: `item <kind> <planted|present|absent|removed|error <why>>`, one per
//! kind in [`KINDS`]. No Win32 call here, so the rule has a unit test.

use super::checks::values;
use super::pool_verdict::Verdict;

/// The kinds of item, in the order they are planted: the HKCU `Run` and `RunOnce` values, a
/// Startup-folder entry, a scheduled task and a BITS job.
pub(crate) const KINDS: [&str; 5] = ["run", "runonce", "startup", "task", "bits"];

/// The word `probe items` printed for `kind` in `lines`.
fn item<'l>(lines: &'l [String], kind: &str) -> Option<&'l str> {
    values(lines, &format!("item {kind}")).first().copied()
}

/// `end-cleanup`: `planted`, the lines of the plant before the session; `after`, those of the
/// last check, `waited_ms` after the session ended. PASS when every kind was planted and is now
/// absent; FAIL naming the kinds left (or not planted, or not checked); INFO when only the BITS
/// job couldn't be planted (BITS unavailable) and the rest are gone.
pub(crate) fn end_cleanup(planted: &[String], after: &[String], waited_ms: u128) -> Verdict {
    let (mut left, mut unchecked, mut unplanted, mut gone) = (vec![], vec![], vec![], vec![]);
    let mut bits = None;
    for kind in KINDS {
        match item(planted, kind) {
            Some("planted") => {}
            Some(word) if kind == "bits" => {
                bits = Some(word.to_owned());
                continue;
            }
            word => {
                unplanted.push(format!("{kind} ({})", word.unwrap_or("no line")));
                continue;
            }
        }
        match item(after, kind) {
            Some("absent") => gone.push(kind),
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
    if !unplanted.is_empty() || !unchecked.is_empty() {
        return Verdict::Fail(format!(
            "not planted: {unplanted:?}; not checked after the session: {unchecked:?}"
        ));
    }
    let note = format!(
        "{} gone {waited_ms} ms after the session ended",
        gone.join(", ")
    );
    match bits {
        Some(why) => Verdict::Info(format!(
            "no BITS job could be planted ({why}), so that item proves nothing; {note}"
        )),
        None => Verdict::Pass(note),
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, end_cleanup};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    /// `base` with each `(at, line)` put in.
    fn with(base: [&'static str; 5], swaps: &[(usize, &'static str)]) -> [&'static str; 5] {
        let mut changed = base;
        for (at, line) in swaps {
            if let Some(slot) = changed.get_mut(*at) {
                *slot = line;
            }
        }
        changed
    }

    const PLANTED: [&str; 5] = [
        "item run planted",
        "item runonce planted",
        "item startup planted",
        "item task planted",
        "item bits planted",
    ];
    const GONE: [&str; 5] = [
        "item run absent",
        "item runonce absent",
        "item startup absent",
        "item task absent",
        "item bits absent",
    ];

    #[test]
    fn end_cleanup_passes_when_every_planted_item_is_gone() {
        let verdict = end_cleanup(&lines(&PLANTED), &lines(&GONE), 4_000);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
    }

    #[test]
    fn end_cleanup_fails_naming_the_kinds_left() {
        let after = with(
            GONE,
            &[(1, "item runonce present"), (3, "item task present")],
        );
        let Verdict::Fail(why) = end_cleanup(&lines(&PLANTED), &lines(&after), 30_000) else {
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
        let verdict = end_cleanup(&lines(&planted), &lines(&GONE), 1_000);
        assert!(
            matches!(&verdict, Verdict::Fail(why) if why.contains("task")),
            "{verdict:?}"
        );
        let partial = lines(GONE.get(..2).unwrap_or_default());
        let verdict = end_cleanup(&lines(&PLANTED), &partial, 30_000);
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
        let verdict = end_cleanup(&lines(&planted), &lines(&GONE), 2_000);
        assert!(
            matches!(&verdict, Verdict::Info(why) if why.contains("BITS")),
            "{verdict:?}"
        );
        let after = with(GONE, &[(0, "item run present")]);
        let verdict = end_cleanup(&lines(&planted), &lines(&after), 30_000);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
    }
}
