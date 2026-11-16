//! The real-Chrome tests' view of the session's processes (`support::process_table`: `/proc` on
//! Linux, `ps` on macOS), and of the guard's stderr when a step fails.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::support::{Guard, Process, Scratch, left_in, process_table};

/// The session's processes: Chrome's process group (its leader found by its profile on its
/// command line), agent-browser's daemon (its pid file, if it started), and any process
/// whose command line names the scratch folder (Chrome's crashpad handlers lead groups of their
/// own).
pub(crate) struct Session {
    pub(crate) chrome_group: i32,
    daemon: Option<i32>,
    scratch: PathBuf,
}

impl Session {
    pub(crate) fn find(scratch: &Scratch) -> Self {
        let flag = format!(
            "--user-data-dir={}",
            scratch.cwd().join("chrome-profile-").display()
        );
        let chrome = processes()
            .into_iter()
            .find(|process| process.has_arg_starting(&flag))
            .expect("Chrome with the session's profile");
        let daemon = crate::support::daemon_pid_file(&scratch.socket_dir())
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| text.trim().parse().ok());
        Self {
            chrome_group: chrome.group,
            daemon,
            scratch: scratch.path().to_owned(),
        }
    }

    /// What of the session still runs (zombies not counted).
    pub(crate) fn left(&self) -> Vec<String> {
        processes()
            .into_iter()
            .filter(|process| {
                let in_group = process.group == self.chrome_group;
                !process.zombie && (in_group || Some(process.pid) == self.daemon)
            })
            .map(|process| describe(&process))
            .chain(left_in(&self.scratch))
            .collect()
    }

    pub(crate) fn assert_gone(&self, scratch: &Scratch) {
        let gone = (0..200).any(|_| {
            let none = self.left().is_empty();
            if !none {
                std::thread::sleep(Duration::from_millis(50));
            }
            none
        });
        assert!(gone, "left behind: {:?}", self.left());
        assert_eq!(
            crate::support::run_files(&scratch.cwd()),
            Vec::<PathBuf>::new()
        );
    }
}

/// Every process, zombies too.
pub(crate) fn processes() -> Vec<Process> {
    process_table()
}

/// A process for a failure's message: `<pid> group <group>[ zombie] <command line>`.
pub(crate) fn describe(process: &Process) -> String {
    let zombie = if process.zombie { " zombie" } else { "" };
    format!(
        "{} group {}{zombie} {}",
        process.pid, process.group, process.line
    )
}

/// The process group of `pid`, a zombie's too; none once it is gone.
pub(crate) fn group_of(pid: i32) -> Option<i32> {
    processes()
        .into_iter()
        .find(|process| process.pid == pid)
        .map(|process| process.group)
}

/// Every process in group `group`, zombies too.
pub(crate) fn group_members(group: i32) -> Vec<Process> {
    processes()
        .into_iter()
        .filter(|process| process.group == group)
        .collect()
}

/// What the guard has written to stderr and not yet been read, and what it writes within `time`,
/// for a failure's message. Read on a thread of its own, and only for `time`: a process the guard
/// started may hold stderr open past the guard's exit.
pub(crate) fn stderr_within(guard: &mut Guard, time: Duration) -> String {
    use std::io::BufRead;
    let (mut text, handle) = guard.stderr_handle();
    let Some(file) = handle else { return text };
    let (lines, inbox) = std::sync::mpsc::channel();
    drop(std::thread::spawn(move || {
        for line in std::io::BufReader::new(file).lines() {
            let Ok(line) = line else { return };
            if lines.send(line).is_err() {
                return;
            }
        }
    }));
    let deadline = Instant::now() + time;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(line) = inbox.recv_timeout(left) else {
            break;
        };
        text.push_str(&line);
        text.push('\n');
    }
    text
}
