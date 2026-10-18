//! Steps as data. `plan` lists what runs and in what order, and `run_all` is the one loop that runs it.
//! Adding an external step is one `ToolStep` in `plan.rs`.

mod plan;
mod run;

pub(crate) use plan::plan;
pub(crate) use run::{StepResult, StepStatus, run_all};

use crate::checks::StaticCheck;
use crate::process::Cmd;
use crate::violation::Finding;
use crate::workspace::Context;

/// Which packages a cargo step covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every member of the workspace.
    Workspace,
    /// These members. Never empty: build it with `Scope::packages`.
    Packages(Vec<String>),
    /// No members. Steps that need packages are skipped.
    Nothing,
}

impl Scope {
    /// `Packages(names)`, or `Nothing` when `names` is empty. An empty list must never reach cargo,
    /// where it would mean every member.
    pub(crate) fn packages(names: Vec<String>) -> Scope {
        if names.is_empty() {
            Scope::Nothing
        } else {
            Scope::Packages(names)
        }
    }

    /// The cargo selector arguments, or `None` for `Nothing`.
    fn selectors(&self) -> Option<Vec<String>> {
        match self {
            Scope::Workspace => Some(vec!["--workspace".into()]),
            Scope::Packages(names) => Some(
                names
                    .iter()
                    .flat_map(|name| ["-p".to_owned(), name.clone()])
                    .collect(),
            ),
            Scope::Nothing => None,
        }
    }
}

/// An external tool run as `cmd`.
pub(crate) struct ToolStep {
    pub name: &'static str,
    /// Exactly what runs, and what the header prints.
    pub cmd: Cmd,
    /// The `file:line` findings in the output. Returns none when the output has no locations.
    pub parse: fn(&str, &Context, &Cmd) -> Vec<Finding>,
    /// The fix line for a failure with no `file:line`, or for a program that cannot start.
    /// `None` means the program could not start; `Some` is the captured output.
    pub fix: fn(&Cmd, Option<&str>) -> String,
}

pub(crate) enum Step {
    Tool(ToolStep),
    Static(&'static StaticCheck),
    Skipped {
        name: &'static str,
        reason: &'static str,
    },
}
