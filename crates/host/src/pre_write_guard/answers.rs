//! The guard's answers other than letting a tool go quietly: the shared line, a refusal and its
//! reason ([`Refusal`]), and input it ignores ([`Ignored`]) with its line.

use clauseo_protocol::MatterPath;

/// What the agent reads when the guard could not run, ended with neither of its codes, or (as
/// Codex's hook) could not read its input.
pub const UNCHECKED: &str = "Clauseo could not check this step, so it did not run.";

/// What the lawyer reads in place of a blocked step's text when Claude Code's guard hook could
/// not answer: it crashed, timed out, ended with a code other than 0 or 2, or (its own line,
/// [`COULD_NOT_CHECK_AGENT`]) caught an error of its own ([`could_not_answer`]).
pub const COULD_NOT_CHECK: &str = "Clauseo couldn't check whether this paper is open elsewhere, so \
                                   nothing was changed. Try again.";

/// What the agent reads when the guard catches an error of its own (Claude Code's hook): the
/// lawyer's words, with one retry and then the lawyer, so a guard that keeps failing does not
/// keep the agent retrying.
pub const COULD_NOT_CHECK_AGENT: &str = "Clauseo couldn't check whether this paper is open \
                                         elsewhere, so nothing was changed. Try once more; if it \
                                         fails again, stop and tell the lawyer.";

/// What follows a held paper's path in [`Refusal::HeldOpen`]'s reason.
const HELD_OPEN_TAIL: &str = " is open in Word or another program on this computer. A change \
                              made now would be lost when that program saves it. Ask the lawyer \
                              to close it, or write to a copy with another name.";

/// The reasons of the refusals other than [`Refusal::HeldOpen`], as [`Refusal::reason`] gives
/// them.
const FIXED_REASONS: [&str; 4] = [
    "Clauseo doesn't let the agent change the .codex folder in a matter: files there change \
     what the agent loads and runs.",
    "Clauseo doesn't let the agent touch Codex's own folder (CODEX_HOME): files there change \
     what the agent loads and runs. Run tools by name (for example rg), not by their path in \
     that folder.",
    "Clauseo doesn't let the agent run stetdoc mcp or stetdoc init in its shell: run the stetdoc \
     command you need directly.",
    "Clauseo can't read this command: it is nested too deep or is too long. Split it into \
     smaller commands.",
];

/// Whether `text`, a blocked step's result, is Claude Code's guard hook that could not answer:
/// a `[…]` on one line holding the hook's own command (its program, then [`FLAG`] as a
/// word), whose text after the closing `]: ` holds none of the guard's refusal reasons. Keyed
/// on the command Clauseo wrote, never on Claude Code's words around it. A matter folder whose
/// path holds `]: ` reads as could not answer (the step is blocked either way).
///
/// [`FLAG`]: super::FLAG
pub(crate) fn could_not_answer(text: &str) -> bool {
    let marker = format!(" {} ", super::FLAG);
    text.match_indices(&marker).any(|(at, _)| {
        let (before, after) = text.split_at(at);
        let line = before.rsplit('\n').next().unwrap_or_default();
        let Some(close) = after.find("]: ") else {
            return false;
        };
        let bracket = after.get(..close).unwrap_or_default();
        let rest = after.get(close + 3..).unwrap_or_default();
        line.contains('[') && !bracket.contains('\n') && !refusal_in(rest)
    })
}

/// Whether `text` holds one of the guard's refusal reasons.
fn refusal_in(text: &str) -> bool {
    text.contains(HELD_OPEN_TAIL) || FIXED_REASONS.iter().any(|reason| text.contains(reason))
}

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
}

impl Refusal {
    /// What the agent is told.
    pub(super) fn reason(&self) -> String {
        let [codex_folder, codex_home, stetdoc, unread] = FIXED_REASONS;
        match self {
            Self::HeldOpen(path) => format!("{path}{HELD_OPEN_TAIL}"),
            Self::CodexFolder => codex_folder.to_owned(),
            Self::CodexHome => codex_home.to_owned(),
            Self::StetdocServe => stetdoc.to_owned(),
            Self::Unread => unread.to_owned(),
        }
    }
}
