//! The verdicts of the `cancel` and `stdin-unread-cancel` rows (`checks_cancel.rs`), from the
//! lines the harness prints: no Win32 call here, so each rule has a unit test.
//!
//! The harness's lines, one set per look after the outer runner ended (`<ms>` after the kill,
//! or after its own exit):
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
/// What the outer logs once the end sweeper is resumed, before `<n> ms`: its own logon, shut and
/// resume's time.
const LOGON_TIME: &str = "clauseo-sandbox-run: end sweeper: logon to resume ";

/// The sweeper's logon-to-resume time in the outer's `stderr`, as information alone.
fn logon_time(stderr: &[String]) -> String {
    stderr
        .iter()
        .find_map(|line| line.split_once(LOGON_TIME).map(|(_, rest)| rest.trim()))
        .map_or_else(
            || "the outer logged no sweeper logon time".to_owned(),
            |time| format!("the end sweeper's logon to resume took {time}"),
        )
}

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
    let said = format!("{said}; {}", logon_time(stderr));
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

/// The `stdin-unread-cancel` row's bound: from the harness's close of the outer's stdin to the
/// outer's own exit, in milliseconds. The ladder is the agent's 1 s, the job's end, up to 400 ms
/// for it to empty and the end sweeper's start, all inside the host's 2 s; 1 s more is slack.
pub(crate) const LADDER_MS: u128 = 3_000;

/// `stdin-unread-cancel`: `started`, the agent's pid once the session started (or why it
/// didn't); `exited`, the milliseconds from the stdin's close to the outer's own exit, `None`
/// when it still ran at [`LADDER_MS`] and the harness killed it; `lines`, the looks after it
/// ended; `stderr`, the outer's. FAIL when the outer didn't exit by itself within the bound,
/// naming each runner process of the pool user (or of a user that couldn't be read) suspended in
/// any look or still running at the last; INFO when the session never started, or the outer
/// exited in time but no look could read the threads' states; PASS otherwise.
pub(crate) fn stdin_unread(
    started: Result<u32, &str>,
    exited: Option<u128>,
    lines: &[String],
    stderr: &[String],
) -> Verdict {
    let agent = match started {
        Ok(pid) => pid,
        Err(why) => {
            return Verdict::Info(format!(
                "the session never started ({why}), so nothing was cancelled"
            ));
        }
    };
    let looked: Vec<u128> = values(lines, "look")
        .into_iter()
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .collect();
    let last = looked.iter().max().map(u128::to_string);
    let mut left = BTreeSet::new();
    for runner in runners(lines) {
        let at_last = last.as_deref() == Some(runner.ms);
        if !runner.suspended && !at_last {
            continue;
        }
        let state = if runner.suspended {
            "suspended"
        } else {
            "running at the last look"
        };
        left.insert(match runner.unreadable {
            None => format!(
                "the pool user's clauseo-sandbox-run {} {state} ({} ms after the outer ended)",
                runner.pid, runner.ms
            ),
            Some(code) => format!(
                "clauseo-sandbox-run {} whose user couldn't be read ({code}), {state} ({} ms \
                 after the outer ended)",
                runner.pid, runner.ms
            ),
        });
    }
    let mut problems = Vec::new();
    match exited {
        Some(ms) if ms <= LADDER_MS => {}
        Some(ms) => problems.push(format!(
            "the outer exited {ms} ms after the stdin's close, past the ladder's {LADDER_MS} ms"
        )),
        None => problems.push(format!(
            "the outer still ran {LADDER_MS} ms after the stdin's close, and the harness killed \
             it: the agent's blocked stdin write wasn't released"
        )),
    }
    problems.extend(left);
    let time = logon_time(stderr);
    if !problems.is_empty() {
        return Verdict::Fail(format!(
            "after 4 MiB the agent never read and the stdin's close: {}; {time}",
            problems.join(", ")
        ));
    }
    if looked.is_empty() {
        return Verdict::Info(
            "the outer exited in time, but no look read the threads' states, so whether a \
             runner was left proves nothing"
                .to_owned(),
        );
    }
    let ms = exited.unwrap_or_default();
    Verdict::Pass(format!(
        "the outer exited {ms} ms after the close of a stdin holding 4 MiB the agent ({agent}) \
         never read, within {LADDER_MS} ms: the job's end released the blocked write; no runner \
         process of the pool user suspended in {} looks, none running at the last; {time}",
        looked.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::{LADDER_MS, Verdict, cancel, stdin_unread};

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
        let timed = lines(&["clauseo-sandbox-run: end sweeper: logon to resume 212 ms"]);
        let verdict = cancel(Ok(300), &sweeper, &timed);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "running runner processes 800",
                "2 looks",
                "logon to resume took 212 ms",
            ],
        );
        // Missing, it is said, and the rule is the same.
        let verdict = cancel(Ok(300), &sweeper, &[]);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(&verdict, &["no sweeper logon time"]);
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

    #[test]
    fn stdin_unread_passes_when_the_outer_exits_in_time_and_nothing_is_left() {
        let seen = lines(&["look 0 1", "runner 0 800 running pool", "look 3000 0"]);
        let verdict = stdin_unread(Ok(300), Some(1_620), &seen, &[]);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(&verdict, &["exited 1620 ms", "2 looks", "released"]);
    }

    #[test]
    fn stdin_unread_fails_naming_a_late_or_killed_outer_and_each_runner_left() {
        let clean = lines(&["look 0 0", "look 3000 0"]);
        let verdict = stdin_unread(Ok(300), None, &clean, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(&verdict, &["still ran 3000 ms", "killed it"]);
        let verdict = stdin_unread(Ok(300), Some(LADDER_MS + 1), &clean, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(&verdict, &["exited 3001 ms"]);
        let left = lines(&[
            "look 0 2",
            "runner 0 700 suspended pool",
            "runner 0 800 running pool",
            "look 3000 2",
            "runner 3000 800 running pool",
            "runner 3000 900 running unreadable 5",
        ]);
        let verdict = stdin_unread(Ok(300), Some(1_500), &left, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "clauseo-sandbox-run 700 suspended",
                "clauseo-sandbox-run 800 running at the last look",
                "900 whose user couldn't be read (5)",
            ],
        );
        assert!(!text_of(&verdict).contains("800 running at the last look (0 ms"));
    }

    #[test]
    fn stdin_unread_is_info_when_nothing_started_or_nothing_could_be_read() {
        let verdict = stdin_unread(Err("no pid"), None, &[], &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        let failed = lines(&["suspend-error 0 0xc0000022"]);
        let verdict = stdin_unread(Ok(300), Some(900), &failed, &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
    }
}
