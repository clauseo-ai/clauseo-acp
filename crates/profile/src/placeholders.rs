//! The placeholders the profile leaves for the host to fill per session, and the one way to
//! fill them.
//!
//! The hook sees only the agent's id, so anything that differs per session goes out as a
//! token. The host fills [`IN_INSTRUCTIONS`] in `_meta.systemPrompt.append` and in Codex's
//! `developer_instructions`, and [`IN_SERVERS`] in every argument and variable of the
//! `browser` entry, both with [`fill`]. `{author}` is not here: the profile fills it itself
//! (`instructions::Instructions::compose`), since it knows the author when it is built.
//!
//! This module is the one home of the token strings and of the fill: the host depends on
//! `clauseo-profile` and calls [`fill`] with these constants, with no copy of its own
//! (decision record 0030).
//!
//! The host fills decoded strings (one argument, one variable's value, the instructions' text)
//! and only then encodes them for their destination (`serde_json` for `_meta` and
//! `CODEX_CONFIG`; MCP arguments go as a list, with no shell). It never fills inside text that
//! is already encoded.

/// The session's work folder: `<data>/run/<session id>/work` until stage C's per-agent state
/// folders exist. Instructions only.
pub const WORK_FOLDER: &str = "{work_folder}";
/// The matter's notes folder, `<data>/agent-state/<matter id>/notes` (R22). Instructions only.
pub const MATTER_NOTES: &str = "{matter_notes}";
/// The session's browser state folder, `<data>/run/<session id>/browser`, as the browser
/// guard's `--cwd`. The host joins it with the platform's own separators and fills it with a
/// `dunce`-simplified path (no `\\?\` prefix on Windows), as one whole argument.
pub const BROWSER_DIR: &str = "{browser_dir}";
/// The egress proxy's port for the session. Unfillable (no proxy): the host drops `browser`
/// and shows `NoProxy`.
pub const PROXY_PORT: &str = "{proxy_port}";
/// agent-browser's session name: the session's id on macOS and Linux; on Windows a name the
/// host draws so that agent-browser's daemon port, a hash of the name, is one it claimed.
pub const SESSION_ID: &str = "{session_id}";
/// The Chrome the host picked for this session. Unfillable (none installed): the host drops
/// `browser` and shows `BrowserNotInstalled`.
pub const BROWSER_EXECUTABLE: &str = "{browser_executable}";

/// macOS and Linux only: the session's browser socket folder, `/tmp/clauseo-ab-<16 random>`,
/// which the host makes for the session (mode 0700) and removes once the launch's process tree
/// has exited, as `AGENT_BROWSER_SOCKET_DIR`. Unfillable (the folder could not be made): the
/// host drops `browser` and shows `BrowserNotInstalled`.
pub const SOCKET_DIR: &str = "{socket_dir}";
/// The loopback port the host claimed for the session's stream server, as
/// `AGENT_BROWSER_STREAM_PORT`. Unfillable (no port claimed): the host drops `browser`.
pub const STREAM_PORT: &str = "{stream_port}";
/// The loopback port the host claimed for the session's Chrome debugging endpoint, as the
/// guard's `--cdp-port`. Unfillable (no port claimed): the host drops `browser`.
pub const CDP_PORT: &str = "{cdp_port}";
/// Windows only: the loopback port the host claimed for agent-browser's daemon, as the guard's
/// `--daemon-port`. Unfillable (no port claimed): the host drops `browser`.
pub const DAEMON_PORT: &str = "{daemon_port}";

/// The tokens the host fills in the instructions. An unfillable one is a host bug: the host
/// fails the launch with `agent.launch_failed`, naming the token ([`Unfilled`]'s message).
pub const IN_INSTRUCTIONS: [&str; 2] = [WORK_FOLDER, MATTER_NOTES];
/// The tokens the host fills in the `browser` entry. The entry holds [`SOCKET_DIR`] on macOS
/// and Linux only and [`DAEMON_PORT`] on Windows only; the host fills whichever it finds.
pub const IN_SERVERS: [&str; 8] = [
    BROWSER_DIR,
    PROXY_PORT,
    SESSION_ID,
    BROWSER_EXECUTABLE,
    SOCKET_DIR,
    STREAM_PORT,
    CDP_PORT,
    DAEMON_PORT,
];

/// A token in `tokens` that [`fill`] had no value for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no value for the placeholder {0}")]
pub struct Unfilled(pub &'static str);

/// `text` with each of `tokens` replaced by its value in `values`, in one pass from left to
/// right: a value put in is never scanned again, so a folder path that itself holds
/// `{proxy_port}` stays as it is. Text that is no token of `tokens` stays too. A token that
/// `text` holds and `values` has no entry for is an error.
pub fn fill(
    text: &str,
    tokens: &[&'static str],
    values: &[(&str, &str)],
) -> Result<String, Unfilled> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('{') {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        if let Some(token) = tokens.iter().find(|token| from.starts_with(**token)) {
            let value = values
                .iter()
                .find(|(name, _)| name == token)
                .map(|(_, value)| *value)
                .ok_or(Unfilled(token))?;
            out.push_str(value);
            rest = from.get(token.len()..).unwrap_or_default();
        } else {
            out.push('{');
            rest = from.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filled_value_is_never_scanned_again() {
        let filled = fill(
            "--cwd {browser_dir} on {proxy_port}",
            &IN_SERVERS,
            &[(BROWSER_DIR, "/tmp/{proxy_port}"), (PROXY_PORT, "8123")],
        );
        assert_eq!(filled.as_deref(), Ok("--cwd /tmp/{proxy_port} on 8123"));
    }

    #[test]
    fn a_token_with_no_value_is_an_error_and_other_braces_stay() {
        assert_eq!(
            fill("http://127.0.0.1:{proxy_port}", &IN_SERVERS, &[]),
            Err(Unfilled(PROXY_PORT))
        );
        assert_eq!(
            fill("{x} {work_folder", &IN_INSTRUCTIONS, &[]).as_deref(),
            Ok("{x} {work_folder")
        );
        assert_eq!(
            fill(
                "{work_folder}{matter_notes}",
                &IN_INSTRUCTIONS,
                &[(WORK_FOLDER, "w"), (MATTER_NOTES, "m")]
            )
            .as_deref(),
            Ok("wm")
        );
    }

    #[test]
    fn the_port_and_socket_tokens_fill_in_one_pass() {
        let values = [
            (SOCKET_DIR, "/tmp/clauseo-ab-0123456789abcdef"),
            (STREAM_PORT, "61100"),
            (CDP_PORT, "{stream_port}"),
            (DAEMON_PORT, "61102"),
        ];
        assert_eq!(
            fill(
                "{socket_dir} {stream_port} {cdp_port} {daemon_port}",
                &IN_SERVERS,
                &values
            )
            .as_deref(),
            Ok("/tmp/clauseo-ab-0123456789abcdef 61100 {stream_port} 61102")
        );
        for token in [SOCKET_DIR, STREAM_PORT, CDP_PORT, DAEMON_PORT] {
            assert_eq!(fill(token, &IN_SERVERS, &[]), Err(Unfilled(token)));
        }
    }

    #[test]
    fn a_server_token_in_the_instructions_is_left_alone() {
        assert_eq!(
            fill(
                "{browser_dir} {work_folder}",
                &IN_INSTRUCTIONS,
                &[(WORK_FOLDER, "w"), (MATTER_NOTES, "m")]
            )
            .as_deref(),
            Ok("{browser_dir} w")
        );
    }
}
