//! The refresh task's last run, as the desktop logs it at start-up: the Task Scheduler's last
//! result for `\Clauseo\SandboxRefresh` (`task::last_refresh_run`), read back as a
//! [`RefreshExit`] through [`RefreshExit::as_code`]'s numbers, so a refresh that keeps failing
//! (a bad signature, say) shows in host.log. No Windows call here: it builds and is tested
//! everywhere.

use super::RefreshExit;

/// `SCHED_S_TASK_HAS_NOT_RUN`: the task's last result before its first run.
pub const SCHED_S_TASK_HAS_NOT_RUN: i32 = 0x0004_1303;
/// `SCHED_S_TASK_RUNNING`: the task's last result while a run is going on.
pub const SCHED_S_TASK_RUNNING: i32 = 0x0004_1301;

/// Every [`RefreshExit`], in code order, for reading a code back.
const EXITS: [RefreshExit; 14] = [
    RefreshExit::Done,
    RefreshExit::NotAllowed,
    RefreshExit::NoRecord,
    RefreshExit::SourceRefused,
    RefreshExit::SignatureRefused,
    RefreshExit::PlaceholderKey,
    RefreshExit::StagingFailed,
    RefreshExit::ReplaceFailed,
    RefreshExit::RecordNotWritten,
    RefreshExit::Downgrade,
    RefreshExit::OutsideProfile,
    RefreshExit::WrongMachine,
    RefreshExit::FolderBusy,
    RefreshExit::LockHeld,
];

/// The task's last run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LastRun {
    /// It has never run: its last result is [`SCHED_S_TASK_HAS_NOT_RUN`], or its last run time
    /// is zero.
    NeverRun,
    /// It ran.
    Ran {
        /// Its last result: the exit code, or a Task Scheduler code.
        code: i32,
        /// Its last run time, an OLE automation date in the machine's local time (days since
        /// 1899-12-30).
        at: f64,
    },
}

/// How loud a last run's line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastRunLevel {
    /// Nothing to look at.
    Info,
    /// The refresh failed, or ended with a code it doesn't have.
    Warn,
}

/// The [`LastRun`] the task's last result `code` and last run time `at` give.
pub fn last_run(code: i32, at: f64) -> LastRun {
    if code == SCHED_S_TASK_HAS_NOT_RUN || at == 0.0 {
        LastRun::NeverRun
    } else {
        LastRun::Ran { code, at }
    }
}

/// The [`RefreshExit`] whose [`RefreshExit::as_code`] is `code`, if any.
pub fn refresh_exit(code: i32) -> Option<RefreshExit> {
    EXITS
        .into_iter()
        .find(|exit| i32::from(exit.as_code()) == code)
}

/// A label and a level for `run`: `never run`, `running`, the [`RefreshExit`]'s name (`Done`,
/// `SignatureRefused`, ...) or `unknown` for a code the refresh doesn't have. `Done`, never run
/// and running are [`LastRunLevel::Info`]; every other code is [`LastRunLevel::Warn`].
pub fn last_run_label(run: LastRun) -> (&'static str, LastRunLevel) {
    let LastRun::Ran { code, .. } = run else {
        return ("never run", LastRunLevel::Info);
    };
    if code == SCHED_S_TASK_RUNNING {
        return ("running", LastRunLevel::Info);
    }
    match refresh_exit(code) {
        Some(RefreshExit::Done) => ("Done", LastRunLevel::Info),
        Some(exit) => (exit_name(exit), LastRunLevel::Warn),
        None => ("unknown", LastRunLevel::Warn),
    }
}

/// `exit`'s name, as its variant spells it.
fn exit_name(exit: RefreshExit) -> &'static str {
    match exit {
        RefreshExit::Done => "Done",
        RefreshExit::NotAllowed => "NotAllowed",
        RefreshExit::NoRecord => "NoRecord",
        RefreshExit::SourceRefused => "SourceRefused",
        RefreshExit::SignatureRefused => "SignatureRefused",
        RefreshExit::PlaceholderKey => "PlaceholderKey",
        RefreshExit::StagingFailed => "StagingFailed",
        RefreshExit::ReplaceFailed => "ReplaceFailed",
        RefreshExit::RecordNotWritten => "RecordNotWritten",
        RefreshExit::Downgrade => "Downgrade",
        RefreshExit::OutsideProfile => "OutsideProfile",
        RefreshExit::WrongMachine => "WrongMachine",
        RefreshExit::FolderBusy => "FolderBusy",
        RefreshExit::LockHeld => "LockHeld",
    }
}

/// An OLE automation date (days since 1899-12-30, the fraction the time of day) as
/// `YYYY-MM-DD HH:MM:SS`, to the nearest second. `None` when it isn't a date from the year 1900
/// to 9999.
pub fn ole_date_text(at: f64) -> Option<String> {
    // 1900-01-01 and 10000-01-01 as OLE days.
    if !(2.0..2_958_466.0).contains(&at) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "in range: at most 2958466 days of 86400 seconds"
    )]
    let seconds = (at * 86_400.0).round() as i64;
    // Days from 1970-01-01, then Howard Hinnant's `civil_from_days`.
    let days = seconds.div_euclid(86_400) - 25_569;
    let time = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (hour, minute, second) = (time / 3600, time % 3600 / 60, time % 60);
    Some(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_that_never_ran_is_info() {
        assert_eq!(
            last_run(SCHED_S_TASK_HAS_NOT_RUN, 45_000.5),
            LastRun::NeverRun
        );
        assert_eq!(last_run(0, 0.0), LastRun::NeverRun);
        assert_eq!(
            last_run_label(LastRun::NeverRun),
            ("never run", LastRunLevel::Info)
        );
    }

    #[test]
    fn done_is_info() {
        let run = last_run(0, 45_000.5);
        assert_eq!(
            run,
            LastRun::Ran {
                code: 0,
                at: 45_000.5
            }
        );
        assert_eq!(last_run_label(run), ("Done", LastRunLevel::Info));
        let running = last_run(SCHED_S_TASK_RUNNING, 45_000.5);
        assert_eq!(last_run_label(running), ("running", LastRunLevel::Info));
    }

    #[test]
    fn a_failure_is_warn_with_its_name() {
        let run = last_run(35, 45_000.5);
        assert_eq!(
            last_run_label(run),
            ("SignatureRefused", LastRunLevel::Warn)
        );
        assert_eq!(
            last_run_label(last_run(44, 1.0e4)),
            ("LockHeld", LastRunLevel::Warn)
        );
    }

    #[test]
    fn an_unknown_code_is_warn() {
        for code in [1, 31, 45, -2_147_024_894] {
            assert_eq!(
                last_run_label(last_run(code, 45_000.5)),
                ("unknown", LastRunLevel::Warn),
                "{code}"
            );
        }
    }

    #[test]
    fn every_exit_reads_back_from_its_code() {
        for exit in EXITS {
            assert_eq!(refresh_exit(i32::from(exit.as_code())), Some(exit));
            assert_eq!(format!("{exit:?}"), exit_name(exit));
        }
    }

    #[test]
    fn ole_dates_read_as_local_text() {
        assert_eq!(ole_date_text(2.0).as_deref(), Some("1900-01-01 00:00:00"));
        assert_eq!(
            ole_date_text(25_569.0).as_deref(),
            Some("1970-01-01 00:00:00")
        );
        // 2026-11-08 12:30:00.
        assert_eq!(
            ole_date_text(46_334.520_833_333_336).as_deref(),
            Some("2026-11-08 12:30:00")
        );
        assert_eq!(ole_date_text(0.0), None);
        assert_eq!(ole_date_text(f64::NAN), None);
    }
}
