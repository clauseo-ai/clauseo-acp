//! Shape validation for commands, shared by every surface that takes commands (`desktop`,
//! `transport_ws`, `cli`), the [`ValidatedCommand`] it makes, and the limits it checks. State
//! checks are the host's.

use std::io;

use super::{CommandEnvelope, HostCommand, MatterSource};
use crate::title::shows_something;
#[cfg(doc)]
use crate::{AgentChoiceId, DeliveryRequest, check_display_title};
use crate::{
    CommandId, Delivery, ElicitationResponse, Error, FileChoice, FileDecision, MatterPath,
    PromptBlock, RawPayload, RestoreRequest, Title, is_windows_reserved_name,
};

/// Most prompt text, in UTF-8 bytes summed over the prompt's `Text` blocks (256 KiB). Raw bytes,
/// not JSON-escaped ones: the lawyer typed them, and the limit guards the agent's input.
pub const MAX_PROMPT_TEXT_BYTES: usize = 256 * 1024;

/// Most `File` blocks in one prompt.
pub const MAX_PROMPT_FILES: usize = 50;

/// Most blocks of any kind in one prompt.
pub const MAX_PROMPT_BLOCKS: usize = 100;

/// Most UTF-8 bytes in a new matter folder's name: a folder name's limit on common file systems.
pub const MAX_MATTER_NAME_BYTES: usize = 255;

/// Most elicitation values, as the length of their compact JSON text (64 KiB), counted without
/// building the text.
pub const MAX_ELICITATION_VALUES_BYTES: usize = 64 * 1024;

/// Most files named in an `UndoStep` or a `RestoreVersion` files scope. Far above what one step
/// or one restore touches in a legal matter, so it only stops a client sending an unbounded list.
pub const MAX_FILE_LIST: usize = 10_000;

/// Most bytes of paths in one such file list (1 MiB), summed over its `MatterPath`s, so a list
/// within [`MAX_FILE_LIST`] of long paths cannot make a 40 MB command.
pub const MAX_PATH_LIST_BYTES: usize = 1024 * 1024;

/// A command whose shape passed [`CommandEnvelope::validate`], the only way to build one: its
/// fields are private and it has no other constructor, so a command built in Rust goes through
/// the same check as one decoded off the wire, and the host takes only this type.
///
/// ```compile_fail,E0451
/// use clauseo_protocol::{CommandEnvelope, ValidatedCommand};
///
/// fn forge(envelope: CommandEnvelope) -> ValidatedCommand {
///     ValidatedCommand { command_id: envelope.command_id, command: envelope.command }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedCommand {
    command_id: CommandId,
    command: HostCommand,
}

impl ValidatedCommand {
    /// The client's id for the command.
    pub fn command_id(&self) -> CommandId {
        self.command_id
    }

    /// The command.
    pub fn command(&self) -> &HostCommand {
        &self.command
    }

    /// The envelope it came from, for a receipt that stores the command as sent.
    pub fn into_envelope(self) -> CommandEnvelope {
        CommandEnvelope {
            command_id: self.command_id,
            command: self.command,
        }
    }
}

impl CommandEnvelope {
    /// Checks the command's shape rules decoding cannot (counts, sums, rules across fields),
    /// never state, and returns the [`ValidatedCommand`] the host takes. Each rule is written
    /// once, on what it bounds: `SendPrompt`'s `content` (also for `EditQueuedPrompt`),
    /// [`MatterSource::NewFolder`] for a new folder's name, `AnswerElicitation`'s `response`,
    /// `UndoStep`'s `files` and `RestoreVersion`'s `scope`, and [`MAX_FILE_LIST`] and
    /// [`MAX_PATH_LIST_BYTES`] for both file lists, in which no path is named twice. Titles
    /// ([`check_display_title`]), agent-chosen ids ([`AgentChoiceId`]), paths ([`MatterPath`]),
    /// the delivery ([`DeliveryRequest`]) and ids were checked when they were decoded or built.
    ///
    /// # Errors
    ///
    /// [`Error::TooLarge`] for a size over its limit, [`Error::Format`] for every other rule.
    /// A caller reports either with
    /// [`HostError::invalid_command`](crate::HostError::invalid_command).
    pub fn validate(self) -> Result<ValidatedCommand, Error> {
        self.command.validate()?;
        Ok(ValidatedCommand {
            command_id: self.command_id,
            command: self.command,
        })
    }
}

impl HostCommand {
    /// How the prompt is delivered, as the host logs it (a missing `delivery` is
    /// [`Delivery::Queue`]). `None` for every command but `SendPrompt`.
    pub fn delivery(&self) -> Option<Delivery> {
        if let Self::SendPrompt { delivery, .. } = self {
            Some(Delivery::from(*delivery))
        } else {
            None
        }
    }

    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::CreateMatter { name, source } => match source {
                MatterSource::NewFolder { folder: _ } => check_folder_name(name),
                MatterSource::ExistingFolder { folder: _ } => Ok(()),
            },
            Self::SendPrompt {
                thread: _,
                content,
                delivery: _,
            } => check_prompt("SendPrompt", content),
            Self::EditQueuedPrompt {
                thread: _,
                turn: _,
                content,
            } => check_prompt("EditQueuedPrompt", content),
            Self::AnswerElicitation {
                thread: _,
                elicitation: _,
                response,
            } => check_response(response),
            Self::UndoStep {
                thread: _,
                turn: _,
                files,
            } => {
                // An undo undoes at least one file: a list of only `Keep` choices (or none)
                // is refused alike, since the files it leaves out are kept anyway.
                if !files
                    .iter()
                    .any(|FileDecision { path: _, choice }| *choice == FileChoice::Undo)
                {
                    return Err(Error::Format {
                        kind: "UndoStep",
                        rule: "an undo undoes at least one file",
                    });
                }
                check_paths(
                    "UndoStep",
                    files.iter().map(|FileDecision { path, choice: _ }| path),
                )
            }
            Self::RestoreVersion {
                matter: _,
                checkpoint: _,
                scope,
            } => match scope {
                RestoreRequest::Matter => Ok(()),
                RestoreRequest::Files { paths } if paths.is_empty() => Err(Error::Format {
                    kind: "RestoreVersion",
                    rule: "a files scope names at least one file",
                }),
                RestoreRequest::Files { paths } => check_paths("RestoreVersion", paths.iter()),
            },
            Self::CreateThread {
                matter: _,
                agent: _,
            }
            | Self::RemoveQueuedPrompt { thread: _, turn: _ }
            | Self::SendQueuedNow { thread: _, turn: _ }
            | Self::ResumeQueue { thread: _ }
            | Self::Cancel { thread: _ }
            | Self::RestartAgent { thread: _ }
            | Self::ArchiveThread { thread: _ }
            | Self::AnswerPermission {
                thread: _,
                request: _,
                option_id: _,
            }
            | Self::SetConfigOption {
                thread: _,
                option_id: _,
                value: _,
            }
            | Self::SetMode {
                thread: _,
                mode_id: _,
            }
            | Self::RenameThread {
                thread: _,
                title: _,
            } => Ok(()),
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
                visible_text = visible_text || shows_something(text);
            }
            PromptBlock::File { file: _ } => files += 1,
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
            rule: "a prompt holds a visible character of text or a file",
        });
    }
    Ok(())
}

fn check_folder_name(name: &Title) -> Result<(), Error> {
    let name = name.as_str();
    let refuse = |rule| {
        Err(Error::Format {
            kind: "CreateMatter",
            rule,
        })
    };
    if name.len() > MAX_MATTER_NAME_BYTES {
        return Err(too_large(
            "CreateMatter",
            "new folder name in UTF-8 bytes",
            name.len(),
            MAX_MATTER_NAME_BYTES,
        ));
    }
    if name == "." || name == ".." {
        return refuse("a new folder name is not `.` or `..`");
    }
    if name.starts_with('.') {
        return refuse("a new folder name does not start with a dot, which hides it in Finder");
    }
    if name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']) {
        return refuse("a new folder name holds none of < > : \" / \\ | ? *");
    }
    if name.ends_with(['.', ' ']) || name.starts_with(' ') {
        return refuse(
            "a new folder name does not end with a dot or a space, or start with a space",
        );
    }
    if is_windows_reserved_name(name) {
        return refuse("a new folder name is not a Windows device name such as CON or COM1");
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
    if !string_keys_only(values) {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values have a JSON form (string keys only)",
        });
    }
    let Some(size) = json_len(values) else {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values nest at most MAX_RAW_ENCODE_DEPTH lists and maps",
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

/// Whether every map key in `value`, at any depth, is a string. A JSON copy writes a scalar or
/// binary key as a string, but a command is strict input, so its values must be JSON as sent.
/// The walk keeps its own stack, so a value built by hand however deep cannot overflow it.
fn string_keys_only(value: &RawPayload) -> bool {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        if let RawPayload::Seq(items) = value {
            pending.extend(items.iter());
        } else if let RawPayload::Map(entries) = value {
            for (key, value) in entries {
                if !matches!(key, RawPayload::Str(_)) {
                    return false;
                }
                pending.push(value);
            }
        }
    }
    true
}

/// The length of `value`'s compact JSON text, written into a counter, never into a buffer;
/// `None` when it nests deeper than [`MAX_RAW_ENCODE_DEPTH`](crate::MAX_RAW_ENCODE_DEPTH),
/// which the logged answer could not hold.
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
    let bytes = sorted
        .iter()
        .fold(0usize, |sum, path| sum.saturating_add(path.len()));
    if bytes > MAX_PATH_LIST_BYTES {
        return Err(too_large(
            kind,
            "bytes of paths named",
            bytes,
            MAX_PATH_LIST_BYTES,
        ));
    }
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
