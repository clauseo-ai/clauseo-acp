//! The verdicts of the `cancel` and `stdin-unread-cancel` rows (`checks_cancel.rs`), from the
//! lines the harness prints: no Win32 call here, so each rule has a unit test.
//!
//! The harness's lines, one set per look (`<ms>` after the kill, after the outer's own exit, or,
//! for `stdin-unread-cancel`'s look at the ladder's end, after the stdin's close):
//!
//! - `look <ms> <n>`: the look read every process's threads (`win_suspend::thread_states`) and
//!   saw `n` runner processes (`clauseo-sandbox-run.exe`) of any user; or `suspend-error <ms>
//!   <ntstatus>` when `NtQuerySystemInformation` failed.
//! - `runner <ms> <pid> <suspended|running> pool` per runner process of the pool user, and
//!   `runner <ms> <pid> <suspended|running> unreadable <code>` per one whose user the harness
//!   couldn't read. `suspended`: every thread waits, suspended. Another user's isn't listed.
//!
//! No runner process of the pool user runs after a session's end: the outer starts none there
//! (it asks the Task Scheduler for the `SYSTEM` task `\Clauseo\SandboxSweep` instead), and the
//! inner is in the job the outer ends. One suspended in any look, or running at the last, is a
//! failure; one running in an earlier look is still ending and is named as information.

use std::collections::BTreeSet;

use super::checks::values;
use super::pool_verdict::Verdict;

/// What every line the outer logs about the sandbox sweep task holds.
pub(crate) const SWEEP_TASK: &str = "the sandbox sweep task";

/// The outer's lines about the sandbox sweep task in `stderr`, as information alone. A clean
/// result (0) logs nothing.
pub(crate) fn sweep_task(stderr: &[String]) -> String {
    let said: Vec<&str> = stderr
        .iter()
        .filter(|line| line.contains(SWEEP_TASK))
        .map(|line| line.trim())
        .collect();
    if said.is_empty() {
        "the outer logged no sandbox sweep task warning".to_owned()
    } else {
        format!("the outer logged {}", said.join(" / "))
    }
}

/// One `runner` line.
struct Runner<'l> {
    ms: &'l str,
    pid: &'l str,
    suspended: bool,
    /// `None` for the pool user's; the code for one whose user couldn't be read.
    unreadable: Option<&'l str>,
}

/// The `runner` lines in `lines`.
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

/// The milliseconds of each `look` line in `lines`.
fn looked(lines: &[String]) -> Vec<u128> {
    values(lines, "look")
        .into_iter()
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .collect()
}

/// The `suspend-error` lines in `lines`, as `<ntstatus> at <ms> ms`.
fn errors(lines: &[String]) -> Vec<String> {
    values(lines, "suspend-error")
        .into_iter()
        .map(|rest| match rest.split_once(' ') {
            Some((ms, status)) => format!("{status} at {ms} ms"),
            None => rest.to_owned(),
        })
        .collect()
}

/// Each runner process in `lines` suspended in any look or running in the last one, named;
/// `after` says what the milliseconds count from.
fn left(lines: &[String], after: &str) -> BTreeSet<String> {
    let last = looked(lines).into_iter().max().map(|ms| ms.to_string());
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
                "the pool user's clauseo-sandbox-run {} {state} ({} ms {after})",
                runner.pid, runner.ms
            ),
            Some(code) => format!(
                "clauseo-sandbox-run {} whose user couldn't be read ({code}), {state} ({} ms \
                 {after})",
                runner.pid, runner.ms
            ),
        });
    }
    left
}

/// The pool user's runner processes running in a look before the last: still ending.
fn ending(lines: &[String]) -> String {
    let last = looked(lines).into_iter().max().map(|ms| ms.to_string());
    let pids: BTreeSet<&str> = runners(lines)
        .into_iter()
        .filter(|r| r.unreadable.is_none() && !r.suspended && last.as_deref() != Some(r.ms))
        .map(|r| r.pid)
        .collect();
    if pids.is_empty() {
        "no runner process of the pool user in an earlier look".to_owned()
    } else {
        let pids: Vec<&str> = pids.into_iter().collect();
        format!(
            "the pool user's runner processes {} still ending in an earlier look",
            pids.join(", ")
        )
    }
}

/// `cancel`: `started`, the agent's pid once the session started (or why it didn't); `lines`,
/// the looks after the host-style kill; `stderr`, the outer runner's. FAIL naming each runner
/// process of the pool user, or of a user that couldn't be read, suspended in any look or
/// running at the last; INFO when the session never started or no look could read the threads'
/// states (naming the `NTSTATUS`); PASS otherwise. Each verdict names the outer's lines about
/// the sandbox sweep task, as information.
pub(crate) fn cancel(started: Result<u32, &str>, lines: &[String], stderr: &[String]) -> Verdict {
    let agent = match started {
        Ok(pid) => pid,
        Err(why) => {
            return Verdict::Info(format!(
                "the session never started ({why}), so nothing was cancelled"
            ));
        }
    };
    let looks = looked(lines).len();
    let errors = errors(lines);
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
    let said = sweep_task(stderr);
    let bad = left(lines, "after the kill");
    if !bad.is_empty() {
        let named: Vec<String> = bad.into_iter().collect();
        return Verdict::Fail(format!(
            "a runner process was left after the host-style kill: {}; {said}",
            named.join(", ")
        ));
    }
    let failed = if errors.is_empty() {
        String::new()
    } else {
        format!("; {} looks failed ({})", errors.len(), errors.join(", "))
    };
    Verdict::Pass(format!(
        "after stdin closed, 2 s, then a kill of the outer (the agent was {agent}): no runner \
         process of the pool user suspended in {looks} looks, none running at the last{failed}; \
         {}; {said}",
        ending(lines)
    ))
}

/// The ladder, in milliseconds from the harness's close of the outer's stdin: the agent's 1 s,
/// the job's end, up to 400 ms for it to empty and the sweep task's request, all inside the
/// host's 2 s; 1 s more is slack. By then no runner process of the pool user is left.
pub(crate) const LADDER_MS: u128 = 3_000;

/// How long the outer polls the sandbox sweep task's result at its own end, in milliseconds.
const SWEEP_POLL_MS: u128 = 20_000;

/// The `stdin-unread-cancel` row's bound on the outer's own exit, in milliseconds from the
/// stdin's close: the ladder, then the sweep task's poll, then 2 s of slack.
pub(crate) const EXIT_MS: u128 = LADDER_MS + SWEEP_POLL_MS + 2_000;

/// `stdin-unread-cancel`: `started`, the agent's pid once the session started (or why it
/// didn't); `exited`, the milliseconds from the stdin's close to the outer's own exit, `None`
/// when it still ran at [`EXIT_MS`] and the harness killed it; `ladder`, the look at the
/// ladder's end (or the outer's earlier exit); `lines`, the looks after the outer ended;
/// `stderr`, the outer's. FAIL when the outer didn't exit by itself within [`EXIT_MS`], naming
/// each runner process of the pool user (or of a user that couldn't be read) seen in the ladder's
/// look, suspended in any later look or still running at the last; INFO when the session never
/// started, or nothing failed but no look could read the threads' states; PASS otherwise.
pub(crate) fn stdin_unread(
    started: Result<u32, &str>,
    exited: Option<u128>,
    ladder: &[String],
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
    let mut problems = Vec::new();
    match exited {
        Some(ms) if ms <= EXIT_MS => {}
        Some(ms) => problems.push(format!(
            "the outer exited {ms} ms after the stdin's close, past the bound's {EXIT_MS} ms"
        )),
        None => problems.push(format!(
            "the outer still ran {EXIT_MS} ms after the stdin's close, and the harness killed \
             it: neither the ladder nor the sweep task's poll accounts for it"
        )),
    }
    let early: Vec<String> = left(ladder, "after the stdin's close")
        .into_iter()
        .map(|named| format!("the job hadn't ended: {named}"))
        .collect();
    problems.extend(early);
    problems.extend(left(lines, "after the outer ended"));
    let said = sweep_task(stderr);
    if !problems.is_empty() {
        return Verdict::Fail(format!(
            "after 4 MiB the agent never read and the stdin's close: {}; {said}",
            problems.join(", ")
        ));
    }
    let at = looked(ladder).into_iter().max();
    let after = looked(lines);
    let (Some(at), false) = (at, after.is_empty()) else {
        return Verdict::Info(format!(
            "the outer exited in time, but a look didn't read the threads' states ({}), so \
             whether the job ended in time, or a runner was left, proves nothing; {said}",
            [errors(ladder), errors(lines)].concat().join(", ")
        ));
    };
    let ms = exited.unwrap_or_default();
    Verdict::Pass(format!(
        "no runner process of the pool user {at} ms after the close of a stdin holding 4 MiB the \
         agent ({agent}) never read: the job's end released the blocked write; the outer exited \
         {ms} ms after the close, within {EXIT_MS} ms (the ladder, then its poll of the sweep \
         task); no runner process of the pool user suspended in {} looks after, none running at \
         the last; {said}",
        after.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::{EXIT_MS, Verdict, cancel, stdin_unread};

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

    const UNREQUESTED: &str = "clauseo-sandbox-run: warning: the sandbox sweep task couldn't be \
                               requested (0x80070002)";

    #[test]
    fn cancel_fails_naming_each_suspended_or_last_running_runner() {
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
                "clauseo-sandbox-run 700 suspended",
                "900 whose user couldn't be read (5",
            ],
        );
        assert!(!text_of(&verdict).contains(" 800 "), "{verdict:?}");
        // A runner of the pool user still running at the last look is left: a failure.
        let running = lines(&["look 0 1", "runner 0 800 running pool", "look 1500 1"]);
        let running = [running, lines(&["runner 1500 800 running pool"])].concat();
        let verdict = cancel(Ok(300), &running, &lines(&[UNREQUESTED]));
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "800 running at the last look (1500 ms",
                "couldn't be requested",
            ],
        );
    }

    #[test]
    fn cancel_passes_naming_a_runner_still_ending_and_the_sweep_task_lines() {
        let ending = lines(&["look 0 1", "runner 0 800 running pool", "look 400 0"]);
        let verdict = cancel(Ok(300), &ending, &lines(&["other", UNREQUESTED]));
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "runner processes 800 still ending",
                "2 looks",
                "the sandbox sweep task couldn't be requested (0x80070002)",
            ],
        );
        let none = lines(&["look 0 0", "suspend-error 400 0xc0000017", "look 800 0"]);
        let verdict = cancel(Ok(300), &none, &[]);
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "no runner process of the pool user in an earlier look",
                "1 looks failed (0xc0000017 at 400 ms)",
                "no sandbox sweep task warning",
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

    /// The ladder's look with nothing left, at its end.
    const CLEAR: [&str; 1] = ["look 3000 0"];

    #[test]
    fn stdin_unread_passes_when_the_job_ended_in_time_and_the_outer_exits_after_its_poll() {
        let seen = lines(&["look 0 1", "runner 0 800 running pool", "look 3000 0"]);
        let ended = "clauseo-sandbox-run: warning: the sandbox sweep task ended 0x00000001";
        let verdict = stdin_unread(
            Ok(300),
            Some(9_620),
            &lines(&CLEAR),
            &seen,
            &lines(&[ended]),
        );
        assert!(matches!(verdict, Verdict::Pass(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "3000 ms after the close",
                "exited 9620 ms",
                "2 looks after",
                "ended 0x00000001",
            ],
        );
    }

    #[test]
    fn stdin_unread_fails_naming_a_late_or_killed_outer_and_each_runner_left() {
        let clean = lines(&["look 0 0", "look 3000 0"]);
        let ladder = lines(&CLEAR);
        let verdict = stdin_unread(Ok(300), None, &ladder, &clean, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(&verdict, &["still ran 25000 ms", "killed it"]);
        let verdict = stdin_unread(Ok(300), Some(EXIT_MS + 1), &ladder, &clean, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(&verdict, &["exited 25001 ms"]);
        let late = lines(&["look 3000 1", "runner 3000 700 running pool"]);
        let verdict = stdin_unread(Ok(300), Some(5_000), &late, &clean, &[]);
        assert!(matches!(verdict, Verdict::Fail(_)), "{verdict:?}");
        names(
            &verdict,
            &[
                "the job hadn't ended: the pool user's clauseo-sandbox-run 700 running at the \
               last look (3000 ms after the stdin's close)",
            ],
        );
        let left = lines(&[
            "look 0 2",
            "runner 0 700 suspended pool",
            "runner 0 800 running pool",
            "look 3000 2",
            "runner 3000 800 running pool",
            "runner 3000 900 running unreadable 5",
        ]);
        let verdict = stdin_unread(Ok(300), Some(1_500), &ladder, &left, &[]);
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
        let verdict = stdin_unread(Err("no pid"), None, &[], &[], &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        let failed = lines(&["suspend-error 0 0xc0000022"]);
        let verdict = stdin_unread(Ok(300), Some(900), &lines(&CLEAR), &failed, &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        let unread = lines(&["suspend-error 3000 0xc0000022"]);
        let after = lines(&["look 0 0"]);
        let verdict = stdin_unread(Ok(300), Some(900), &unread, &after, &[]);
        assert!(matches!(verdict, Verdict::Info(_)), "{verdict:?}");
        names(&verdict, &["0xc0000022 at 3000 ms"]);
    }
}
