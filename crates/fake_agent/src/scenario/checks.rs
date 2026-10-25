//! Static checks run at load: labels, request ids, file paths, and SDK validation of every sent
//! message.

use std::collections::{HashMap, HashSet};

use agent_client_protocol::schema::v1 as acp;

use super::{Expect, ExpectResponse, FileOp, Loc, Reply, Step, StepKind, Wait};
use crate::Error;
use crate::pattern::{CWD, substitute};
use crate::validate::{Problem, Sent, validate};

/// The absolute `cwd` that `{{cwd}}` becomes while validating.
const VALIDATION_CWD: &str = "/fake_agent/cwd";

/// Whether the client sends `method` as a notification, so its `expect` takes no label.
pub(crate) fn is_client_notification(method: &str) -> bool {
    method == acp::AGENT_METHOD_NAMES.session_cancel
        || method == acp::PROTOCOL_LEVEL_METHOD_NAMES.cancel_request
}

#[derive(Default)]
struct Seen<'a> {
    /// Expect label to its method.
    labels: HashMap<&'a str, &'a str>,
    /// Labels a `respond` step has answered.
    answered: HashSet<&'a str>,
    /// Ids of the agent's `request` steps.
    requests: HashSet<&'a str>,
}

/// Runs every static check over the flattened steps, in order.
pub(super) fn check(steps: &[Step]) -> Result<(), Error> {
    let mut seen = Seen::default();
    for step in steps {
        let at = &step.at;
        match &step.kind {
            StepKind::Wait(Wait::Expect(expect)) => {
                check_expect(expect).map_err(|m| at.load_error(m))?;
                seen.label(expect, at)?;
            }
            StepKind::All(waits) => {
                for wait in waits {
                    match wait {
                        Wait::Expect(expect) => {
                            check_expect(expect).map_err(|m| at.load_error(m))?;
                        }
                        Wait::Response(response) => seen.response(response, at)?,
                    }
                }
                // Labels inside `all` count once the whole `all` step is done.
                for wait in waits {
                    if let Wait::Expect(expect) = wait {
                        seen.label(expect, at)?;
                    }
                }
            }
            StepKind::Wait(Wait::Response(response)) => seen.response(response, at)?,
            StepKind::Respond { to, reply } => seen.respond(step, to, reply)?,
            StepKind::Update { update, .. } => {
                if !step.raw {
                    let update = substitute(update, VALIDATION_CWD);
                    checked(
                        at,
                        acp::CLIENT_METHOD_NAMES.session_update,
                        &Sent::Update(&update),
                    )?;
                }
            }
            StepKind::Notify { method, params } => {
                if !step.raw {
                    let params = params.as_ref().map(|p| substitute(p, VALIDATION_CWD));
                    checked(
                        at,
                        method,
                        &Sent::Notify {
                            method,
                            params: params.as_ref(),
                        },
                    )?;
                }
            }
            StepKind::Request { id, method, params } => {
                if !seen.requests.insert(id) {
                    return Err(at.load_error(format!("request id `{id}` is used twice")));
                }
                if !step.raw {
                    let params = params.as_ref().map(|p| substitute(p, VALIDATION_CWD));
                    checked(
                        at,
                        method,
                        &Sent::Request {
                            method,
                            params: params.as_ref(),
                        },
                    )?;
                }
            }
            StepKind::File(FileOp::Write { path, .. } | FileOp::Delete { path }) => {
                relative(path, SESSION_CWD).map_err(|m| at.load_error(m))?;
            }
            StepKind::File(FileOp::Move { from, to }) => {
                relative(from, SESSION_CWD).map_err(|m| at.load_error(m))?;
                relative(to, SESSION_CWD).map_err(|m| at.load_error(m))?;
            }
            StepKind::SpawnChild { pid_file } => {
                relative(pid_file, PROCESS_FOLDER).map_err(|m| at.load_error(m))?;
            }
            StepKind::SendLine(text) => {
                if text.contains(CWD) {
                    return Err(at.load_error(format!(
                        "send_line: `{CWD}` is not allowed, since the line is written as raw text \
                         and a Windows cwd's backslashes would make it invalid JSON; use notify, \
                         update or request, which substitute it into parsed JSON"
                    )));
                }
            }
            StepKind::Stderr { .. } | StepKind::Exit { .. } => {}
        }
    }
    Ok(())
}

impl<'a> Seen<'a> {
    fn label(&mut self, expect: &'a Expect, at: &Loc) -> Result<(), Error> {
        if let Some(label) = &expect.label
            && self.labels.insert(label, &expect.method).is_some()
        {
            return Err(at.load_error(format!("label `{label}` is used twice")));
        }
        Ok(())
    }

    fn response(&self, response: &ExpectResponse, at: &Loc) -> Result<(), Error> {
        if self.requests.contains(response.id.as_str()) {
            Ok(())
        } else {
            Err(at.load_error(format!(
                "expect_response.id `{}` names no earlier request step",
                response.id
            )))
        }
    }

    fn respond(&mut self, step: &Step, to: &'a str, reply: &Reply) -> Result<(), Error> {
        let at = &step.at;
        let Some(&method) = self.labels.get(to) else {
            return Err(at.load_error(format!("respond.to `{to}` names no earlier expect label")));
        };
        if !self.answered.insert(to) {
            return Err(at.load_error(format!("respond.to `{to}`: already answered")));
        }
        if step.raw {
            return Ok(());
        }
        match reply {
            Reply::Result(result) => {
                let result = substitute(result, VALIDATION_CWD);
                checked(
                    at,
                    method,
                    &Sent::Result {
                        method,
                        result: &result,
                    },
                )
            }
            Reply::Error(error) => {
                checked(at, method, &Sent::Error(&substitute(error, VALIDATION_CWD)))
            }
        }
    }
}

fn check_expect(expect: &Expect) -> Result<(), String> {
    let method = &expect.method;
    match (&expect.label, is_client_notification(method)) {
        (None, false) => Err(format!("expect `{method}`: a request needs an `as` label")),
        (Some(_), true) => Err(format!(
            "expect `{method}`: a notification takes no `as` label"
        )),
        _ => Ok(()),
    }
}

/// Validates one sent message and turns a failure into the load error it is.
fn checked(at: &Loc, method: &str, sent: &Sent<'_>) -> Result<(), Error> {
    match validate(sent) {
        Ok(()) => Ok(()),
        Err(Problem::UnknownMethod) => Err(at.load_error(format!(
            "`{method}` has no type in the SDK's stable schema; mark the step \"raw\": true"
        ))),
        Err(Problem::Invalid(message)) => Err(Error::Validate {
            file: at.file.clone(),
            line: at.line,
            method: method.to_owned(),
            message: at.with_origin(message),
        }),
    }
}

/// What file steps' paths are relative to.
const SESSION_CWD: &str = "the session cwd";
/// What `spawn_child`'s `pid_file` is relative to.
const PROCESS_FOLDER: &str = "the process folder (the agent's working directory)";

/// A file step's path must be relative to `base` and must not climb out with `..`. Both
/// separators count, so a Windows path is refused on every system.
pub(crate) fn relative(path: &str, base: &str) -> Result<(), String> {
    let absolute = path.starts_with(['/', '\\'])
        || path.as_bytes().get(1) == Some(&b':')
        || std::path::Path::new(path).is_absolute();
    if path.is_empty() {
        Err("the path is empty".to_owned())
    } else if absolute {
        Err(format!(
            "path `{path}` is absolute; it must be relative to {base}"
        ))
    } else if path.split(['/', '\\']).any(|part| part == "..") {
        Err(format!(
            "path `{path}` has a `..` component; it must stay under {base}"
        ))
    } else {
        Ok(())
    }
}
