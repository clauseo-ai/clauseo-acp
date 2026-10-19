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

/// What the agent does on stdin EOF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnStdinClose {
    /// Exit 0.
    Exit,
    /// Write `fake_agent: stdin closed, holding` to stderr and park forever.
    Hang,
}

/// A file name and 1-based line, for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Loc {
    pub(crate) file: String,
    pub(crate) line: usize,
}

impl Loc {
    pub(crate) fn load_error(&self, message: impl Into<String>) -> Error {
        Error::Load {
            file: self.file.clone(),
            line: self.line,
            message: message.into(),
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

/// What a step does. See the crate docs for the table.
#[derive(Debug, Clone)]
pub(crate) enum StepKind {
    Expect(Expect),
    ExpectResponse(ExpectResponse),
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
    WriteFile {
        path: String,
        content: String,
    },
    DeleteFile {
        path: String,
    },
    MoveFile {
        from: String,
        to: String,
    },
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
    pub fn parse(name: &str, text: &str) -> Result<Self, Error> {
        let name = name.strip_suffix(".jsonl").unwrap_or(name);
        parse(name, text, &Includes::Bundled)
    }

    /// Reads, parses and validates the scenario file at `path`. `include` steps read `common/`
    /// in the file's folder.
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
    let mut lines = numbered(&file, text);
    let Some((header_at, header)) = lines.next().transpose()? else {
        return Err(Loc {
            file: file.clone(),
            line: 1,
        }
        .load_error("empty file; line 1 must be the header"));
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
            body::Parsed::Include(include) => {
                steps.extend(included(&include, includes).map_err(|m| at.load_error(m))??);
            }
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

/// The steps of included file `include`. The outer error is about the `include` step itself; the
/// inner one points into the included file.
fn included(include: &str, includes: &Includes<'_>) -> Result<Result<Vec<Step>, Error>, String> {
    if include.contains(['/', '\\'])
        || Path::new(include)
            .extension()
            .is_none_or(|ext| ext != "jsonl")
    {
        return Err(format!(
            "include `{include}` must be a .jsonl file name in scenarios/common/"
        ));
    }
    let text = match includes {
        Includes::Bundled => match bundled::common(include) {
            Some(text) => text.to_owned(),
            None => {
                return Err(format!(
                    "include `{include}`: no such file in scenarios/common/"
                ));
            }
        },
        #[cfg(test)]
        Includes::Memory(files) => match files.iter().find(|(name, _)| *name == include) {
            Some((_, text)) => (*text).to_owned(),
            None => {
                return Err(format!(
                    "include `{include}`: no such file in scenarios/common/"
                ));
            }
        },
        Includes::Folder(folder) => {
            let path = folder.join("common").join(include);
            if !path.is_file() {
                return Err(format!(
                    "include `{include}`: no such file {}",
                    path.display()
                ));
            }
            match read(&path) {
                Ok(text) => text,
                Err(error) => return Ok(Err(error)),
            }
        }
    };
    let mut steps = Vec::new();
    for line in numbered(include, &text) {
        let (at, value) = match line {
            Ok(line) => line,
            Err(error) => return Ok(Err(error)),
        };
        match body::step(value) {
            Ok(body::Body {
                kind: body::Parsed::Step(kind),
                raw,
            }) => {
                steps.push(Step { at, kind, raw });
            }
            Ok(body::Body {
                kind: body::Parsed::Include(_),
                ..
            }) => {
                return Ok(Err(at.load_error("an included file may not include")));
            }
            Err(message) => return Ok(Err(at.load_error(message))),
        }
    }
    Ok(Ok(steps))
}

/// The non-blank lines of `text` with their location, each parsed as JSON.
fn numbered<'a>(
    file: &'a str,
    text: &'a str,
) -> impl Iterator<Item = Result<(Loc, Value), Error>> + 'a {
    text.lines()
        .enumerate()
        .filter(|(_, raw)| !raw.trim().is_empty())
        .map(move |(index, raw)| {
            let at = Loc {
                file: file.to_owned(),
                line: index + 1,
            };
            match serde_json::from_str(raw) {
                Ok(value) => Ok((at, value)),
                Err(error) => Err(at.load_error(format!("not JSON: {error}"))),
            }
        })
}

#[cfg(test)]
mod tests;
