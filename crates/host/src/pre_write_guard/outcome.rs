//! What the pre-write guard answers besides letting a tool go: why it ignored its input
//! ([`Ignored`]) and why it refuses a tool ([`Refusal`]), split from `pre_write_guard.rs` for
//! its size.

use clauseo_protocol::MatterPath;

use super::IGNORED;

/// Why the guard let a tool go without reading what it writes: input it could not read, or a
/// shape it does not know. The tool goes on (fail open: see `pre_write_guard.rs`), with one stderr
/// line saying why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ignored {
    /// Reading stdin failed.
    Unreadable,
    /// Stdin was empty or only whitespace.
    Empty,
    /// Stdin was not JSON, or was cut short.
    NotJson,
    /// No absolute `cwd` and no `--matter` folder.
    NoFolder,
    /// No `tool_name` string or no `tool_input`.
    NoTool,
    /// A known tool's `tool_input` lacked the field it names.
    Shape(&'static str),
}

impl Ignored {
    /// The stderr line, without its newline. It names no input content.
    pub(super) fn line(self) -> String {
        let why = match self {
            Self::Unreadable => "stdin could not be read".to_owned(),
            Self::Empty => "stdin was empty".to_owned(),
            Self::NotJson => "stdin was not JSON, or was cut short".to_owned(),
            Self::NoFolder => "it has no absolute cwd and no --matter was given".to_owned(),
            Self::NoTool => "it has no tool_name or tool_input".to_owned(),
            Self::Shape(wanted) => format!("its tool_input has no {wanted}"),
        };
        format!("{IGNORED}: {why}. The tool goes on unchecked.")
    }
}

/// Why the guard refuses a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A target is held open by Word or another program.
    HeldOpen(MatterPath),
    /// A Codex patch writes in the matter's `.codex` folder, or a Codex shell command names a
    /// `.codex` folder.
    CodexFolder,
    /// A shell command runs `stetdoc mcp` or `stetdoc init`.
    StetdocServe,
    /// The hook input's `permission_mode` is one of [`UNASKED_MODES`].
    UnaskedMode,
}

/// The permission modes in which Claude Code runs every tool unasked (`bypassPermissions`) or
/// refuses every ask unseen (`dontAsk`): Clauseo never offers either and switches a session back
/// from them (`actor/turn/forced_mode.rs`), and meanwhile the guard refuses every tool it checks.
/// `acceptEdits` and `plan` are Clauseo's own and go on.
pub(super) const UNASKED_MODES: [&str; 2] = ["bypassPermissions", "dontAsk"];

impl Refusal {
    /// What the agent is told.
    pub(super) fn reason(&self) -> String {
        match self {
            Self::HeldOpen(path) => format!(
                "{path} is open in Word or another program on this computer. A change made now \
                 would be lost when that program saves it. Ask the lawyer to close it, or write \
                 to a copy with another name."
            ),
            Self::CodexFolder => "Clauseo doesn't let the agent change the .codex folder in a \
                                  matter: files there change what the agent loads and runs."
                .to_owned(),
            Self::StetdocServe => "Clauseo doesn't let the agent run stetdoc mcp or stetdoc init \
                                   in its shell: run the stetdoc command you need directly."
                .to_owned(),
            Self::UnaskedMode => "Clauseo doesn't let the agent change files in a permission mode \
                                  that skips the lawyer's approval. Wait until Clauseo has put \
                                  the session back in its own mode."
                .to_owned(),
        }
    }
}
