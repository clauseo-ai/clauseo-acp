//! The guard's answers other than letting a tool go quietly: the shared line, a refusal and its
//! reason ([`Refusal`]), and input it ignores ([`Ignored`]) with its line.

use clauseo_protocol::MatterPath;

/// What the agent reads when the guard could not run, ended with neither of its codes, or (as
/// Codex's hook) could not read its input.
pub const UNCHECKED: &str = "Clauseo could not check this step, so it did not run.";

/// The start of the stderr line saying the guard ignored its input ([`Ignored::line`]).
pub const IGNORED: &str = "Clauseo's pre-write guard ignored its input";

/// Why the guard let a tool go without reading what it writes: input it could not read, or a
/// shape it does not know. The tool goes on (fail open: see the module doc), with one stderr
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
    /// Input that was not read as a hook object at all: unreadable, empty or not JSON.
    pub(super) fn unread(self) -> bool {
        matches!(self, Self::Unreadable | Self::Empty | Self::NotJson)
    }

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
    /// A Codex patch or shell command reaches Codex's home (`CODEX_HOME`), or a shell command
    /// names its variable.
    CodexHome,
    /// A shell command runs `stetdoc mcp` or `stetdoc init`.
    StetdocServe,
    /// A shell command nests `$(…)` too deep, or holds too much nested text, to be read
    /// (`words::MAX_NESTING`, `words::MAX_NESTED_BYTES`).
    Unread,
    /// A file tool writes, or a shell command that does more than read names, one of the
    /// protected paths (`protected.rs`): Claude's own settings, instructions, skills and plugins,
    /// or a plugin folder of Clauseo's.
    Protected,
}

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
            Self::CodexHome => "Clauseo doesn't let the agent touch Codex's own folder \
                                (CODEX_HOME): files there change what the agent loads and runs. \
                                Run tools by name (for example rg), not by their path in that \
                                folder."
                .to_owned(),
            Self::StetdocServe => "Clauseo doesn't let the agent run stetdoc mcp or stetdoc init \
                                   in its shell: run the stetdoc command you need directly."
                .to_owned(),
            Self::Unread => "Clauseo can't read this command: it is nested too deep or is too \
                             long. Split it into smaller commands."
                .to_owned(),
            Self::Protected => "Clauseo doesn't let the agent change Claude's own settings or \
                                Clauseo's plugin files."
                .to_owned(),
        }
    }
}
