//! The Windows command line from a program and its arguments, by std's rules (the ones
//! `std::process::Command` uses on Windows), so the child's C runtime and
//! `CommandLineToArgvW` read back exactly the arguments given.
//!
//! - The program is always quoted and may not hold a `"`.
//! - An argument is quoted when it is empty or holds a space or a tab.
//! - A `"` is written `\"`, and the backslashes before it are doubled; so are the backslashes
//!   before a closing quote.
//! - A NUL anywhere is refused: the command line ends at the first one.

use std::ffi::{OsStr, OsString};

use crate::error::Error;

/// `"`.
const QUOTE: u16 = b'"' as u16;
/// `\`.
const BACKSLASH: u16 = b'\\' as u16;
/// The separator between arguments.
const SPACE: u16 = b' ' as u16;
/// The other blank that splits arguments.
const TAB: u16 = b'\t' as u16;

/// The command line of `program` with `args`, in UTF-16 without its final NUL.
pub fn command_line(program: &OsStr, args: &[OsString]) -> Result<Vec<u16>, Error> {
    let mut line = Vec::new();
    let program = wide(program, 0)?;
    if program.contains(&QUOTE) {
        return Err(Error::Argument {
            index: 0,
            reason: "the program holds a double quote",
        });
    }
    append(&mut line, &program, true);
    for (i, arg) in args.iter().enumerate() {
        let arg = wide(arg, i + 1)?;
        line.push(SPACE);
        let quote = arg.is_empty() || arg.contains(&SPACE) || arg.contains(&TAB);
        append(&mut line, &arg, quote);
    }
    Ok(line)
}

/// `text` in UTF-16, refusing a NUL. Off Windows an `OsStr` has no UTF-16 form, so it must be
/// Unicode there (this path exists for the tests).
fn wide(text: &OsStr, index: usize) -> Result<Vec<u16>, Error> {
    #[cfg(windows)]
    let units: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(text).collect();
    #[cfg(not(windows))]
    let units: Vec<u16> = text
        .to_str()
        .ok_or(Error::Argument {
            index,
            reason: "not Unicode",
        })?
        .encode_utf16()
        .collect();
    if units.contains(&0) {
        return Err(Error::Argument {
            index,
            reason: "holds a NUL",
        });
    }
    Ok(units)
}

/// One argument onto the line, quoted when `quote`.
fn append(line: &mut Vec<u16>, arg: &[u16], quote: bool) {
    if quote {
        line.push(QUOTE);
    }
    let mut backslashes = 0_usize;
    for &unit in arg {
        if unit == BACKSLASH {
            backslashes += 1;
        } else {
            if unit == QUOTE {
                // The run of backslashes before a quote is doubled, plus one for the quote.
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(unit);
    }
    if quote {
        // The run before the closing quote is doubled, so it does not escape it.
        line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        line.push(QUOTE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(program: &str, args: &[&str]) -> String {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        String::from_utf16(&command_line(OsStr::new(program), &args).unwrap()).unwrap()
    }

    /// The arguments as the MSVC runtime (2008 and later) and `CommandLineToArgvW` split them,
    /// after the program.
    fn split(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        // Skip the program: from the first quote to the next one.
        let mut i = chars.iter().skip(1).position(|c| *c == '"').unwrap() + 2;
        let mut args = Vec::new();
        loop {
            while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i >= chars.len() {
                return args;
            }
            let mut arg = String::new();
            let mut quoted = false;
            while i < chars.len() && (quoted || (chars[i] != ' ' && chars[i] != '\t')) {
                let run = chars[i..].iter().take_while(|c| **c == '\\').count();
                if run > 0 && chars.get(i + run) == Some(&'"') {
                    arg.extend(std::iter::repeat_n('\\', run / 2));
                    i += run;
                    if run % 2 == 1 {
                        arg.push('"');
                        i += 1;
                    }
                } else if run > 0 {
                    arg.extend(std::iter::repeat_n('\\', run));
                    i += run;
                } else if chars[i] == '"' {
                    if quoted && chars.get(i + 1) == Some(&'"') {
                        arg.push('"');
                        i += 2;
                    } else {
                        quoted = !quoted;
                        i += 1;
                    }
                } else {
                    arg.push(chars[i]);
                    i += 1;
                }
            }
            args.push(arg);
        }
    }

    /// std's own cases (library/std/src/sys/process/windows/tests.rs, `test_make_command_line`).
    #[test]
    fn std_vectors_hold() {
        assert_eq!(
            line("prog", &["aaa", "bbb", "ccc"]),
            r#""prog" aaa bbb ccc"#
        );
        assert_eq!(
            line(r"C:\Program Files\blah\blah.exe", &["aaa"]),
            r#""C:\Program Files\blah\blah.exe" aaa"#
        );
        assert_eq!(
            line(r"C:\Program Files\test", &["aa\"bb"]),
            r#""C:\Program Files\test" aa\"bb"#
        );
        assert_eq!(line("echo", &["a b c"]), r#""echo" "a b c""#);
        assert_eq!(
            line("echo", &["\" \\\" \\", "\\"]),
            r#""echo" "\" \\\" \\" \"#
        );
        assert_eq!(
            line("\u{03c0}\u{042f}\u{97f3}", &[]),
            "\"\u{03c0}\u{042f}\u{97f3}\""
        );
    }

    #[test]
    fn an_empty_argument_is_kept() {
        assert_eq!(line("p", &["", "x"]), r#""p" "" x"#);
    }

    #[test]
    fn chrome_flags_pass_unquoted() {
        assert_eq!(
            line(
                r"C:\shell\chrome-headless-shell.exe",
                &["--headless", "--proxy-server=127.0.0.1:8080"]
            ),
            r#""C:\shell\chrome-headless-shell.exe" --headless --proxy-server=127.0.0.1:8080"#
        );
    }

    #[test]
    fn every_argument_reads_back_as_given() {
        let cases = [
            "plain",
            "",
            "two words",
            "tab\there",
            "quote\"inside",
            "\"",
            "\\",
            "\\\\",
            "ends\\",
            "ends with space\\",
            "C:\\Program Files\\x\\",
            "a\\\\\"b",
            "\\\"\\\"",
            "  ",
            "--user-data-dir=C:\\p q\\profile",
        ];
        let line = line("prog", &cases);
        assert_eq!(split(&line), cases);
    }

    #[test]
    fn a_quote_in_the_program_or_a_nul_is_refused() {
        assert_eq!(
            command_line(OsStr::new("a\"b"), &[]),
            Err(Error::Argument {
                index: 0,
                reason: "the program holds a double quote"
            })
        );
        assert_eq!(
            command_line(
                OsStr::new("p"),
                &[OsString::from("ok"), OsString::from("a\0b")]
            ),
            Err(Error::Argument {
                index: 2,
                reason: "holds a NUL"
            })
        );
    }
}
