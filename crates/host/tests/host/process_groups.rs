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
use clauseo_process::{Member, boot_stamp, group_members, kill_member, session_id, start_stamp};
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

/// A record's text, with this test process's session as the agent's.
fn record(pgid: u32, leader_start: &str, boot: &str) -> String {
    record_in(pgid, leader_start, sid(), boot)
}

/// A record's text naming session `sid`.
fn record_in(pgid: u32, leader_start: &str, sid: u32, boot: &str) -> String {
    json!({"v": 2, "pgid": pgid, "leader_start": leader_start, "sid": sid, "boot": boot})
        .to_string()
}

/// This test process's session: the one a host's agents (`process_group(0)`) stay in.
fn sid() -> u32 {
    session_id(std::process::id()).expect("this process's session")
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
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 600 & sleep 600"])
            .process_group(0);
        self.orphaned(command)
    }

    /// The same group made the way a daemon makes one: `setsid sh -c …` (`setsid` is no
    /// group leader here, so it makes the session in place and runs the shell as its leader), the
    /// shell then killed. Its sleepers are left in a session and group of their own.
    fn daemon_group(&mut self) -> (u32, clauseo_process::StartStamp) {
        let mut command = Command::new("setsid");
        command.args(["sh", "-c", "sleep 600 & sleep 600"]);
        self.orphaned(command)
    }

    /// Starts `command` (a shell leading a group, starting two sleepers in it), then kills and
    /// reaps the shell. Answers the group's id and the shell's start.
    fn orphaned(&mut self, mut command: Command) -> (u32, clauseo_process::StartStamp) {
        let mut shell = command.spawn().unwrap();
        let pgid = shell.id();
        let mut start = None;
        let mut members = Vec::new();
        for _ in 0..200 {
            start = start.or_else(|| start_stamp(pgid));
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
        (pgid, start.unwrap())
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
    assert_eq!(written["v"], 2);
    assert_eq!(
        written["sid"],
        sid(),
        "the agent stays in the host's session"
    );
    let suffix = format!("-{}.json", std::process::id());
    assert!(names(&dir)[0].ends_with(&suffix), "{:?}", names(&dir));
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

/// A group that took a recorded id later, made the way a daemon makes one (`setsid`, its leader
/// then exited), its members started after the recorded leader: in another session, so it is
/// not the agent's, and it lives on. The record is deleted.
#[test]
fn a_setsid_group_that_took_the_id_later_survives() {
    let dir = Dir::new("pg_setsid", &[]);
    let mut mine = Mine::default();
    let earlier = start_stamp(std::process::id()).unwrap();
    let (pgid, _) = mine.daemon_group();
    let members = group_members(pgid).unwrap();
    assert!(
        members
            .iter()
            .all(|m| m.start >= earlier && m.session == pgid)
    );
    plant(
        &dir,
        "m-t-8.json",
        &record(pgid, &earlier.encode(), &boot()),
    );
    let host = dir.start(0);
    assert_eq!(group_members(pgid).unwrap(), members, "untouched");
    assert_eq!(names(&dir), Vec::<String>::new());
    host.shutdown();
}

/// A leader-less group in the agent's session with a member older than the recorded leader:
/// not the agent's tree, so the whole group is left alone.
#[test]
fn a_group_with_a_member_older_than_the_leader_is_left_whole() {
    let dir = Dir::new("pg_older", &[]);
    let mut mine = Mine::default();
    let (pgid, _) = mine.leaderless_group();
    let members = group_members(pgid).unwrap();
    let newest = members.iter().map(|m| m.start).max().unwrap();
    let later = clauseo_process::StartStamp::decode(&(newest.encode() + "0")).unwrap();
    plant(&dir, "m-t-9.json", &record(pgid, &later.encode(), &boot()));
    let host = dir.start(0);
    assert_eq!(
        group_members(pgid).unwrap(),
        members,
        "every member lives on"
    );
    assert_eq!(names(&dir), Vec::<String>::new());
    host.shutdown();
}

/// A record whose session is not the members' (the agent's recorded session differs) leaves a
/// leader-less group alone, as does one from an earlier build without a session, which is
/// deleted unread.
#[test]
fn another_session_or_an_old_record_ends_nothing() {
    let dir = Dir::new("pg_session", &[]);
    let mut mine = Mine::default();
    let (pgid, start) = mine.leaderless_group();
    let elsewhere = sid() + 1;
    plant(
        &dir,
        "m-t-10.json",
        &record_in(pgid, &start.encode(), elsewhere, &boot()),
    );
    let old = json!({"v": 1, "pgid": pgid, "leader_start": start.encode(), "boot": boot()});
    plant(&dir, "m-t-11.json", &old.to_string());
    let host = dir.start(0);
    assert_eq!(group_members(pgid).unwrap().len(), 2, "the group lives on");
    assert_eq!(names(&dir), Vec::<String>::new());
    host.shutdown();
}

/// A second start on a folder another host holds fails with `store.locked` before its sweep:
/// a live host's records are never acted on.
#[test]
fn a_second_start_on_a_held_folder_kills_nothing() {
    let dir = Dir::new("pg_locked", &[]);
    let host = dir.start(0);
    let mut mine = Mine::default();
    let (pid, start) = mine.sleeper();
    plant(&dir, "m-t-12.json", &record(pid, &start.encode(), &boot()));
    let clock = Dir::clock(1);
    let second = clauseo_host::Host::start(dir.config(1, &clock));
    let error = second.expect_err("the folder is held");
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::StoreLocked);
    assert!(mine.running(pid), "the live host's agent stand-in lives on");
    assert_eq!(names(&dir), ["m-t-12.json"], "its record stays");
    host.shutdown();
}

/// A Codex launch records its group too.
#[test]
fn a_codex_launch_records_its_group() {
    let dir = crate::codex::codex_dir("pg_codex", "codex-handshake");
    let host = dir.start(0);
    let (_, thread, _) =
        crate::support::matter_with_thread(&dir, &host, "m", &[], crate::codex::CODEX);
    let mut events = host.events(clauseo_protocol::StreamId::Thread(thread));
    crate::codex::run_turn(&host, &mut events, thread);
    let written = only_record(&dir);
    let pgid = u32::try_from(written["pgid"].as_i64().unwrap()).unwrap();
    assert_eq!(
        start_stamp(pgid).map(clauseo_process::StartStamp::encode),
        written["leader_start"].as_str().map(str::to_owned)
    );
    host.shutdown();
    assert_eq!(names(&dir), Vec::<String>::new());
}

/// A record that cannot be written (the folder replaced by a file) still launches the agent,
/// and the warning names only the error's kind, never a path.
#[test]
fn a_failed_record_write_still_launches_and_logs_only_the_kind() {
    let lines = Arc::new(Mutex::new(Vec::new()));
    // nextest runs each test in its own process, so this default is this test's alone.
    let _set =
        tracing::subscriber::set_global_default(crate::login_check::Keep(Arc::clone(&lines)));
    let dir = Dir::new("pg_write_fails", &[]);
    claude_then(&dir, &json!([]), &[]);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    std::fs::remove_dir(records(&dir)).unwrap();
    std::fs::write(records(&dir), b"not a folder").unwrap();
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    host.shutdown();
    let lines = lines.lock().unwrap();
    let warning = lines
        .iter()
        .find(|line| line.contains("process group was not recorded"))
        .expect("the failed write is logged");
    assert!(warning.contains("kind="), "{warning}");
    let data = dir.data().display().to_string();
    assert!(
        lines
            .iter()
            .filter(|line| line.contains("process group") || line.contains("process-group record"))
            .all(|line| !line.contains(&data)),
        "no process-group line names the data folder"
    );
}
