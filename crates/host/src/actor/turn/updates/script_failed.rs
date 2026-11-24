//! Claude Code's line for a failed Code Mode script. Claude Code 2.1.330 sends one as an
//! `informational` warning, "Code that Claude ran failed with this error: …", when a script it
//! ran in its `REPL` tool threw or was refused before it ran (`import` and `require` are not
//! available there, say), and Claude then carries on another way. claude-agent-acp 0.110.1
//! passes it as a `notice` whose title is that line. Its words are Claude Code's and may name
//! paths, so the thread gets the host's own words with [`NoticeKind::ScriptFailed`] instead, as
//! an `info` notice, and the line goes only to the host log with its paths taken out.

use clauseo_protocol::{Notice, NoticeKind, NoticeLevel, ThreadId};

/// The fixed start of Claude Code's line (CLI 2.1.330).
pub(super) const PREFIX: &str = "Code that Claude ran failed with this error:";

/// The notice's text: what the app shows when it has no words of its own for the kind.
pub(super) const TEXT: &str = "A script didn't run; Claude tried another way.";

/// The thread's notice for an agent notice of `level` saying `text`: [`TEXT`] as an `info`
/// [`NoticeKind::ScriptFailed`] when `text` starts with [`PREFIX`], else the agent's own, with
/// no kind.
pub(super) fn notice(thread: ThreadId, level: NoticeLevel, text: String) -> Notice {
    if !text.starts_with(PREFIX) {
        return Notice {
            level,
            text,
            kind: None,
        };
    }
    tracing::info!(%thread, line = %without_paths(&text), "a Code Mode script of Claude's failed");
    Notice {
        level: NoticeLevel::Info,
        text: TEXT.to_owned(),
        kind: Some(NoticeKind::ScriptFailed),
    }
}

/// `text` with each word that holds a `/` or a `\` replaced by `<path>`.
fn without_paths(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if word.contains(['/', '\\']) {
                "<path>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::{NoticeKind, NoticeLevel, ThreadId};

    use super::{TEXT, notice, without_paths};

    /// The notice's title as claude-agent-acp 0.110.1 sent it in a live Code Mode session.
    const LIVE: &str = "Code that Claude ran failed with this error: Nothing ran: import and \
                        require aren't available here";

    fn thread() -> ThreadId {
        ThreadId::new(uuid::Uuid::now_v7()).unwrap()
    }

    #[test]
    fn claude_codes_failed_script_line_becomes_the_hosts_own_notice() {
        let shown = notice(thread(), NoticeLevel::Warning, LIVE.to_owned());
        assert_eq!(shown.kind, Some(NoticeKind::ScriptFailed));
        assert_eq!(shown.level, NoticeLevel::Info);
        assert_eq!(shown.text, TEXT);
        let event = serde_json::to_string(&shown).unwrap();
        assert!(!event.contains("import and require"), "{event}");
        assert!(event.contains(r#""kind":"script_failed""#), "{event}");
    }

    #[test]
    fn any_other_notice_is_the_agents_own() {
        for (level, text) in [
            (
                NoticeLevel::Warning,
                "Auto mode unavailable\n\nThe selected model does not support Auto mode.",
            ),
            (
                NoticeLevel::Info,
                "Code that Claude ran was stopped before it finished.",
            ),
            (
                NoticeLevel::Warning,
                "Note: Code that Claude ran failed with this error: x",
            ),
        ] {
            let shown = notice(thread(), level, text.to_owned());
            assert_eq!(shown.kind, None, "{text}");
            assert_eq!(shown.level, level, "{text}");
            assert_eq!(shown.text, text);
        }
    }

    #[test]
    fn the_log_line_names_no_path() {
        let line = without_paths(
            "Code that Claude ran failed with this error: write /Users/a/m/Lease.docx: denied C:\\m\\x",
        );
        assert_eq!(
            line,
            "Code that Claude ran failed with this error: write <path> denied <path>"
        );
    }
}
