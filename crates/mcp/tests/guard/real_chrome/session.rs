//! The real-Chrome tests' view of the session's processes, on `/proc`, and of the guard's stderr
//! when a step fails.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::support::{Guard, Scratch, left_in};

/// The session's processes: Chrome's process group (its leader found by its profile in
/// `/proc/*/cmdline`), agent-browser's daemon (its pid file, if it started), and any process
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
            .find(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmd| {
                    cmd.split(|b| *b == 0)
                        .any(|arg| arg.starts_with(flag.as_bytes()))
                })
            })
            .expect("Chrome with the session's profile");
        let daemon = crate::support::daemon_pid_file(&scratch.socket_dir())
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| text.trim().parse().ok());
        Self {
            chrome_group: group_of(chrome).unwrap(),
            daemon,
            scratch: scratch.path().to_owned(),
        }
    }

    /// What of the session still runs (zombies not counted).
    pub(crate) fn left(&self) -> Vec<String> {
        processes()
            .into_iter()
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let (_, rest) = stat.rsplit_once(") ")?;
                let fields: Vec<&str> = rest.split(' ').collect();
                let zombie = fields.first() == Some(&"Z");
                let in_group =
                    fields.get(2).and_then(|g| g.parse().ok()) == Some(self.chrome_group);
                (!zombie && (in_group || Some(pid) == self.daemon)).then(|| stat.clone())
            })
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

pub(crate) fn processes() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect()
}

pub(crate) fn group_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(2)?.parse().ok()
}

/// Every process in group `group`, zombies too, as `/proc/<pid>/stat` has it.
pub(crate) fn group_members(group: i32) -> Vec<String> {
    processes()
        .into_iter()
        .filter_map(|pid| std::fs::read_to_string(format!("/proc/{pid}/stat")).ok())
        .filter(|stat| {
            stat.rsplit_once(") ")
                .and_then(|(_, rest)| rest.split(' ').nth(2)?.parse().ok())
                == Some(group)
        })
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
