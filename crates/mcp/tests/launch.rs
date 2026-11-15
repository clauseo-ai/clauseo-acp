//! The built `clauseo-mcp launch <file>`: the server it starts gets the file's arguments and
//! variables over the mode's own environment, runs as the mode's own process on Unix (`exec`),
//! and no value from a file that does not read reaches stdout or stderr.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The binary under test.
const MAIN: &str = env!("CARGO_BIN_EXE_clauseo-mcp");
/// The value no output may hold unless the server prints it itself.
const MARKER: &str = "MARKER-SECRET-123";

/// The scratch folders' writes: the one place these tests change files.
#[expect(
    clippy::disallowed_methods,
    reason = "the launch tests' scratch folders change files here alone; clippy.toml bans std's writes elsewhere"
)]
mod write {
    use std::io;
    use std::path::Path;

    pub(crate) fn write(path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }
}

/// Runs `command` to its end with stdin closed and returns its output and its pid.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the launch tests start the built mode here alone; agents and tools start through the process crate"
)]
fn run(command: &mut Command) -> (Output, u32) {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    (child.wait_with_output().unwrap(), pid)
}

/// A folder for one test's launch file, removed on drop.
#[cfg(test)]
struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    fn new(test: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("clauseo-mcp-launch-{test}-{}", std::process::id()));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The launch file, holding `content`.
    fn file(&self, content: &str) -> PathBuf {
        let file = self.0.join("server.json");
        write::write(&file, content).unwrap();
        file
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

fn launch(file: &Path) -> (Output, u32) {
    run(Command::new(MAIN).arg("launch").arg(file))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The server prints its pid, the file's variable, an inherited one and its arguments: one
/// process (the pid the test started), the variable set over the mode's own environment.
#[cfg(unix)]
#[test]
fn the_server_is_the_modes_own_process_with_the_files_args_and_env() {
    let scratch = Scratch::new("exec");
    let file = scratch.file(&format!(
        r#"{{"v":1,"command":"/bin/sh","args":["-c","printf '%s|%s|%s|%s' $$ \"$SECRET\" \"$KEPT\" \"$1\"","sh","one two"],"env":[["SECRET","{MARKER}"],["KEPT","over"]]}}"#
    ));
    let (output, pid) = run(Command::new(MAIN)
        .arg("launch")
        .arg(&file)
        .env("KEPT", "inherited")
        .env("OWN", "x"));
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), format!("{pid}|{MARKER}|over|one two"));
}

/// The mode's own environment reaches the server under the file's.
#[cfg(unix)]
#[test]
fn the_modes_environment_reaches_the_server() {
    let scratch = Scratch::new("inherit");
    let file =
        scratch.file(r#"{"v":1,"command":"/bin/sh","args":["-c","printf %s \"$OWN\""],"env":[]}"#);
    let (output, _) = run(Command::new(MAIN)
        .arg("launch")
        .arg(&file)
        .env("OWN", "own"));
    assert_eq!(text(&output.stdout), "own");
}

/// The file stays: a server the agent restarts mid-session starts from it again.
#[cfg(unix)]
#[test]
fn a_restarted_server_reads_the_same_file_again() {
    let scratch = Scratch::new("again");
    let file = scratch.file(&format!(
        r#"{{"v":1,"command":"/bin/sh","args":["-c","printf %s \"$SECRET\""],"env":[["SECRET","{MARKER}"]]}}"#
    ));
    for _ in 0..2 {
        let (output, _) = launch(&file);
        assert_eq!(text(&output.stdout), MARKER);
    }
    assert!(file.exists());
}

/// A file that does not read, a wrong version, a relative program, an unknown key, a bad
/// variable name: exit 2, and no value of the file in either stream.
#[test]
fn a_file_that_does_not_read_exits_2_and_shows_no_value() {
    let scratch = Scratch::new("bad");
    for content in [
        format!(r#"{{"v":1,"command":"/bin/sh","args":["{MARKER}"],"env":[["A",5]]}}"#),
        format!(r#"{{"v":1,"command":"/bin/sh","args":[],"env":[["A","{MARKER}"]],"x":1}}"#),
        format!(r#"{{"v":2,"command":"/bin/sh","args":["{MARKER}"],"env":[]}}"#),
        format!(r#"{{"v":1,"command":"sh","args":["{MARKER}"],"env":[]}}"#),
        format!(r#"{{"v":1,"command":"/bin/sh","args":[],"env":[["A=B","{MARKER}"]]}}"#),
        format!(r#"{{"v":1,"command":"{MARKER}","args":[],"env":[]}}"#),
        format!("{{\"v\":1,\"command\":\"/bin/sh\",\"args\":[\"{MARKER}"),
    ] {
        let file = scratch.file(&content);
        let (output, _) = launch(&file);
        assert_eq!(output.status.code(), Some(2), "{content}");
        assert!(output.stdout.is_empty(), "{content}");
        assert!(!text(&output.stderr).contains("MARKER"), "{content}");
    }
}

/// A program that is not there: exit 1, named by the error's kind only.
#[test]
fn a_missing_program_exits_1() {
    let scratch = Scratch::new("missing");
    let program = scratch.0.join("no-such-server");
    let file = scratch.file(&format!(
        r#"{{"v":1,"command":{},"args":["{MARKER}"],"env":[["A","{MARKER}"]]}}"#,
        serde_json::to_string(&program.display().to_string()).unwrap()
    ));
    let (output, _) = launch(&file);
    assert_eq!(output.status.code(), Some(1));
    assert!(!text(&output.stderr).contains("MARKER"));
}

/// No file, a missing file, two files: exit 2.
#[test]
fn a_bad_command_line_exits_2() {
    let scratch = Scratch::new("usage");
    let missing = scratch.0.join("absent.json");
    for args in [
        vec![],
        vec![missing.clone()],
        vec![missing.clone(), missing],
    ] {
        let (output, _) = run(Command::new(MAIN).arg("launch").args(&args));
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}
