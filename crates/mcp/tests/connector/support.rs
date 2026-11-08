//! The helpers for the connector's process tests: the scratch folder, the command line and the
//! process starts. The one place these tests change files or start the built mode.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

/// The binary under test.
pub(crate) const MAIN: &str = env!("CARGO_BIN_EXE_clauseo-mcp");
/// A 64-character lowercase token.
pub(crate) const TOKEN: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
/// The relay's answer that accepts the handshake.
pub(crate) const OK: &str = r#"{"clauseo_relay":1,"ok":true}"#;

/// The scratch folders' writes: the one place these tests change files, since `clippy.toml` bans
/// std's writes everywhere else.
#[expect(
    clippy::disallowed_methods,
    reason = "the connector tests' scratch folders change files here alone; clippy.toml bans std's writes elsewhere"
)]
mod write {
    use std::io;
    use std::path::Path;

    /// `fs::write`.
    pub(crate) fn write(path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    /// `fs::create_dir_all`.
    pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    /// `fs::remove_dir_all`.
    pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }
}

/// Runs `command` to its end with stdin closed, and returns its output.
#[expect(
    clippy::disallowed_methods,
    reason = "the connector tests run the built mode here alone; agents and tools start through the process crate"
)]
pub(crate) fn run(command: &mut Command) -> Output {
    command.stdin(Stdio::null()).output().unwrap()
}

/// Starts `command` with stdin and stdout piped; its stderr is inherited.
#[expect(
    clippy::disallowed_methods,
    reason = "the connector tests start the built mode here alone; agents and tools start through the process crate"
)]
pub(crate) fn start(command: &mut Command) -> Child {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

/// A folder for one test's token file, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-mcp-connector-bin-{test}-{}",
            std::process::id()
        ));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The token file, holding `content`.
    pub(crate) fn token_file(&self, content: &str) -> PathBuf {
        let path = self.0.join("relay-token");
        write::write(&path, content).unwrap();
        path
    }

    /// A path in the folder that nothing is written to.
    pub(crate) fn missing(&self) -> PathBuf {
        self.0.join("missing")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// A `clauseo-mcp connector case-law` with the relay address and token file given, and nothing
/// else in its environment but, on Windows, `SystemRoot` and `windir`: Winsock cannot start
/// without them, so with a cleared environment every socket fails (WSAEPROVIDERFAILEDINIT) and the
/// mode could only answer `connect_failed`. Every real launcher passes it on: the host's spec and
/// the agents' MCP clients both keep `SystemRoot`.
pub(crate) fn connector(relay: &str, token_file: &Path) -> Command {
    let mut command = Command::new(MAIN);
    command
        .args(["connector", "case-law"])
        .env_clear()
        .env("CLAUSEO_RELAY", relay)
        .env("CLAUSEO_RELAY_TOKEN_FILE", token_file);
    if cfg!(windows) {
        // Windows names are case-blind, so this covers `SYSTEMROOT` too.
        for name in ["SystemRoot", "windir"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
    }
    command
}

/// A port nothing listens on: bound, then dropped.
pub(crate) fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
