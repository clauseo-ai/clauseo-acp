//! Plays a scenario over stdin and stdout, one step at a time, waiting only for client lines.

mod client;
mod effects;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use agent_client_protocol::schema::v1 as acp;
use serde_json::{Map, Value, json};

use crate::scenario::{Loc, OnStdinClose, Reply, Scenario, StepKind, Wait};
use client::{Incoming, Miss, NO_CWD, classify, describe_wait, match_wait, placed};

/// Why the script stopped before reaching its end.
pub(crate) enum Stop {
    /// An `exit` step.
    Exit(i32),
    /// Stdin reached EOF.
    StdinClosed,
    /// A mismatch or a failed step: exit 3 with this message.
    Fail(Loc, String),
}

/// The client's request an `expect` label names.
struct Label {
    id: Value,
    method: String,
}

/// The state of one run.
struct Runner<'s, R, W> {
    scenario: &'s Scenario,
    input: R,
    out: W,
    cwd: Option<String>,
    session: Option<String>,
    labels: HashMap<String, Label>,
}

/// Runs `scenario` on the real stdin, stdout and stderr and returns the exit code. Never returns
/// when stdin closes under `on_stdin_close: "hang"`.
pub(crate) fn run(scenario: &Scenario) -> i32 {
    let stdin = std::io::stdin().lock();
    let stdout = std::io::BufWriter::new(std::io::stdout().lock());
    let mut runner = Runner {
        scenario,
        input: stdin,
        out: stdout,
        cwd: None,
        session: None,
        labels: HashMap::new(),
    };
    match runner.play() {
        Stop::Exit(code) => code,
        Stop::StdinClosed => match scenario.on_stdin_close {
            OnStdinClose::Exit => 0,
            OnStdinClose::Hang => {
                effects::write_stderr("fake_agent: stdin closed, holding\n").unwrap_or_default();
                crate::park_forever()
            }
        },
        Stop::Fail(at, message) => {
            let line = format!("fake_agent: {}:{}: {message}\n", at.file, at.line);
            effects::write_stderr(&line).unwrap_or_default();
            3
        }
    }
}

impl<R: BufRead, W: Write> Runner<'_, R, W> {
    /// Plays every step, then reads until EOF; any client message after the last step is a
    /// mismatch.
    fn play(&mut self) -> Stop {
        let scenario = self.scenario;
        for step in &scenario.steps {
            if let Err(stop) = self.step(&step.at, &step.kind) {
                return stop;
            }
        }
        match self.read(&scenario.end) {
            Ok(incoming) => Stop::Fail(
                scenario.end.clone(),
                format!("unexpected {} after the last step", incoming.describe()),
            ),
            Err(stop) => stop,
        }
    }

    fn step(&mut self, at: &Loc, kind: &StepKind) -> Result<(), Stop> {
        match kind {
            StepKind::Expect(expect) => {
                let wait = Wait::Expect(expect.clone());
                let incoming = self.read(at)?;
                match_wait(&wait, &incoming, self.cwd.as_deref())
                    .map_err(|miss| Stop::Fail(at.clone(), miss.into_message()))?;
                self.received(&wait, incoming);
            }
            StepKind::ExpectResponse(response) => {
                let wait = Wait::Response(response.clone());
                let incoming = self.read(at)?;
                match_wait(&wait, &incoming, self.cwd.as_deref())
                    .map_err(|miss| Stop::Fail(at.clone(), miss.into_message()))?;
            }
            StepKind::All(waits) => self.all(at, waits)?,
            StepKind::Respond { to, reply } => self.respond(at, to, reply)?,
            StepKind::Update { update, session } => {
                let session = session
                    .clone()
                    .or_else(|| self.session.clone())
                    .ok_or_else(|| {
                        Stop::Fail(at.clone(), "no current session for this update".to_owned())
                    })?;
                let update = self.placed(at, update)?;
                let params = json!({"sessionId": session, "update": update});
                self.send(
                    at,
                    &message(
                        None,
                        Some(acp::CLIENT_METHOD_NAMES.session_update),
                        Some(params),
                    ),
                )?;
            }
            StepKind::Notify { method, params } => {
                let params = params.as_ref().map(|p| self.placed(at, p)).transpose()?;
                self.send(at, &message(None, Some(method), params))?;
            }
            StepKind::Request { id, method, params } => {
                let params = params.as_ref().map(|p| self.placed(at, p)).transpose()?;
                self.send(at, &message(Some(json!(id)), Some(method), params))?;
            }
            StepKind::SendLine(text) => {
                let text = match &self.cwd {
                    Some(cwd) => text.replace(crate::pattern::CWD, cwd),
                    None if text.contains(crate::pattern::CWD) => {
                        return Err(Stop::Fail(at.clone(), NO_CWD.to_owned()));
                    }
                    None => text.clone(),
                };
                self.write_line(at, &text)?;
            }
            StepKind::WriteFile { .. }
            | StepKind::DeleteFile { .. }
            | StepKind::MoveFile { .. } => {
                let cwd = self
                    .cwd
                    .as_deref()
                    .ok_or_else(|| Stop::Fail(at.clone(), NO_CWD.to_owned()))?;
                effects::file_step(cwd, kind).map_err(|m| Stop::Fail(at.clone(), m))?;
            }
            StepKind::Stderr { line, repeat } => effects::stderr_step(line, *repeat)
                .map_err(|e| Stop::Fail(at.clone(), format!("write to stderr failed: {e}")))?,
            StepKind::SpawnChild { pid_file } => {
                effects::spawn_child(pid_file).map_err(|m| Stop::Fail(at.clone(), m))?;
            }
            StepKind::Exit { code } => {
                self.out
                    .flush()
                    .map_err(|e| Stop::Fail(at.clone(), format!("write to stdout failed: {e}")))?;
                return Err(Stop::Exit(*code));
            }
        }
        Ok(())
    }

    /// Reads until every message in `waits` has arrived, in any order.
    fn all(&mut self, at: &Loc, waits: &[Wait]) -> Result<(), Stop> {
        let mut pending: Vec<&Wait> = waits.iter().collect();
        while !pending.is_empty() {
            let incoming = self.read(at)?;
            let mut content_miss = None;
            let mut found = None;
            for (index, wait) in pending.iter().enumerate() {
                match match_wait(wait, &incoming, self.cwd.as_deref()) {
                    Ok(()) => {
                        found = Some(index);
                        break;
                    }
                    Err(Miss::Content(message)) => content_miss = content_miss.or(Some(message)),
                    Err(Miss::Identity(_)) => {}
                }
            }
            let Some(index) = found else {
                let message = content_miss.unwrap_or_else(|| {
                    let wanted: Vec<String> = pending.iter().map(|w| describe_wait(w)).collect();
                    format!(
                        "expected one of: {}; got {}",
                        wanted.join(", "),
                        incoming.describe()
                    )
                });
                return Err(Stop::Fail(at.clone(), message));
            };
            let wait = pending.remove(index);
            self.received(wait, incoming);
        }
        Ok(())
    }

    /// Records what a matched client line changes: its label, and the current cwd and session.
    fn received(&mut self, wait: &Wait, incoming: Incoming) {
        let (Wait::Expect(expect), Incoming::Request { id, method, params }) = (wait, incoming)
        else {
            return;
        };
        let names = acp::AGENT_METHOD_NAMES;
        if method == names.session_new || method == names.session_load {
            let field = |key: &str| {
                params
                    .as_ref()
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_str)
            };
            if let Some(cwd) = field("cwd") {
                self.cwd = Some(cwd.to_owned());
            }
            if method == names.session_load {
                self.session = field("sessionId").map(str::to_owned);
            }
        }
        if let Some(label) = &expect.label {
            self.labels.insert(label.clone(), Label { id, method });
        }
    }

    fn respond(&mut self, at: &Loc, to: &str, reply: &Reply) -> Result<(), Stop> {
        let Some(label) = self.labels.get(to) else {
            return Err(Stop::Fail(
                at.clone(),
                format!("respond.to `{to}`: no such request received"),
            ));
        };
        let mut body = Map::new();
        body.insert("jsonrpc".to_owned(), json!("2.0"));
        body.insert("id".to_owned(), label.id.clone());
        let new_session = label.method == acp::AGENT_METHOD_NAMES.session_new;
        match reply {
            Reply::Result(result) => {
                let result = self.placed(at, result)?;
                if let Some(id) = result.get("sessionId").and_then(Value::as_str)
                    && new_session
                {
                    self.session = Some(id.to_owned());
                }
                body.insert("result".to_owned(), result);
            }
            Reply::Error(error) => {
                body.insert("error".to_owned(), self.placed(at, error)?);
            }
        }
        self.send(at, &Value::Object(body))
    }

    fn placed(&self, at: &Loc, value: &Value) -> Result<Value, Stop> {
        placed(value, self.cwd.as_deref()).map_err(|m| Stop::Fail(at.clone(), m))
    }

    /// Reads and classifies the next client line. EOF is [`Stop::StdinClosed`].
    fn read(&mut self, at: &Loc) -> Result<Incoming, Stop> {
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) => Err(Stop::StdinClosed),
            Ok(_) => {
                // `read_line` stops at EOF too: text without `\n` is a line the client never
                // finished before closing stdin.
                let Some(line) = line.strip_suffix('\n') else {
                    return Err(Stop::Fail(
                        at.clone(),
                        "client line has no newline before stdin closed".to_owned(),
                    ));
                };
                let line = line.strip_suffix('\r').unwrap_or(line);
                classify(line).map_err(|m| Stop::Fail(at.clone(), m))
            }
            Err(error) => Err(Stop::Fail(
                at.clone(),
                format!("read from stdin failed: {error}"),
            )),
        }
    }

    fn send(&mut self, at: &Loc, value: &Value) -> Result<(), Stop> {
        self.write_line(at, &value.to_string())
    }

    /// Writes `text` and `\n` to stdout and flushes.
    fn write_line(&mut self, at: &Loc, text: &str) -> Result<(), Stop> {
        let written = self
            .out
            .write_all(text.as_bytes())
            .and_then(|()| self.out.write_all(b"\n"))
            .and_then(|()| self.out.flush());
        written.map_err(|e| Stop::Fail(at.clone(), format!("write to stdout failed: {e}")))
    }
}

/// A JSON-RPC 2.0 request (with `id`) or notification (without).
fn message(id: Option<Value>, method: Option<&str>, params: Option<Value>) -> Value {
    let mut body = Map::new();
    body.insert("jsonrpc".to_owned(), json!("2.0"));
    if let Some(id) = id {
        body.insert("id".to_owned(), id);
    }
    if let Some(method) = method {
        body.insert("method".to_owned(), json!(method));
    }
    if let Some(params) = params {
        body.insert("params".to_owned(), params);
    }
    Value::Object(body)
}
