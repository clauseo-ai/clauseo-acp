//! The quit on a signal (Unix): SIGTERM, SIGINT and SIGHUP stop the agents and the host as the
//! normal quit does, instead of ending the process with the agents' process groups left running.
//!
//! Contract:
//! - The signals are caught with handlers (tokio's `signal`, on Tauri's runtime), never blocked
//!   and never ignored. A caught signal goes back to its default action in a child at `exec`,
//!   while a blocked one stays blocked: std's `Command` keeps the parent thread's mask (fork/exec
//!   and `posix_spawn` alike), so a blocked SIGTERM would reach every agent, installer and
//!   opener the app starts, and the ladder's group SIGTERM would do nothing.
//!   `tests/quit_on_signal.rs` checks the agents start with none of the three blocked, ignored
//!   or caught; `crates/process/tests/signal_mask.rs` shows the hazard.
//! - [`QuitSignals::listen`] installs the handlers (inside a tokio runtime); from then on a
//!   signal is held for [`QuitSignals::run`], even one that comes before it first waits. Before
//!   the handlers are in (the first moments of the start, before setup) a signal keeps its
//!   default action: the process ends, and no agent runs yet.
//! - First signal: one log line with the signal's name and nothing else, then `on_first`, which
//!   must return at once (the app's asks for the normal quit, `AppHandle::exit(0)`, so
//!   `RunEvent::ExitRequested` runs `stop_host_then_exit` exactly as for a closed window).
//! - Second signal, whatever the first stop has reached: `flush` (best effort; the app's flushes
//!   the host log), then the process exits at once with `128 + signo`, the shell's code for a
//!   death by that signal. The user insisted; what the stop had not ended yet is left running.
//! - Windows has no such signals here: the Job Object's kill-on-close ends the agents' trees
//!   when the process dies, however it dies (`clauseo-process`).

use std::future::poll_fn;
use std::io;
use std::task::Poll;

use tauri::{AppHandle, Runtime};
use tokio::signal::unix::{Signal, SignalKind, signal};

/// Starts the quit signals' task on Tauri's runtime (`quit_signals.rs`): the first SIGTERM,
/// SIGINT or SIGHUP asks for the normal quit, `AppHandle::exit(0)`, whose
/// `RunEvent::ExitRequested` (tauri 2.12's `AppHandle::exit` sends `RequestExit`, which
/// tauri-runtime-wry turns into it) runs `stop_host_then_exit`; the second flushes the log and
/// exits at once.
pub(crate) fn quit_on_signals<R: Runtime>(app: AppHandle<R>) {
    // Not awaited: it lives as long as the runtime.
    drop(tauri::async_runtime::spawn(async move {
        match QuitSignals::listen() {
            Ok(signals) => {
                let flush = app.clone();
                signals
                    .run(
                        |_signal| app.exit(0),
                        move || crate::host_log::flush_app(&flush),
                    )
                    .await;
            }
            Err(err) => {
                tracing::warn!(%err, "no quit signal handlers: a signal ends the app at once");
            }
        }
    }));
}

/// One of the quit signals: its name, its number, and its stream.
struct Quit {
    name: &'static str,
    signo: i32,
    stream: Signal,
}

/// The handlers for SIGTERM, SIGINT and SIGHUP, installed.
pub struct QuitSignals([Quit; 3]);

impl QuitSignals {
    /// Installs the three handlers. Must run inside a tokio runtime with its signal driver
    /// (Tauri's has it).
    pub fn listen() -> io::Result<QuitSignals> {
        let quit = |name, signo, kind| -> io::Result<Quit> {
            Ok(Quit {
                name,
                signo,
                stream: signal(kind)?,
            })
        };
        Ok(QuitSignals([
            quit("SIGTERM", libc::SIGTERM, SignalKind::terminate())?,
            quit("SIGINT", libc::SIGINT, SignalKind::interrupt())?,
            quit("SIGHUP", libc::SIGHUP, SignalKind::hangup())?,
        ]))
    }

    /// The first signal logs its name and runs `on_first` with it; the second runs `flush` and
    /// exits with [`exit_code`]. Returns only if the runtime stops delivering signals.
    pub async fn run(mut self, on_first: impl FnOnce(&'static str), flush: impl FnOnce()) {
        let Some((first, _)) = self.next().await else {
            return;
        };
        tracing::warn!(signal = first, "quitting on a signal");
        on_first(first);
        let Some((_, second)) = self.next().await else {
            return;
        };
        flush();
        // A second quit signal: the user insists, so the app ends now. (clippy's `exit` lint
        // does not look inside an async body.)
        std::process::exit(exit_code(second));
    }

    /// The next quit signal, as its name and number; `None` once the runtime has shut down.
    async fn next(&mut self) -> Option<(&'static str, i32)> {
        poll_fn(|cx| {
            for quit in &mut self.0 {
                if let Poll::Ready(received) = quit.stream.poll_recv(cx) {
                    return Poll::Ready(received.map(|()| (quit.name, quit.signo)));
                }
            }
            Poll::Pending
        })
        .await
    }
}

/// The exit code for a death by signal number `signo`: `128 + signo`.
pub fn exit_code(signo: i32) -> i32 {
    128 + signo
}

#[cfg(test)]
mod tests {
    use super::exit_code;

    #[test]
    fn the_second_signal_exits_as_a_death_by_it() {
        assert_eq!(exit_code(libc::SIGTERM), 143);
        assert_eq!(exit_code(libc::SIGINT), 130);
        assert_eq!(exit_code(libc::SIGHUP), 129);
    }
}
