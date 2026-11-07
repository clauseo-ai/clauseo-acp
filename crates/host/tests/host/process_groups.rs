//! The crash-survivor sweep (`process_groups.rs`, decision record 0051): at start, before it
//! serves, the host ends each agent process group an earlier run recorded that is provably still
//! that group, and deletes the records; a launch records its agent's group and the session's
//! removal deletes the record once the tree is gone.
//!
//! Linux only (the CI's Unix runner; macOS's listing is the same code over libproc). Every
//! process signalled here is one this test started, and each test ends its own in a guard.
#![cfg(target_os = "linux")]
#![expect(
    clippy::disallowed_methods,
    reason = "the test starts its own process groups with std, plants records and links in the \
              data folder, and waits for a shell to start its sleepers"
)]

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{EgressProxy, SessionKey};
use clauseo_process::{Member, boot_stamp, group_members, kill_member, start_stamp};
use serde_json::json;

use crate::egress::claude_then;
use crate::profile_rig::{CLAUDE, Hook, Wired, browser_server, turn};
use crate::support::Dir;

/// The records' folder of `dir`'s data folder.
fn records(dir: &Dir) -> PathBuf {
    dir.data().join("process-groups")
}

/// The names in `dir`'s records folder, sorted.
fn names(dir: &Dir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(records(dir))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A record's text.
fn record(pgid: u32, leader_start: &str, boot: &str) -> String {
    json!({"v": 1, "pgid": pgid, "leader_start": leader_start, "boot": boot}).to_string()
}

/// Writes `text` at `<records>/<name>`, 0600.
fn plant(dir: &Dir, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(records(dir)).unwrap();
    let path = records(dir).join(name);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    path
}

/// This boot's stamp.
fn boot() -> String {
    boot_stamp().expect("the boot stamp")
}

/// The test's own processes, each with its start: killed (by its start) when the guard drops,
/// so a failed assert leaves nothing behind.
#[derive(Default)]
struct Mine {
    children: Vec<Child>,
    members: Vec<Member>,
}

impl Mine {
    /// `sleep 600` leading a new group of its own.
    fn sleeper(&mut self) -> (u32, clauseo_process::StartStamp) {
        let child = Command::new("sleep")
            .arg("600")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let start = start_stamp(pid).unwrap();
        self.children.push(child);
        (pid, start)
    }

    /// An orphaned agent tree's stand-in: `sh -c 'sleep 600 & sleep 600'` leading a new group,
    /// its leader then killed and reaped, so the group is left with its two sleepers and no
    /// leader. Answers the group's id and the leader's start.
    fn leaderless_group(&mut self) -> (u32, clauseo_process::StartStamp) {
        let mut shell = Command::new("sh")
            .args(["-c", "sleep 600 & sleep 600"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = shell.id();
        let start = start_stamp(pgid).unwrap();
        let mut members = Vec::new();
        for _ in 0..200 {
            members = group_members(pgid).unwrap();
            if members.len() == 3 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(members.len(), 3, "the shell and its two sleepers");
        let sleepers: Vec<Member> = members.into_iter().filter(|m| m.pid != pgid).collect();
        self.members.extend(sleepers.iter().copied());
        shell.kill().unwrap();
        shell.wait().unwrap();
        assert_eq!(start_stamp(pgid), None, "the leader is gone");
        assert_eq!(
            group_members(pgid).unwrap().len(),
            2,
            "its sleepers live on"
        );
        (pgid, start)
    }

    /// Whether the child `pid` the test started still runs (it is reaped if it has exited).
    fn running(&mut self, pid: u32) -> bool {
        let child = self
            .children
            .iter_mut()
            .find(|child| child.id() == pid)
            .unwrap();
        child.try_wait().unwrap().is_none()
    }
}

impl Drop for Mine {
    fn drop(&mut self) {
        for member in &self.members {
            kill_member(member);
        }
        for child in &mut self.children {
            drop(child.kill());
            drop(child.wait());
        }
    }
}

/// (1) A crash survivor: a leader-less group whose record names it. `Host::start` has ended
/// the group before it returns, and deleted the record.
#[test]
fn a_crash_survivors_group_is_ended_before_the_start_returns() {
    let dir = Dir::new("pg_survivor", &[]);
    let mut mine = Mine::default();
    let (pgid, start) = mine.leaderless_group();
    plant(&dir, "m-t-1.json", &record(pgid, &start.encode(), &boot()));
    let host = dir.start(0);
    assert_eq!(group_members(pgid).unwrap(), [], "the group was ended");
    assert_eq!(names(&dir), Vec::<String>::new(), "the record was deleted");
    host.shutdown();
}

/// (2) A live leader with the recorded start: its group is ended.
#[test]
fn a_live_leader_with_the_recorded_start_is_ended() {
    let dir = Dir::new("pg_leader", &[]);
    let mut mine = Mine::default();
    let (pid, start) = mine.sleeper();
    plant(&dir, "m-t-2.json", &record(pid, &start.encode(), &boot()));
    let host = dir.start(0);
    assert!(!mine.running(pid), "the leader was killed");
    assert_eq!(names(&dir), Vec::<String>::new());
    host.shutdown();
}

/// (3) A reused id: the record's group id is a live process's pid with another start. It lives
/// on, and the record is deleted.
#[test]
fn a_reused_id_is_left_alone() {
    let dir = Dir::new("pg_reused", &[]);
    let mut mine = Mine::default();
    let (pid, start) = mine.sleeper();
    let other = clauseo_process::StartStamp::decode(&(start.encode() + "0")).unwrap();
    assert_ne!(other, start);
    plant(&dir, "m-t-3.json", &record(pid, &other.encode(), &boot()));
    let host = dir.start(0);
    assert!(
        mine.running(pid),
        "a process with another start is not the agent"
    );
    assert_eq!(names(&dir), Vec::<String>::new());
    host.shutdown();
}

/// (4) Another boot's record touches nothing, even naming a live group with its own start.
#[test]
fn another_boots_record_touches_nothing() {
    let dir = Dir::new("pg_boot", &[]);
    let mut mine = Mine::default();
    let (pid, start) = mine.sleeper();
    plant(
        &dir,
        "m-t-4.json",
        &record(pid, &start.encode(), "another-boot"),
    );
    let host = dir.start(0);
    assert!(mine.running(pid));
    assert_eq!(names(&dir), Vec::<String>::new(), "the stale record goes");
    host.shutdown();
}

/// (5) Forged records: a link to a record, and a record with a second (hard) link, each naming
/// a live group with its own start. Both are refused unread and deleted (the links only); the
/// group lives on and the files they point at stay.
#[test]
fn a_linked_or_hard_linked_record_is_refused_and_deleted() {
    let dir = Dir::new("pg_forged", &[]);
    let mut mine = Mine::default();
    let (pid, start) = mine.sleeper();
    let text = record(pid, &start.encode(), &boot());
    let outside = dir.work().join("forged.json");
    std::fs::write(&outside, &text).unwrap();
    std::fs::create_dir_all(records(&dir)).unwrap();
    std::os::unix::fs::symlink(&outside, records(&dir).join("m-t-5.json")).unwrap();
    let second = dir.work().join("second.json");
    std::fs::write(&second, &text).unwrap();
    std::fs::hard_link(&second, records(&dir).join("m-t-6.json")).unwrap();
    let host = dir.start(0);
    assert!(mine.running(pid), "a forged record ends nothing");
    assert_eq!(names(&dir), Vec::<String>::new(), "both links are deleted");
    assert!(
        outside.is_file() && second.is_file(),
        "what they named stays"
    );
    host.shutdown();
}

/// The one record in `dir`'s folder, as JSON.
fn only_record(dir: &Dir) -> serde_json::Value {
    let names = names(dir);
    assert_eq!(names.len(), 1, "{names:?}");
    let text = std::fs::read(records(dir).join(&names[0])).unwrap();
    serde_json::from_slice(&text).unwrap()
}

/// (6) A normal end: a launched agent's record holds its group (its own pid, with its start
/// and this boot) while it runs, and is gone once the session's removal has freed the run.
#[test]
fn a_launch_records_its_group_and_the_removal_deletes_it() {
    let dir = Dir::new("pg_normal", &[]);
    claude_then(&dir, &json!([]), &[]);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    assert_eq!(names(&dir), Vec::<String>::new(), "the folder starts empty");
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let written = only_record(&dir);
    let pgid = u32::try_from(written["pgid"].as_i64().unwrap()).unwrap();
    assert_eq!(written["v"], 1);
    assert_eq!(written["boot"], boot().as_str());
    assert_eq!(
        start_stamp(pgid).map(clauseo_process::StartStamp::encode),
        written["leader_start"].as_str().map(str::to_owned),
        "the running agent's own start"
    );
    let path = records(&dir).join(&names(&dir)[0]);
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions());
    assert_eq!(mode & 0o777, 0o600);
    host.shutdown();
    assert_eq!(names(&dir), Vec::<String>::new(), "the removal deleted it");
}

/// A proxy that, as the first launch claims its ports, records whether the survivor's group is
/// already gone.
struct Witness {
    pgid: u32,
    log: Arc<Mutex<Vec<String>>>,
}

impl EgressProxy for Witness {
    fn open(
        &self,
        _session: SessionKey,
        _config: clauseo_sandbox::ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error> {
        let gone = group_members(self.pgid).is_some_and(|members| members.is_empty());
        let line = if gone {
            "open, survivor gone"
        } else {
            "open, survivor alive"
        };
        self.log.lock().unwrap().push(line.to_owned());
        Ok(())
    }

    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(8123)
    }

    fn close(&self, _session: SessionKey) {}
}

/// (7) The order: the sweep has ended the survivor's group before the first launch claims its
/// proxy port.
#[test]
fn the_sweep_runs_before_the_first_launch_claims_a_port() {
    let dir = Dir::new("pg_order", &[]);
    let mut mine = Mine::default();
    let (pgid, start) = mine.leaderless_group();
    plant(&dir, "m-t-7.json", &record(pgid, &start.encode(), &boot()));
    claude_then(&dir, &json!([{"name": "browser"}]), &[]);
    let log = Arc::new(Mutex::new(Vec::new()));
    let proxy = Arc::new(Witness {
        pgid,
        log: Arc::clone(&log),
    });
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        config
            .with_profile_hook(Box::new(Hook {
                servers: vec![browser_server()],
                ..Hook::default()
            }))
            .with_egress_proxy(proxy)
            .with_browser_executable(Some(PathBuf::from("/chrome")))
    });
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(*log.lock().unwrap(), ["open, survivor gone"]);
    host.shutdown();
}
