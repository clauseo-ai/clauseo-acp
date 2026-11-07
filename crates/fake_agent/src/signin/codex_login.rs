//! Codex's browser login as the fake plays it, with `FAKE_CODEX_PORT`'s listener and its
//! `/cancel`, and `slow-save`'s login file written in two halves.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::time::Duration;

use serde_json::json;

use super::{
    CODEX_CALLBACK_LINE, CODEX_LOGIN_FILE, CODEX_PORT_BUSY_LINE, CODEX_SUCCESS, EMAIL, FAILED,
    Script, codex_plant, complete, say,
};
use crate::{EXIT_OK, write};

/// The variable naming the port Codex's browser login listens on, for the tests of its `/cancel`.
const CODEX_PORT_VAR: &str = "FAKE_CODEX_PORT";
/// The line Codex's browser login prints when `GET /cancel` ends it.
const CODEX_CANCELLED: &str = "Login cancelled";
/// The file a login `/cancel` ended leaves in its home: the tests' proof the request came.
const CANCELLED_FILE: &str = "fake-login-cancelled";
/// The file `slow-save` leaves in its home once the whole login file is written.
const SAVE_FINISHED_FILE: &str = "fake-save-finished";
/// The line `slow-save` prints as its callback arrives, before it starts writing.
const CODEX_CALLBACK_RECEIVED: &str = "callback received";

/// Codex's browser login. It prints where its callback listens. With `FAKE_CODEX_PORT` it binds
/// that port on `127.0.0.1` first: under `never` it then answers requests one at a time until
/// `GET /cancel` ends it (`Login cancelled`, exit 1, nothing written); under `slow-save` it saves
/// the login file slowly while the port stays bound, and exits 0 without answering. Without the
/// variable it binds nothing.
pub(super) fn codex_browser_login(script: &Script, folder: &Path) -> io::Result<i32> {
    if matches!(script, Script::PortBusy) {
        say(CODEX_PORT_BUSY_LINE)?;
        return Ok(FAILED);
    }
    let listener = match callback_port() {
        Some(port) => Some(TcpListener::bind((Ipv4Addr::LOCALHOST, port))?),
        None => None,
    };
    say(CODEX_CALLBACK_LINE)?;
    match (script, &listener) {
        (Script::SlowSave(ms), _) => {
            say(CODEX_CALLBACK_RECEIVED)?;
            slow_save(folder, Duration::from_millis(*ms))?;
            say(CODEX_SUCCESS)?;
            Ok(EXIT_OK)
        }
        (Script::Never, Some(listener)) => serve_until_cancel(listener, folder),
        _ => complete(script, || codex_plant(script, folder), CODEX_SUCCESS),
    }
}

/// The port `FAKE_CODEX_PORT` names, if it names one.
fn callback_port() -> Option<u16> {
    std::env::var(CODEX_PORT_VAR).ok()?.parse().ok()
}

/// Answers each request on `listener` in turn: `GET /cancel` ends the login (`Login cancelled`,
/// exit 1, and [`CANCELLED_FILE`] left in `folder` for the tests); anything else gets a 404.
fn serve_until_cancel(listener: &TcpListener, folder: &Path) -> io::Result<i32> {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let mut first = String::new();
        BufReader::new(&stream).read_line(&mut first)?;
        if first.starts_with("GET /cancel ") {
            let body = format!("{CODEX_CANCELLED}\n");
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            // The caller may have stopped reading; the login ends all the same.
            stream.write_all(answer.as_bytes()).unwrap_or_default();
            say(CODEX_CANCELLED)?;
            write::create_dir_all(folder)?;
            write::write(folder.join(CANCELLED_FILE), b"")?;
            return Ok(FAILED);
        }
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap_or_default();
    }
    Ok(FAILED)
}

/// Saves Codex's login file in `folder` as Codex does: truncated, then written in place, here
/// in two halves with `pause` between them, so a kill in between leaves it cut. The file is
/// the fake's marker, never a credential.
fn slow_save(folder: &Path, pause: Duration) -> io::Result<()> {
    write::create_dir_all(folder)?;
    let marker = format!("{}\n", json!({"authMethod": "chatgpt", "email": EMAIL}));
    let (front, back) = marker.as_bytes().split_at(marker.len() / 2);
    let mut file = write::create(folder.join(CODEX_LOGIN_FILE))?;
    file.write_all(front)?;
    file.flush()?;
    crate::run::effects::pause(pause);
    file.write_all(back)?;
    file.flush()?;
    // The tests' proof the save ran to its end, whatever happens to the file after.
    write::write(folder.join(SAVE_FINISHED_FILE), b"")
}
