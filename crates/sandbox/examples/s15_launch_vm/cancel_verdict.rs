//! The verdict of the `cancel` row (`checks_cancel.rs`), from the lines the harness prints: no
//! Win32 call here, so each rule has a unit test.
//!
//! The harness's lines, one set per look after it killed the outer runner (`<ms>` after the
//! kill):
//!
//! - `look <ms> <n>`: the look read every process's threads (`win_suspend::thread_states`) and
//!   saw `n` runner processes (`clauseo-sandbox-run.exe`) of any user; or `suspend-error <ms>
//!   <ntstatus>` when `NtQuerySystemInformation` failed.
//! - `runner <ms> <pid> <suspended|running> pool` per runner process of the pool user, and
//!   `runner <ms> <pid> <suspended|running> unreadable <code>` per one whose user the harness
//!   couldn't read. `suspended`: every thread waits, suspended. Another user's isn't listed.

use std::collections::BTreeSet;

use super::checks::values;
use super::pool_verdict::Verdict;

/// What the runner logs on stderr when it starts no end sweeper.
pub(crate) const NO_SWEEPER: &str = "clauseo-sandbox-run: warning: no end sweeper";

/// One `runner` line.
struct Runner<'l> {
    ms: &'l str,
    pid: &'l str,
    suspended: bool,
    /// `None` for the pool user's; the code for one whose user couldn't be read.
    unreadable: Option<&'l str>,
}

fn runners(lines: &[String]) -> Vec<Runner<'_>> {
    values(lines, "runner")
        .into_iter()
        .filter_map(|rest| {
            let words: Vec<&str> = rest.split_whitespace().collect();
            let (ms, pid, state, user) = match words.as_slice() {
                [ms, pid, state, "pool"] => (*ms, *pid, *state, None),
                [ms, pid, state, "unreadable", code] => (*ms, *pid, *state, Some(*code)),
                _ => return None,
            };
            Some(Runner {
                ms,
                pid,
                suspended: state == "suspended",
                unreadable: user,
            })
        })
        .collect()
}

/// `cancel`: `started`, the agent's pid once the session started (or why it didn't); `lines`,
/// the looks after the host-style kill; `stderr`, the outer runner's. FAIL naming each suspended
/// runner process of the pool user, and each suspended one whose user couldn't be read; INFO
/// when the session never started or no look could read the threads' states (naming the
/// `NTSTATUS`); PASS otherwise, naming the running runner processes of the pool user seen (the
/// end sweeper) or none, and whether the outer said it skipped the sweeper.
pub(crate) fn cancel(started: Result<u32, &str>, lines: &[String], stderr: &[String]) -> Verdict {
    let agent = match started {
        Ok(pid) => pid,
        Err(why) => {
            return Verdict::Info(format!(
                "the session never started ({why}), so nothing was cancelled"
            ));
        }
    };
    let looks = values(lines, "look").len();
    let errors: Vec<String> = values(lines, "suspend-error")
        .into_iter()
        .map(|rest| match rest.split_once(' ') {
            Some((ms, status)) => format!("{status} at {ms} ms"),
            None => rest.to_owned(),
        })
        .collect();
    if looks == 0 {
        return Verdict::Info(format!(
            "no look read the threads' states (NtQuerySystemInformation gave {}), so it proves \
             nothing",
            if errors.is_empty() {
                "nothing".to_owned()
            } else {
                errors.join(", ")
            }
        ));
    }
    let all = runners(lines);
    let mut bad = BTreeSet::new();
    for runner in all.iter().filter(|r| r.suspended) {
        bad.insert(match runner.unreadable {
            None => format!(
                "the pool user's clauseo-sandbox-run {} (seen {} ms after the kill)",
                runner.pid, runner.ms
            ),
            Some(code) => format!(
                "clauseo-sandbox-run {} whose user couldn't be read ({code}; seen {} ms after \
                 the kill)",
                runner.pid, runner.ms
            ),
        });
    }
    let skipped = stderr.iter().find(|line| line.contains(NO_SWEEPER));
    let said = skipped.map_or_else(
        || "the outer didn't log skipping the end sweeper".to_owned(),
        |line| format!("the outer logged {line:?}"),
    );
    if !bad.is_empty() {
        let named: Vec<String> = bad.into_iter().collect();
        return Verdict::Fail(format!(
            "a suspended runner process was left after the host-style kill: {}; {said}",
            named.join(", ")
        ));
    }
    let running: BTreeSet<&str> = all
        .iter()
        .filter(|r| r.unreadable.is_none())
        .map(|r| r.pid)
        .collect();
    let seen = if running.is_empty() {
        "no runner process of the pool user".to_owned()
    } else {
        let pids: Vec<&str> = running.into_iter().collect();
        format!(
            "the pool user's running runner processes {} (an end sweeper, or a runner still \
             ending)",
            pids.join(", ")
        )
    };
    let failed = if errors.is_empty() {
        String::new()
    } else {
        format!("; {} looks failed ({})", errors.len(), errors.join(", "))
    };
    Verdict::Pass(format!(
        "after stdin closed, 2 s, then a kill of the outer (the agent was {agent}): no suspended \
         runner process in {looks} looks{failed}; {seen}; {said}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{Verdict, cancel};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    fn text_of(verdict: &Verdict) -> &str {
        match verdict {
            Verdict::Pass(t) | Verdict::Fail(t) | Verdict::Info(t) => t,
        }
    }

    fn names(verdict: &Verdict, words: &[&str]) {
        let text = text_of(verdict);
        for word in words {
            assert!(text.contains(word), "{text:?} doesn't name {word:?}");
        }
    }

    const SKIPPED: &str = "clauseo-sandbox-run: warning: no end sweeper: 300 ms left of the \
                           host's 2 s; the next launch's first act sweeps";

    #[test]
    fn cancel_fails_naming_each_suspended_runner_of_the_pool_user() {
        let seen = lines(&[
            "look 0 2",
            "runner 0 700 suspended pool",
            "runner 0 800 running pool",
            "look 400 1",
            "runner 400 700 suspended pool",
            "runner 400 900 suspended unreadable 5",
        ]);
        let verdict = cancel(Ok(300), &seen, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "clauseo-sandbox-run 700",
                "900 whose user couldn't be read (5",
            ],
        );
        assert!(!text_of(&verdict).contains(" 800 "), "{verdict:?}");
    }

    #[test]
    fn cancel_passes_naming_a_running_sweeper_or_none() {
        let sweeper = lines(&["look 0 1", "runner 0 800 running pool", "look 400 0"]);
        let verdict = cancel(Ok(300), &sweeper, &[]);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(&verdict, &["running runner processes 800", "2 looks"]);
        let none = lines(&["look 0 0", "suspend-error 400 0xc0000017", "look 800 0"]);
        let verdict = cancel(Ok(300), &none, &lines(&["other", SKIPPED]));
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "no runner process",
                "1 looks failed (0xc0000017 at 400 ms)",
                "no end sweeper: 300 ms",
            ],
        );
    }

    #[test]
    fn cancel_is_info_when_nothing_could_be_read_or_nothing_started() {
        let failed = lines(&["suspend-error 0 0xc0000022", "suspend-error 400 0xc0000022"]);
        let verdict = cancel(Ok(300), &failed, &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        names(&verdict, &["0xc0000022 at 0 ms, 0xc0000022 at 400 ms"]);
        let verdict = cancel(Err("A's probe printed no pid"), &[], &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        names(&verdict, &["never started", "printed no pid"]);
    }
}
