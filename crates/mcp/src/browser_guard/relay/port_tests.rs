//! The port files checked through the relay: for the session's whole life (a match does not end
//! the checks), the stream server's on every OS, and a refusal ends the child.

#[cfg(unix)]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use super::super::port_file::Expected;
use super::refusal;

/// A folder of its own under the system temp folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("clauseo-mcp-relay-{test}-{}", std::process::id()));
        drop(crate::write::remove_dir_all(&path));
        crate::write::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(crate::write::remove_dir_all(&self.0));
    }
}

/// A match is no verdict for later: the same check, read again after the file changed, refuses.
#[test]
fn a_match_does_not_end_the_checks() {
    let scratch = Scratch::new("refusal");
    let checks = [
        Expected::daemon(&scratch.0, "s", 61_102),
        Expected::stream(&scratch.0, "s", 61_100),
    ];
    assert_eq!(refusal(&checks), None, "no files yet");
    crate::write::write(&checks[0].file, "61102").unwrap();
    crate::write::write(&checks[1].file, "61100").unwrap();
    assert_eq!(refusal(&checks), None, "both on their claimed ports");
    crate::write::write(&checks[0].file, "50123").unwrap();
    assert_eq!(
        refusal(&checks).as_deref(),
        Some("agent-browser's daemon port file says 50123, not the claimed port 61102")
    );
    crate::write::write(&checks[0].file, "61102").unwrap();
    crate::write::write(&checks[1].file, "40117").unwrap();
    assert_eq!(
        refusal(&checks).as_deref(),
        Some("agent-browser's stream port file says 40117, not the claimed port 61100")
    );
}

/// A shell script standing in for agent-browser.
#[cfg(unix)]
fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-agent-browser.sh");
    crate::write::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    crate::write::set_mode(&path, 0o755).unwrap();
    path
}

#[cfg(unix)]
fn env(pairs: &[(&str, &Path)]) -> BTreeMap<String, OsString> {
    let mut env = BTreeMap::from([("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]);
    for (name, value) in pairs {
        env.insert((*name).to_owned(), value.as_os_str().to_owned());
    }
    env
}

/// The review's repro: the `.port` file holds the claimed port at first (the first daemon, or
/// one the agent planted), then another after the daemon restarts. The guard refuses.
#[cfg(unix)]
#[test]
fn a_daemon_restarted_on_another_port_after_a_first_match_is_refused() {
    let scratch = Scratch::new("restart");
    let port_file = scratch.0.join("s.port");
    crate::write::write(&port_file, "61102").unwrap();
    // The guard reads the file after line 1 (a match); the pause lets that read land before the
    // rewrite, so line 2's read is the one that refuses. Without the fix the guard stops reading
    // after the match and the child runs on until the guard's close timeouts end it.
    let program = script(
        &scratch.0,
        r#"echo '{"jsonrpc":"2.0","id":1,"result":{}}'
sleep 1
printf 50123 > "$PORT_FILE"
echo '{"jsonrpc":"2.0","id":2,"result":{}}'
exec sleep 30"#,
    );
    let outcome = super::run(
        &program,
        &[],
        &scratch.0,
        &env(&[("PORT_FILE", &port_file)]),
        vec![Expected::daemon(&scratch.0, "s", 61_102)],
    )
    .unwrap();
    assert_eq!(
        outcome,
        super::Outcome::PortRefused(
            "agent-browser's daemon port file says 50123, not the claimed port 61102".to_owned()
        )
    );
}

/// The stream server's fallback: a `.stream` file naming another port ends the child, on Unix
/// too.
#[cfg(unix)]
#[test]
fn a_stream_server_on_another_port_is_refused() {
    let scratch = Scratch::new("stream");
    let program = script(
        &scratch.0,
        r#"printf 40117 > "$DIR/s.stream"
echo '{"jsonrpc":"2.0","id":1,"result":{}}'
exec sleep 30"#,
    );
    let outcome = super::run(
        &program,
        &[],
        &scratch.0,
        &env(&[("DIR", &scratch.0)]),
        vec![Expected::stream(&scratch.0, "s", 61_100)],
    )
    .unwrap();
    assert_eq!(
        outcome,
        super::Outcome::PortRefused(
            "agent-browser's stream port file says 40117, not the claimed port 61100".to_owned()
        )
    );
}
