//! The `connector` mode: `clauseo-mcp connector <name>`, the stdio MCP server an agent runs for a
//! connector. It relays each JSON-RPC line, unchanged, over loopback TCP to the host's per-launch
//! relay, and back.
//!
//! In order: read the connector's name from the command line, `CLAUSEO_RELAY`
//! (`127.0.0.1:<port>`) and the token from the file `CLAUSEO_RELAY_TOKEN_FILE` names (64
//! lowercase hex characters), exit 2 on anything else; connect and send the handshake line
//! (`handshake.rs`); then pump
//! lines both ways (`pump.rs`). When the relay refuses or is not there, each request the agent
//! sends gets the "not reachable" error with its `id` (`answer.rs`), and the mode keeps reading
//! stdin until it closes. Exit 0 then.
//!
//! The token never reaches stdout, stderr or a log: it is read from a file, not from argv or the
//! environment, and no error names it, its file's content or a line's content.
//!
//! The protocol is the R24 brief's "The relay protocol between `clauseo-mcp connector` and the
//! host" section, with the plan's decisions; the host side is R24's relay in `crates/host`.

mod answer;
mod handshake;
mod head;
#[cfg(test)]
mod in_flight;
mod line;
#[cfg(test)]
mod no_shutdown;
mod pending;
mod pump;
mod socket;
#[cfg(test)]
mod tests;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, Read};
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
use std::sync::{Arc, Mutex};

use crate::Failure;
use line::Out;
pub(crate) use socket::Shut;

/// The mode's command line.
pub const USAGE: &str = "usage: clauseo-mcp connector <name>";

/// The exit code for a bad command line or environment.
const EXIT_USAGE: i32 = 2;
/// The exit code when stdout cannot take a line: the agent has gone.
const EXIT_FAILED: i32 = 1;

/// The longest line the mode relays, in bytes without the newline, in either direction: the
/// host's cap (R24's protocol). A longer agent line gets the line-too-long error and ends the link.
pub(crate) const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// The longest token file the mode reads; a 64-character token plus a line break fits well inside.
const TOKEN_FILE_MAX_BYTES: u64 = 256;
/// The length of the token: 64 lowercase hex characters.
const TOKEN_LEN: usize = 64;
/// The longest connector name: lowercase letters, digits and hyphens.
const NAME_MAX_LEN: usize = 32;

/// What the mode reads from its environment and command line.
struct Settings {
    addr: SocketAddr,
    /// The relay token. Never printed; `Settings` has no `Debug`.
    token: String,
    name: String,
}

/// Runs the mode with the arguments after `connector`, on the process's stdin and stdout. Returns
/// the exit code, or a [`Failure`] for `main` to print (exit 2 for a bad setting).
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let env = |key: &str| std::env::var_os(key);
    let stdout: Out = Arc::new(Mutex::new(io::stdout()));
    run_with(args, &env, io::stdin().lock(), &stdout)
}

/// [`run`] with the environment, stdin and stdout given, so tests need no process state.
pub(crate) fn run_with(
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    stdin: impl BufRead,
    out: &Out,
) -> Result<i32, Failure> {
    run_with_shut(args, env, stdin, out, TcpStream::shutdown)
}

/// [`run_with`] with `shut` as the socket's `shutdown`: tests give one that does nothing, as
/// a stand-in for Windows, where a `shutdown` wakes no blocked read.
pub(crate) fn run_with_shut(
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    stdin: impl BufRead,
    out: &Out,
    shut: Shut,
) -> Result<i32, Failure> {
    let settings = Settings::parse(args, env).map_err(usage_failure)?;
    let (link, refusal) = match handshake::connect(settings.addr, &settings.token, &settings.name) {
        // The reason a link that later fails gives: the same as a refused handshake's default.
        Ok(link) => (Some(link), String::from(handshake::CONNECT_FAILED)),
        Err(reason) => (None, reason),
    };
    // The token is no longer needed; the handshake has been sent.
    drop(settings);
    pump::serve(stdin, out, link, refusal, shut).map_err(|error| Failure {
        code: EXIT_FAILED,
        message: format!("clauseo-mcp connector: stdio failed: {error}"),
    })?;
    Ok(0)
}

/// The one line a bad setting prints: the usage line and the reason. Reasons are fixed texts.
fn usage_failure(reason: &str) -> Failure {
    Failure {
        code: EXIT_USAGE,
        message: format!("{USAGE}: {reason}"),
    }
}

impl Settings {
    /// Reads the settings. The reason on a failure is a fixed text that names no value.
    fn parse(
        args: &[OsString],
        env: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, &'static str> {
        let [name] = args else {
            return Err("give exactly one connector name");
        };
        let name = name
            .to_str()
            .filter(|name| valid_name(name))
            .ok_or("the connector name is lowercase letters, digits and hyphens")?
            .to_owned();
        let relay = env("CLAUSEO_RELAY").ok_or("CLAUSEO_RELAY is not set")?;
        let addr = relay_addr(&relay).ok_or("CLAUSEO_RELAY must be 127.0.0.1 and a port")?;
        let file = env("CLAUSEO_RELAY_TOKEN_FILE").ok_or("CLAUSEO_RELAY_TOKEN_FILE is not set")?;
        let token = read_token(&file)?;
        Ok(Self { addr, token, name })
    }
}

/// Whether `name` is 1 to 32 lowercase ASCII letters, digits or hyphens, not starting with a
/// hyphen. The name goes into the handshake line unescaped, so nothing else may pass.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX_LEN
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// `127.0.0.1:<port>` and nothing else: no other host, no leading zero, port 1 to 65535.
fn relay_addr(value: &OsStr) -> Option<SocketAddr> {
    let port = value.to_str()?.strip_prefix("127.0.0.1:")?;
    if port.starts_with('0') || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port: u16 = port.parse().ok().filter(|port| *port != 0)?;
    Some(SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::LOCALHOST,
        port,
    )))
}

/// The token in the file at `path`: 64 lowercase hex characters, with trailing whitespace
/// allowed. The reasons name no file content.
fn read_token(path: &OsStr) -> Result<String, &'static str> {
    const UNREADABLE: &str = "the token file cannot be read";
    const MALFORMED: &str = "the token file does not hold a 64-character lowercase hex token";
    let file = File::open(path).map_err(|_unopened| UNREADABLE)?;
    let mut bytes = Vec::new();
    file.take(TOKEN_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_unreadable| UNREADABLE)?;
    let token = bytes.trim_ascii_end();
    let well_formed = token.len() == TOKEN_LEN
        && token
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
    if !well_formed {
        return Err(MALFORMED);
    }
    String::from_utf8(token.to_vec()).map_err(|_not_utf8| MALFORMED)
}
