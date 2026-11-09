//! The fix lines of the runner-stderr check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the runner-stderr check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A `eprintln!`, `eprint!`, `println!`, `print!` or `dbg!` call.
    Console,
    /// A `write!` or `writeln!` whose target mentions `stderr`.
    WriteStderr,
    /// A call of a path ending in `stderr`, such as `io::stderr()`.
    StderrCall,
    /// `io::stderr` named but not called (passed or stored), or imported under a bare name.
    StderrValue,
    /// A `use crate::` in the runner of a name outside the helper modules.
    OutsideHelpers,
    /// An error's Display or Debug, or its `to_string()`, formatted outside `Detail::new`.
    ErrorText,
    /// A `use` that renames `Detail` or `stderr`.
    Alias,
    /// A `macro_rules!` whose body calls a console macro or names `stderr`.
    Wrapper,
    /// A `.rs` file syn cannot parse.
    Unparsable,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Console => write!(
                f,
                "say the line through runner::stderr with stderr::refuse or stderr::notice: the \
                 host copies the runner's stderr into the matter's event log, so only the API's \
                 fixed sentences may go to it"
            ),
            Fix::WriteStderr => write!(
                f,
                "say the line through runner::stderr with stderr::refuse or stderr::notice rather \
                 than writing to a stderr handle: runner-stderr fails write! and writeln! whose \
                 target is stderr"
            ),
            Fix::StderrCall => write!(
                f,
                "take the stream from runner::stderr (stderr::refuse, stderr::notice or \
                 stderr::agent_stream) instead of opening stderr: runner-stderr fails each call \
                 that opens it"
            ),
            Fix::StderrValue => write!(
                f,
                "take the stream from runner::stderr (stderr::refuse, stderr::notice or \
                 stderr::agent_stream) instead of naming io::stderr: runner-stderr fails \
                 io::stderr passed, stored or imported, not only called"
            ),
            Fix::OutsideHelpers => write!(
                f,
                "import from crate::windows, crate::denial or crate::detect (or a name lib.rs \
                 re-exports from one), or add the module to runner-stderr's helpers: it reads only \
                 those for stderr writes"
            ),
            Fix::ErrorText => write!(
                f,
                "format the error inside a Detail::new(|| ...) argument, which only the \
                 vm-harness-detail builds call: error text can hold a path, and the host copies \
                 the runner's stderr into the event log"
            ),
            Fix::Alias => write!(
                f,
                "import Detail and stderr under their own names: runner-stderr finds them by name \
                 and can't follow a rename"
            ),
            Fix::Wrapper => write!(
                f,
                "write the console or stderr call where it is needed, not in a macro_rules! body: \
                 runner-stderr reads the calls it can see and can't follow one a macro builds"
            ),
            Fix::Unparsable => write!(
                f,
                "make the file valid Rust: runner-stderr parses it to find its stderr and format \
                 calls"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
