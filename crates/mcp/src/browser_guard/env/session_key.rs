//! The session key (Windows): drawn once per guard run and given only to the agent-browser the
//! guard connects to the browser. Re-exported by `env`, which keeps every path to it.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::set_cdp;

/// Windows only: the session key, 64 lower-case hex digits from the OS's random source, drawn
/// once per guard run ([`new_session_key`]). The patched agent-browser (W2) serves its daemon's
/// pipe only to a process that proves it, and its CLI talks only to a daemon that proves it, so
/// an unsandboxed process of the user (Claude's shell on Windows has no OS sandbox) can no
/// longer send the daemon raw commands past [`crate`]'s filter. Only the agent-browser that
/// [`agent_env`] is for gets it, created hardened in the guard's job (`job.rs`); it passes it
/// to its hardened children only. Never in a file, on a command line, in Chrome's environment
/// ([`super::chrome_env`]) or the one-shot's, and taken out of agent-browser's output
/// (`relay/redact.rs`).
pub(crate) const SESSION_KEY: &str = "AGENT_BROWSER_SESSION_KEY";

/// The environment of the agent-browser connected to the browser: `base` (the launcher's,
/// which the one-shot gets as it is) with [`super::CDP`] and, when the guard drew one (Windows),
/// the [`SESSION_KEY`].
pub(crate) fn agent_env(
    base: &BTreeMap<String, OsString>,
    endpoint: &str,
    session_key: Option<&str>,
) -> BTreeMap<String, OsString> {
    let mut env = base.clone();
    set_cdp(&mut env, endpoint);
    if let Some(key) = session_key {
        env.insert(SESSION_KEY.to_owned(), OsString::from(key));
    }
    env
}

/// A new [`SESSION_KEY`]: 32 bytes from the OS's random source as 64 lower-case hex digits.
pub(crate) fn new_session_key() -> std::io::Result<String> {
    crate::browser_guard::devtools::new_secret()
}
