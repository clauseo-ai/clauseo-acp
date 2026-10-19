//! Shape validation for commands: lengths, counts and formats, shared by every surface that
//! takes commands (`desktop`, `transport_ws`, `cli`). State checks are the host's.
//!
//! Limits (doc 16 §3.2, plus bounds chosen here for ids and lists):
//! - prompt text ([`MAX_PROMPT_TEXT_BYTES`]): the UTF-8 bytes of every `Text` block, summed;
//! - file references per prompt ([`MAX_PROMPT_FILES`]);
//! - thread titles and matter names ([`MAX_TITLE_CHARS`]), counted in `char`s;
//! - elicitation values ([`MAX_ELICITATION_VALUES_BYTES`]): their compact JSON length;
//! - agent-chosen ids (`option_id`, `mode_id`, a select value) ([`MAX_ID_TEXT_BYTES`]);
//! - files named in an undo or a restore ([`MAX_FILE_LIST`]).

use std::io;

use super::{CommandEnvelope, ConfigValue, HostCommand};
use crate::{
    Delivery, ElicitationResponse, Error, FileDecision, MatterPath, PromptBlock, RawPayload,
    RestoreScope,
};

/// Most prompt text, in UTF-8 bytes summed over the prompt's `Text` blocks (256 KiB). Raw bytes,
/// not JSON-escaped ones: the lawyer typed them, and the limit guards the agent's input.
pub const MAX_PROMPT_TEXT_BYTES: usize = 256 * 1024;

/// Most `File` blocks in one prompt.
pub const MAX_PROMPT_FILES: usize = 50;

/// Most blocks of any kind in one prompt.
const MAX_PROMPT_BLOCKS: usize = 100;

/// Most UTF-8 bytes in a matter name: a folder name's limit on common file systems.
const MAX_MATTER_NAME_BYTES: usize = 255;

/// Most characters (`char`s, Unicode scalar values) in a thread title or a matter name.
pub const MAX_TITLE_CHARS: usize = 200;

/// Most elicitation values, as the length of their compact JSON text (64 KiB), counted without
/// building the text.
pub const MAX_ELICITATION_VALUES_BYTES: usize = 64 * 1024;

/// Most UTF-8 bytes in an agent-chosen id a command echoes: `option_id`, `mode_id` and a select
/// value. ACP's ids are short words; 256 leaves room without letting a client send megabytes.
pub const MAX_ID_TEXT_BYTES: usize = 256;

/// Most files named in an `UndoStep` or a `RestoreVersion` scope. Far above what one step or one
/// restore touches in a legal matter, so it only stops a client sending an unbounded list.
pub const MAX_FILE_LIST: usize = 10_000;

impl CommandEnvelope {
    /// Checks the command's shape; see [`HostCommand::validate`].
    ///
    /// # Errors
    ///
    /// As [`HostCommand::validate`].
    pub fn validate(&self) -> Result<(), Error> {
        self.command.validate()
    }
}

impl HostCommand {
    /// How the prompt is delivered: `delivery`, or [`Delivery::Queue`] when it is missing.
    /// `None` for every command but `SendPrompt`.
    pub fn delivery(&self) -> Option<Delivery> {
        match self {
            Self::SendPrompt { delivery, .. } => Some(delivery.unwrap_or(Delivery::Queue)),
            Self::CreateMatter { .. }
            | Self::CreateThread { .. }
            | Self::EditQueuedPrompt { .. }
            | Self::RemoveQueuedPrompt { .. }
            | Self::SendQueuedNow { .. }
            | Self::ResumeQueue { .. }
            | Self::Cancel { .. }
            | Self::RestartAgent { .. }
            | Self::AnswerPermission { .. }
            | Self::AnswerElicitation { .. }
            | Self::SetConfigOption { .. }
            | Self::SetMode { .. }
            | Self::RenameThread { .. }
            | Self::ArchiveThread { .. }
            | Self::UndoStep { .. }
            | Self::RestoreVersion { .. } => None,
        }
    }
    /// Checks lengths, counts and formats, never state. Ids and paths were already checked when
    /// they were decoded or built.
    ///
    /// - `SendPrompt` and `EditQueuedPrompt`: 1 to 100 blocks, no empty text block, at least
    ///   one non-whitespace character of text or one file, at most [`MAX_PROMPT_TEXT_BYTES`] of
    ///   text and [`MAX_PROMPT_FILES`] file references; a `Delivery::Unknown` (a newer client's
    ///   mode) is refused, never run as another mode.
    /// - Titles and names (`RenameThread.title`, `CreateMatter.name`): at most
    ///   [`MAX_TITLE_CHARS`] characters, at least one visible character (not only whitespace or
    ///   U+200B to U+200D, U+2060, U+FEFF), no control character and no bidirectional control
    ///   (U+202A to U+202E, U+2066 to U+2069).
    /// - `CreateMatter.name` also works as a folder name on every system: at most 255 bytes, not
    ///   `.` or `..`, none of `< > : " / \ | ? *`, no trailing dot or space, no leading space,
    ///   not a Windows device name (CON, PRN, AUX, NUL, COM1 to COM9, LPT1 to LPT9) with or
    ///   without an extension, in any case.
    /// - `option_id`, `mode_id` and a select value: 1 to [`MAX_ID_TEXT_BYTES`] bytes.
    /// - `AnswerElicitation` with `Accept`: the values are a JSON object of at most
    ///   [`MAX_ELICITATION_VALUES_BYTES`] as compact JSON.
    /// - `UndoStep.files` and `RestoreScope::Files`: at most [`MAX_FILE_LIST`] paths, no path
    ///   twice; a `Files` scope names at least one.
    ///
    /// # Errors
    ///
    /// [`Error::TooLarge`] for a size over its limit, [`Error::Format`] for every other rule.
    /// Both map to `command.invalid`.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::CreateMatter { name, .. } => {
                check_title("CreateMatter", name)?;
                check_folder_name(name)
            }
            Self::SendPrompt {
                content, delivery, ..
            } => {
                check_prompt("SendPrompt", content)?;
                match delivery {
                    Some(Delivery::Unknown) => Err(Error::Format {
                        kind: "SendPrompt",
                        rule: "delivery is from a newer Clauseo; it is refused, not guessed",
                    }),
                    Some(Delivery::Queue | Delivery::SendNow | Delivery::Steer) | None => Ok(()),
                }
            }
            Self::EditQueuedPrompt { content, .. } => check_prompt("EditQueuedPrompt", content),
            Self::AnswerPermission { option_id, .. } => {
                check_id("AnswerPermission", "option_id", option_id)
            }
            Self::AnswerElicitation { response, .. } => check_response(response),
            Self::SetConfigOption {
                option_id, value, ..
            } => {
                check_id("SetConfigOption", "option_id", option_id)?;
                match value {
                    ConfigValue::Select(choice) => check_id("SetConfigOption", "value", choice),
                    ConfigValue::Boolean(_) => Ok(()),
                }
            }
            Self::SetMode { mode_id, .. } => check_id("SetMode", "mode_id", mode_id),
            Self::RenameThread { title, .. } => check_title("RenameThread", title),
            Self::UndoStep { files, .. } => check_paths(
                "UndoStep",
                files.iter().map(|file: &FileDecision| &file.path),
            ),
            Self::RestoreVersion { scope, .. } => match scope {
                RestoreScope::Matter => Ok(()),
                RestoreScope::Files(paths) if paths.is_empty() => Err(Error::Format {
                    kind: "RestoreVersion",
                    rule: "a files scope names at least one file",
                }),
                RestoreScope::Files(paths) => check_paths("RestoreVersion", paths.iter()),
            },
            Self::CreateThread { .. }
            | Self::RemoveQueuedPrompt { .. }
            | Self::SendQueuedNow { .. }
            | Self::ResumeQueue { .. }
            | Self::Cancel { .. }
            | Self::RestartAgent { .. }
            | Self::ArchiveThread { .. } => Ok(()),
        }
    }
}

fn too_large(kind: &'static str, what: &'static str, size: usize, max: usize) -> Error {
    Error::TooLarge {
        kind,
        what,
        size,
        max,
    }
}

fn check_prompt(kind: &'static str, content: &[PromptBlock]) -> Result<(), Error> {
    if content.is_empty() {
        return Err(Error::Format {
            kind,
            rule: "a prompt has at least one block",
        });
    }
    if content.len() > MAX_PROMPT_BLOCKS {
        return Err(too_large(
            kind,
            "prompt blocks",
            content.len(),
            MAX_PROMPT_BLOCKS,
        ));
    }
    let mut text_bytes = 0usize;
    let mut files = 0usize;
    let mut visible_text = false;
    for block in content {
        match block {
            PromptBlock::Text { text } if text.is_empty() => {
                return Err(Error::Format {
                    kind,
                    rule: "a text block is not empty",
                });
            }
            PromptBlock::Text { text } => {
                text_bytes = text_bytes.saturating_add(text.len());
                visible_text = visible_text || !text.trim().is_empty();
            }
            PromptBlock::File { .. } => files += 1,
        }
    }
    if text_bytes > MAX_PROMPT_TEXT_BYTES {
        return Err(too_large(
            kind,
            "prompt text in UTF-8 bytes",
            text_bytes,
            MAX_PROMPT_TEXT_BYTES,
        ));
    }
    if files > MAX_PROMPT_FILES {
        return Err(too_large(kind, "file references", files, MAX_PROMPT_FILES));
    }
    if !visible_text && files == 0 {
        return Err(Error::Format {
            kind,
            rule: "a prompt holds a non-whitespace character of text or a file",
        });
    }
    Ok(())
}

/// Bidirectional embedding, override and isolate controls, which can make a title read as
/// something else (U+202A to U+202E, U+2066 to U+2069).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Whitespace, or a zero-width character (U+200B to U+200D, U+2060, U+FEFF).
fn is_invisible(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

fn check_title(kind: &'static str, title: &str) -> Result<(), Error> {
    if title.chars().all(is_invisible) {
        return Err(Error::Format {
            kind,
            rule: "a title or name has a visible character",
        });
    }
    let chars = title.chars().count();
    if chars > MAX_TITLE_CHARS {
        return Err(too_large(
            kind,
            "title or name in characters",
            chars,
            MAX_TITLE_CHARS,
        ));
    }
    if title.chars().any(|c| c.is_control() || is_bidi_control(c)) {
        return Err(Error::Format {
            kind,
            rule: "a title or name holds no control or bidirectional control character",
        });
    }
    Ok(())
}

/// Windows device names, refused as a folder name with or without an extension.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A matter name may become a folder name (for `NewFolder`), so it is checked as one on every
/// system for every source: the same name rules whichever way the matter was made.
fn check_folder_name(name: &str) -> Result<(), Error> {
    let refuse = |rule| {
        Err(Error::Format {
            kind: "CreateMatter",
            rule,
        })
    };
    if name.len() > MAX_MATTER_NAME_BYTES {
        return Err(too_large(
            "CreateMatter",
            "matter name in UTF-8 bytes",
            name.len(),
            MAX_MATTER_NAME_BYTES,
        ));
    }
    if name == "." || name == ".." {
        return refuse("a matter name is not `.` or `..`");
    }
    if name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']) {
        return refuse("a matter name holds none of < > : \" / \\ | ? *");
    }
    if name.ends_with(['.', ' ']) || name.starts_with(' ') {
        return refuse("a matter name does not end with a dot or a space, or start with a space");
    }
    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return refuse("a matter name is not a Windows device name such as CON or COM1");
    }
    Ok(())
}

fn check_id(kind: &'static str, what: &'static str, id: &str) -> Result<(), Error> {
    if id.is_empty() {
        return Err(Error::Format {
            kind,
            rule: "an option, mode or value id is not empty",
        });
    }
    if id.len() > MAX_ID_TEXT_BYTES {
        return Err(too_large(kind, what, id.len(), MAX_ID_TEXT_BYTES));
    }
    Ok(())
}

fn check_response(response: &ElicitationResponse) -> Result<(), Error> {
    let values = match response {
        ElicitationResponse::Accept { values } => values,
        ElicitationResponse::Decline | ElicitationResponse::Cancel => return Ok(()),
    };
    if !matches!(values, RawPayload::Map(_)) {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values are a JSON object",
        });
    }
    let Some(size) = json_len(values) else {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values have a JSON form (string keys only)",
        });
    };
    if size > MAX_ELICITATION_VALUES_BYTES {
        return Err(too_large(
            "AnswerElicitation",
            "values as compact JSON in bytes",
            size,
            MAX_ELICITATION_VALUES_BYTES,
        ));
    }
    Ok(())
}

/// The length of `value`'s compact JSON text, written into a counter, never into a buffer;
/// `None` when it has no JSON form (a non-string map key, possible only from `MessagePack`).
fn json_len(value: &RawPayload) -> Option<usize> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

/// An `io::Write` that only counts.
struct ByteCounter(usize);

impl io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn check_paths<'a>(
    kind: &'static str,
    paths: impl ExactSizeIterator<Item = &'a MatterPath>,
) -> Result<(), Error> {
    if paths.len() > MAX_FILE_LIST {
        return Err(too_large(kind, "files named", paths.len(), MAX_FILE_LIST));
    }
    let mut sorted: Vec<&str> = paths.map(MatterPath::as_str).collect();
    sorted.sort_unstable();
    if sorted
        .windows(2)
        .any(|pair| matches!(pair, [a, b] if a == b))
    {
        return Err(Error::Format {
            kind,
            rule: "no file is named twice",
        });
    }
    Ok(())
}
