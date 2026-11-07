//! The stop's parts: the `GET /cancel` on the wire, and the 0-byte rule's two looks.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::time::Duration;

use clauseo_protocol::AgentId;

use super::{EMPTY_AGE, OnStop, Stopping, after_kill, before_status, codex_home_of, send_cancel};
use crate::signin::reuse::{login_file_for_tests, write_for_tests};
use crate::write::TestDir;

fn agent() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// The request is a plain `GET /cancel` to the local port, by HTTP/1.1, closing after.
#[test]
fn the_cancel_is_a_plain_get_on_the_local_port() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            head.push(line.trim_end().to_owned());
        }
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        head
    });
    send_cancel(port);
    let head = server.join().unwrap();
    assert_eq!(
        head.first().map(String::as_str),
        Some("GET /cancel HTTP/1.1")
    );
    assert!(
        head.contains(&format!("Host: 127.0.0.1:{port}")),
        "{head:?}"
    );
    assert!(head.contains(&"Connection: close".to_owned()), "{head:?}");
}

/// Nothing listening: the send gives up without an error.
#[test]
fn a_cancel_to_a_closed_port_is_ignored() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    send_cancel(port);
}

/// Only a non-empty `CODEX_HOME` names the run's home.
#[test]
fn the_runs_home_is_its_codex_home() {
    let mut env = BTreeMap::new();
    assert_eq!(codex_home_of(&env), None);
    env.insert(OsString::from("CODEX_HOME"), OsString::new());
    assert_eq!(codex_home_of(&env), None);
    env.insert(OsString::from("CODEX_HOME"), OsString::from("/x/home"));
    assert_eq!(codex_home_of(&env), Some("/x/home".into()));
}

/// After a kill: the empty file goes, a full one stays, a run that writes nothing changes
/// nothing.
#[test]
fn after_a_kill_only_an_empty_file_goes() {
    let dir = TestDir::new("stop-after-kill");
    let home = dir.0.join("home");
    let file = login_file_for_tests(&home);
    let env = BTreeMap::from([(OsString::from("CODEX_HOME"), home.into_os_string())]);
    let writes = OnStop {
        how: Stopping::Wait,
        what: "codex logout",
        writes: true,
    };
    write_for_tests(&file, b"");
    let lawyer = dir.0.join("lawyer");
    let lawyer = Some(lawyer.as_path());
    after_kill(OnStop::status("codex login status"), &env, lawyer, &agent());
    assert!(file.exists(), "a run that writes nothing removed it");
    after_kill(writes, &env, None, &agent());
    assert!(file.exists(), "removed with the lawyer's home unknown");
    after_kill(writes, &env, lawyer, &agent());
    assert!(!file.exists());
    write_for_tests(&file, b"{}");
    after_kill(writes, &env, lawyer, &agent());
    assert!(file.exists());
}

/// The lawyer's own file, reused through a hard link, is never removed, empty or not.
#[test]
fn a_reused_empty_file_is_never_removed() {
    let dir = TestDir::new("stop-reused");
    let home = dir.0.join("home");
    let lawyer = dir.0.join("lawyer");
    let lawyers = login_file_for_tests(&lawyer.join(".codex"));
    write_for_tests(&lawyers, b"");
    let file = login_file_for_tests(&home);
    crate::write::create_dir_all(&home).unwrap();
    crate::write::hard_link(&lawyers, &file).unwrap();
    let env = BTreeMap::from([(OsString::from("CODEX_HOME"), home.clone().into_os_string())]);
    let writes = OnStop {
        how: Stopping::Wait,
        what: "codex logout",
        writes: true,
    };
    after_kill(writes, &env, Some(&lawyer), &agent());
    let modified = std::fs::symlink_metadata(&file)
        .unwrap()
        .modified()
        .unwrap();
    before_status(&home, Some(&lawyer), modified + EMPTY_AGE * 2, &agent());
    assert!(file.exists() && lawyers.exists());
}

/// Before a status read: an empty file goes once untouched for `EMPTY_AGE`; a just-written one
/// (a Codex writing it now) and a full one stay.
#[test]
fn before_a_status_an_old_empty_file_goes() {
    let dir = TestDir::new("stop-before-status");
    let home = dir.0.join("home");
    let file = login_file_for_tests(&home);
    write_for_tests(&file, b"");
    let modified = std::fs::symlink_metadata(&file)
        .unwrap()
        .modified()
        .unwrap();
    let lawyer = dir.0.join("lawyer");
    let lawyer = Some(lawyer.as_path());
    before_status(&home, lawyer, modified + Duration::from_secs(1), &agent());
    assert!(file.exists(), "a file written a second ago was removed");
    before_status(
        &home,
        lawyer,
        modified + EMPTY_AGE + Duration::from_secs(1),
        &agent(),
    );
    assert!(!file.exists());
    write_for_tests(&file, b"{}");
    let modified = std::fs::symlink_metadata(&file)
        .unwrap()
        .modified()
        .unwrap();
    before_status(&home, lawyer, modified + EMPTY_AGE * 2, &agent());
    assert!(file.exists());
}
