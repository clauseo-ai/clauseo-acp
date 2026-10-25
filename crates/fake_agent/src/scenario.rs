//! The scenario model and its parser: the header, the steps, and `include`.

mod body;
mod checks;

use std::path::Path;

use serde_json::Value;

use crate::Error;
use crate::bundled;

/// A parsed and validated scenario, ready to run.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// The scenario name, equal to the file stem.
    name: String,
    pub(crate) on_stdin_close: OnStdinClose,
    pub(crate) steps: Vec<Step>,
    /// Where an error after the last step points: the last step, or the header.
    pub(crate) end: Loc,
}

/// What the agent does on stdin EOF: the header's `on_stdin_close`, `"exit"` when left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OnStdinClose {
    /// Exit [`crate::EXIT_OK`].
    #[default]
    Exit,
    /// Write `fake_agent: stdin closed, holding` to stderr and park forever.
    Hang,
}

/// A file name and 1-based line, for messages, and for a line of an included file, the
/// `include` step that brought it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Loc {
    pub(crate) file: String,
    pub(crate) line: usize,
    pub(crate) included_from: Option<Box<Loc>>,
}

impl Loc {
    /// The location of line `line` of `file`, not included from anywhere.
    pub(crate) fn new(file: &str, line: usize) -> Self {
        Self {
            file: file.to_owned(),
            line,
            included_from: None,
        }
    }

    pub(crate) fn load_error(&self, message: impl Into<String>) -> Error {
        Error::Load {
            file: self.file.clone(),
            line: self.line,
            message: self.with_origin(message),
        }
    }

    /// `message`, followed by `; included from <file>:<line>` for a line of an included file.
    pub(crate) fn with_origin(&self, message: impl Into<String>) -> String {
        let message = message.into();
        match &self.included_from {
            Some(from) => format!("{message}; included from {}:{}", from.file, from.line),
            None => message,
        }
    }
}

/// One step and where it was written. An included step keeps its own file and line.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub(crate) at: Loc,
    pub(crate) kind: StepKind,
    /// `"raw": true`: the message skips SDK validation.
    pub(crate) raw: bool,
}

/// What a step does. See the crate docs for the table. `body.rs` builds it from the step's one
/// key with an exhaustive match, so a new step key cannot be forgotten here.
#[derive(Debug, Clone)]
pub(crate) enum StepKind {
    /// `expect` or `expect_response`.
    Wait(Wait),
    All(Vec<Wait>),
    Respond {
        to: String,
        reply: Reply,
    },
    Update {
        update: Value,
        session: Option<String>,
    },
    Notify {
        method: String,
        params: Option<Value>,
    },
    Request {
        id: String,
        method: String,
        params: Option<Value>,
    },
    SendLine(String),
    File(FileOp),
    Stderr {
        line: String,
        repeat: u32,
    },
    SpawnChild {
        pid_file: String,
    },
    Exit {
        code: i32,
    },
}

/// A `write_file`, `delete_file` or `move_file` step: paths relative to the session `cwd`.
#[derive(Debug, Clone)]
pub(crate) enum FileOp {
    Write { path: String, content: String },
    Delete { path: String },
    Move { from: String, to: String },
}

/// A client request (with a label) or notification (without) the agent waits for.
#[derive(Debug, Clone)]
pub(crate) struct Expect {
    pub(crate) method: String,
    pub(crate) label: Option<String>,
    pub(crate) params: Option<Value>,
    pub(crate) off: Vec<String>,
}

/// The client's response to the agent's request `id`.
#[derive(Debug, Clone)]
pub(crate) struct ExpectResponse {
    pub(crate) id: String,
    pub(crate) reply: Reply,
}

/// One message an `all` step waits for.
#[derive(Debug, Clone)]
pub(crate) enum Wait {
    Expect(Expect),
    Response(ExpectResponse),
}

/// A JSON-RPC `result` or `error` value: sent by `respond`, a pattern in `expect_response`.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    Result(Value),
    Error(Value),
}

/// Where `include` steps find their files.
enum Includes<'a> {
    /// The compiled-in `scenarios/common/` files.
    Bundled,
    /// `common/` beside the scenario file on disk.
    Folder(&'a Path),
    /// In-memory files, for the parser's tests.
    #[cfg(test)]
    Memory(&'a [(&'a str, &'a str)]),
}

impl Scenario {
    /// Parses and validates scenario `name` (the file stem; a `.jsonl` suffix is dropped) from
    /// `text`. `include` steps read the bundled `scenarios/common/` files.
    ///
    /// # Errors
    ///
    /// [`Error::Load`] or [`Error::Validate`], naming the file and line, if `text` is not a valid
    /// scenario.
    pub fn parse(name: &str, text: &str) -> Result<Self, Error> {
        let name = name.strip_suffix(".jsonl").unwrap_or(name);
        parse(name, text, &Includes::Bundled)
    }

    /// Reads, parses and validates the scenario file at `path`. `include` steps read `common/`
    /// in the file's folder.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a file cannot be read, else as [`Scenario::parse`].
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = read(path)?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        parse(
            name.as_deref().unwrap_or_default(),
            &text,
            &Includes::Folder(folder),
        )
    }

    /// The scenario name, equal to its file stem.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn parse(name: &str, text: &str, includes: &Includes<'_>) -> Result<Scenario, Error> {
    let file = format!("{name}.jsonl");
    let mut lines = numbered(&file, text, None);
    let Some((header_at, header)) = lines.next().transpose()? else {
        return Err(Loc::new(&file, 1).load_error("empty file; line 1 must be the header"));
    };
    let on_stdin_close = body::header(name, header).map_err(|m| header_at.load_error(m))?;
    let mut steps = Vec::new();
    for line in lines {
        let (at, value) = line?;
        let parsed = body::step(value).map_err(|m| at.load_error(m))?;
        match parsed.kind {
            body::Parsed::Step(kind) => steps.push(Step {
                at,
                kind,
                raw: parsed.raw,
            }),
            body::Parsed::Include(include) => steps.extend(included(&include, &at, includes)?),
        }
    }
    let end = steps.last().map_or(header_at, |step| step.at.clone());
    checks::check(&steps)?;
    Ok(Scenario {
        name: name.to_owned(),
        on_stdin_close,
        steps,
        end,
    })
}

/// The steps of file `include`, named by the `include` step at `at`. Each step's location
/// points into the included file and remembers `at`.
fn included(include: &str, at: &Loc, includes: &Includes<'_>) -> Result<Vec<Step>, Error> {
    if include.contains(['/', '\\'])
        || Path::new(include)
            .extension()
            .is_none_or(|ext| ext != "jsonl")
    {
        return Err(at.load_error(format!(
            "include `{include}` must be a .jsonl file name in scenarios/common/"
        )));
    }
    let missing = || {
        at.load_error(format!(
            "include `{include}`: no such file in scenarios/common/"
        ))
    };
    let text = match includes {
        Includes::Bundled => bundled::common(include).ok_or_else(missing)?.to_owned(),
        #[cfg(test)]
        Includes::Memory(files) => files
            .iter()
            .find(|(name, _)| *name == include)
            .map(|(_, text)| (*text).to_owned())
            .ok_or_else(missing)?,
        Includes::Folder(folder) => {
            let path = folder.join("common").join(include);
            if !path.is_file() {
                return Err(at.load_error(format!(
                    "include `{include}`: no such file {}",
                    path.display()
                )));
            }
            read(&path)?
        }
    };
    let mut steps = Vec::new();
    for line in numbered(include, &text, Some(at)) {
        let (step_at, value) = line?;
        match body::step(value).map_err(|m| step_at.load_error(m))? {
            body::Body {
                kind: body::Parsed::Step(kind),
                raw,
            } => steps.push(Step {
                at: step_at,
                kind,
                raw,
            }),
            body::Body {
                kind: body::Parsed::Include(_),
                ..
            } => return Err(step_at.load_error("an included file may not include")),
        }
    }
    Ok(steps)
}

/// The non-blank lines of `text` with their location, each parsed as JSON. `included_from` is
/// the `include` step that names the file, if any.
fn numbered<'a>(
    file: &'a str,
    text: &'a str,
    included_from: Option<&'a Loc>,
) -> impl Iterator<Item = Result<(Loc, Value), Error>> + 'a {
    text.lines()
        .enumerate()
        .filter(|(_, raw)| !raw.trim().is_empty())
        .map(move |(index, raw)| {
            let at = Loc {
                included_from: included_from.cloned().map(Box::new),
                ..Loc::new(file, index + 1)
            };
            match serde_json::from_str(raw) {
                Ok(value) => Ok((at, value)),
                Err(error) => Err(at.load_error(format!("not JSON: {error}"))),
            }
        })
}

#[cfg(test)]
mod tests;
