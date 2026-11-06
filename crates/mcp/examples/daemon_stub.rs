//! A stand-in for agent-browser in the guard's lifecycle tests (`tests/guard/lifecycle.rs`). On
//! Unix it first starts itself again as `daemon_stub --daemon` in a process group of its own, the
//! way agent-browser's daemon leaves the guard's group (`setsid`); that daemon listens on
//! `<AGENT_BROWSER_SOCKET_DIR>/<AGENT_BROWSER_SESSION>.sock` (removing a stale socket file there
//! first, as agent-browser does), writes its pid to `<session>.pid` beside it, as agent-browser's
//! does, and waits to be killed; once it runs, this process also writes the daemon's pid to
//! `daemon-of-<parent pid>.pid` there (the guard that started it). Then, on every OS, it answers
//! each line that has an `id` with an empty result, and exits 0 at the end of its stdin. With the
//! argument `linger` it first also starts itself as `daemon_stub --linger` in its own process
//! group, standing in for a CLI call of agent-browser's that outlives it: that process writes its
//! pid to `linger.pid` in the socket folder and waits to be killed. With the argument `late` it
//! starts no daemon at once but `daemon_stub --late` in a process group of its own: a daemon that
//! has left agent-browser's group but not yet bound its socket. That process writes its pid to
//! `late.pid` in the socket folder, waits until a file `late-go` appears there, then serves as
//! the daemon does. Cargo builds it for the tests; it is never shipped.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub plays agent-browser's daemon: its socket, its pid file, its start"
)]
mod daemon {
    use std::os::unix::net::UnixListener;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn files() -> (PathBuf, String) {
        let dir = PathBuf::from(std::env::var_os("AGENT_BROWSER_SOCKET_DIR").unwrap_or_default());
        let session = std::env::var("AGENT_BROWSER_SESSION").unwrap_or_default();
        (dir, session)
    }

    /// The daemon: listens, records its pid, accepts and drops connections until killed.
    pub(crate) fn serve() -> std::io::Result<()> {
        let (dir, session) = files();
        let socket = dir.join(format!("{session}.sock"));
        drop(std::fs::remove_file(&socket));
        let listener = UnixListener::bind(socket)?;
        std::fs::write(
            dir.join(format!("{session}.pid")),
            std::process::id().to_string(),
        )?;
        for stream in listener.incoming() {
            drop(stream);
        }
        Ok(())
    }

    /// `--late`: records its pid, waits for `late-go`, then serves.
    pub(crate) fn late() -> std::io::Result<()> {
        let (dir, _) = files();
        std::fs::write(dir.join("late.pid"), std::process::id().to_string())?;
        while !dir.join("late-go").exists() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        serve()
    }

    /// Starts `daemon_stub <flag>` in a process group of its own, not waiting for it.
    pub(crate) fn start_alone(flag: &str) -> std::io::Result<()> {
        let child = Command::new(std::env::current_exe()?)
            .arg(flag)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        drop(child);
        Ok(())
    }

    /// `--linger`: records its pid and waits to be killed.
    pub(crate) fn linger() -> std::io::Result<()> {
        let (dir, _) = files();
        std::fs::write(dir.join("linger.pid"), std::process::id().to_string())?;
        wait_forever()
    }

    /// Blocks until the process is killed.
    fn wait_forever() -> ! {
        loop {
            std::thread::park();
        }
    }

    /// Starts `daemon_stub --linger` in this process's group, not waiting for it.
    pub(crate) fn start_linger() -> std::io::Result<()> {
        let child = Command::new(std::env::current_exe()?)
            .arg("--linger")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        drop(child);
        Ok(())
    }

    /// Starts the daemon in a group of its own and waits until its pid file exists.
    pub(crate) fn start() -> std::io::Result<()> {
        let (dir, session) = files();
        let mut daemon = Command::new(std::env::current_exe()?)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let pid_file = dir.join(format!("{session}.pid"));
        while !pid_file.exists() {
            if daemon.try_wait()?.is_some() {
                return Err(std::io::Error::other("the daemon stub exited"));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let guard = std::os::unix::process::parent_id();
        std::fs::write(
            dir.join(format!("daemon-of-{guard}.pid")),
            daemon.id().to_string(),
        )?;
        // Not reaped here: it outlives this process, as agent-browser's daemon does.
        drop(daemon);
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        match std::env::args().nth(1).as_deref() {
            Some("--daemon") => return daemon::serve(),
            Some("--linger") => return daemon::linger(),
            Some("--late") => return daemon::late(),
            Some("linger") => daemon::start_linger()?,
            _ => {}
        }
        if std::env::args().nth(1).as_deref() == Some("late") {
            daemon::start_alone("--late")?;
        } else {
            daemon::start()?;
        }
    }
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(message) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if let Some(id) = message.get("id") {
            writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": {}}))?;
            out.flush()?;
        }
    }
    Ok(())
}
