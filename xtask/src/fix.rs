//! The `fix:` line of a violation or a failure, typed.
//!
//! Each check names its failure kinds in its own `Fix` enum, whose `Display` writes each text once,
//! and its `fix/tests.rs` pins every variant's text with `tests::fix_texts!`, which numbers the
//! arms of one exhaustive `match`. `tests::assert_texts` then fails until each arm has a sample,
//! so a new kind needs its arm and its sample. The modules that several checks share (the helpers
//! in `checks.rs`, `crates_table`, `graph`, `exceptions`) have an enum each too. This enum wraps
//! them all, plus `Tool`: the text of the steps, which build their fix lines from what a tool
//! printed.

use crate::budgets;
use crate::checks;
use std::fmt;

#[cfg(test)]
pub(crate) mod tests;

/// One fix line. `Display` prints the text, without the `fix: ` label. Each check's enum is
/// boxed, so a `Violation` stays small however many fields a variant carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// The helpers in `checks.rs` that several checks call.
    Checks(Box<checks::Fix>),
    Budgets(Box<budgets::Fix>),
    Capabilities(Box<checks::capabilities::Fix>),
    Plugins(Box<checks::plugins::Fix>),
    TauriConfig(Box<checks::tauri_config::Fix>),
    BuildtimeEdits(Box<checks::buildtime_edits::Fix>),
    Dependencies(Box<checks::dependencies::Fix>),
    CoreIo(Box<checks::core_io::Fix>),
    Docs(Box<checks::docs::Fix>),
    FileSize(Box<checks::file_size::Fix>),
    CratesTable(Box<checks::crates_table::Fix>),
    Graph(Box<checks::graph::Fix>),
    Exceptions(Box<checks::exceptions::Fix>),
    ExpectAllowlist(Box<checks::expect_allowlist::Fix>),
    /// A step's fix line, built from the command that ran and what the tool printed. Only
    /// `Fix::tool` makes one, and no check calls it: a planted case fails on it (see
    /// `planted::Planted::verdict`).
    Tool(ToolText),
    /// A made-up fix for the tests of the runner and the planted cases, which need a check of
    /// their own.
    #[cfg(test)]
    Fake(&'static str),
}

/// The text of a step's fix line. Its field is private, so `Fix::tool` is the only way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolText(String);

impl Fix {
    /// A fix line for a step or a tool finding. A check writes its own `Fix` enum instead.
    pub(crate) fn tool(text: impl Into<String>) -> Fix {
        Fix::Tool(ToolText(text.into()))
    }
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Checks(fix) => fix.fmt(f),
            Fix::Budgets(fix) => fix.fmt(f),
            Fix::Capabilities(fix) => fix.fmt(f),
            Fix::Plugins(fix) => fix.fmt(f),
            Fix::TauriConfig(fix) => fix.fmt(f),
            Fix::BuildtimeEdits(fix) => fix.fmt(f),
            Fix::Dependencies(fix) => fix.fmt(f),
            Fix::CoreIo(fix) => fix.fmt(f),
            Fix::Docs(fix) => fix.fmt(f),
            Fix::FileSize(fix) => fix.fmt(f),
            Fix::CratesTable(fix) => fix.fmt(f),
            Fix::Graph(fix) => fix.fmt(f),
            Fix::Exceptions(fix) => fix.fmt(f),
            Fix::ExpectAllowlist(fix) => fix.fmt(f),
            Fix::Tool(ToolText(text)) => f.write_str(text),
            #[cfg(test)]
            Fix::Fake(text) => f.write_str(text),
        }
    }
}

/// `From` for each wrapped enum, so a check passes its own `Fix` where a `Fix` is taken.
macro_rules! wraps {
    ($($variant:ident($inner:ty)),+ $(,)?) => {
        $(
            impl From<$inner> for Fix {
                fn from(fix: $inner) -> Fix {
                    Fix::$variant(Box::new(fix))
                }
            }
        )+
    };
}

wraps! {
    Checks(checks::Fix),
    Budgets(budgets::Fix),
    Capabilities(checks::capabilities::Fix),
    Plugins(checks::plugins::Fix),
    TauriConfig(checks::tauri_config::Fix),
    BuildtimeEdits(checks::buildtime_edits::Fix),
    Dependencies(checks::dependencies::Fix),
    CoreIo(checks::core_io::Fix),
    Docs(checks::docs::Fix),
    FileSize(checks::file_size::Fix),
    CratesTable(checks::crates_table::Fix),
    Graph(checks::graph::Fix),
    Exceptions(checks::exceptions::Fix),
    ExpectAllowlist(checks::expect_allowlist::Fix),
}
