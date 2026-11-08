//! The Event Log's words for one of the setup exe's `tracing` lines (`event_trace.rs`): a fixed
//! description and codes, never a path, a SID or any other text the line's fields carry. Pure,
//! so the rule is tested on every OS.
//!
//! The description is the line's message, the fixed words its call site wrote; one that names
//! a path or an account anyway (a message built with `format!`-style captures) is withheld. The
//! codes are where it was logged (the module below `windows::` and the line, which say which
//! warning it is) and every whole-number field (an exit code, a Win32 error); a text field, an
//! error's words or a `Display` of a path, never goes.

use std::fmt::Write as _;

/// What stands for a message that names a path or an account.
pub(super) const WITHHELD: &str = "a set-up warning (its words named a path or an account)";

/// The line for a message `message`, logged in the module `module` at `line`, with the
/// whole-number fields `numbers`.
pub(super) fn event_line(
    message: &str,
    module: &str,
    line: Option<u32>,
    numbers: &[(&str, i128)],
) -> String {
    let description = if names_something(message) {
        WITHHELD
    } else {
        message
    };
    let place = module
        .rsplit_once("::windows::")
        .map_or(module, |(_crate, below)| below);
    let mut text = format!("clauseo-sandbox-setup: {description} [{place}");
    if let Some(line) = line {
        let _infallible = write!(text, ":{line}");
    }
    text.push(']');
    for (name, value) in numbers {
        let _infallible = write!(text, " {name}={value}");
    }
    text
}

/// A line the set-up task's server writes to the Event Log itself (`serve.rs`, some ending in an
/// error's words), held to the same rule as the lines above: itself when it names no path or
/// account, [`WITHHELD`] otherwise.
pub(super) fn fixed_words(line: &str) -> &str {
    if names_something(line) {
        WITHHELD
    } else {
        line
    }
}

/// Whether `message` looks like it names a path (a drive, a backslash, a slash, a UNC or
/// `\\?\` prefix) or an account (a SID's `S-1-`).
fn names_something(message: &str) -> bool {
    message.contains(['\\', '/']) || message.contains("S-1-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_holds_its_fixed_words_its_place_and_its_numbers_never_a_path() {
        let module = "clauseo_sandbox::windows::setup::late_steps";
        assert_eq!(
            event_line("the change journal wasn't enlarged", module, Some(292), &[]),
            "clauseo-sandbox-setup: the change journal wasn't enlarged [setup::late_steps:292]"
        );
        assert_eq!(
            event_line("a step failed", module, None, &[("code", 5), ("exit", -1)]),
            "clauseo-sandbox-setup: a step failed [setup::late_steps] code=5 exit=-1"
        );
        for named in [
            r"couldn't grow C:\",
            r"no folder C:\Users\rohan\AppData",
            r"\\?\Volume{0}",
            r"\\server\share",
            "/home/rohan",
            "S-1-5-21-1-2-3-1001 isn't a pool user",
        ] {
            let line = event_line(named, module, Some(1), &[]);
            assert!(line.contains(WITHHELD), "{line}");
            assert!(
                !line.contains("rohan") && !line.contains("S-1-5-21"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_serve_set_up_line_keeps_its_words_unless_its_error_names_a_path_or_an_account() {
        let plain = "--serve-set-up: the pipe: Access is denied. (os error 5)";
        assert_eq!(fixed_words(plain), plain);
        for named in [
            r"--serve-set-up: this process: C:\Users\rohan\x.exe isn't the admin copy",
            "--serve-set-up: the record: S-1-5-21-1-2-3-1001 isn't a pool user",
        ] {
            assert_eq!(fixed_words(named), WITHHELD, "{named}");
        }
    }
}
