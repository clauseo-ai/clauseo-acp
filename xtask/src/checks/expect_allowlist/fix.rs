//! The fix lines of the expect-allowlist check, one variant per kind of failure.

use std::fmt;

/// An attribute that lowers a lint, which only `expect` may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// `allow`: silences the lint.
    Allow,
    /// `warn`: turns a denied lint into a warning, which a local run passes.
    Warn,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Allow => "allow",
            Level::Warn => "warn",
        })
    }
}

/// What the reader does about one failure of the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// An `expect` site with no line of its own in the allowlist.
    Unlisted {
        path: String,
        item: String,
        lint: String,
    },
    /// An allowlist line, at `line`, with no site of its own.
    Stale { line: u32 },
    /// An `allow` or `warn` of `lint`; `inner` for `#![..]`.
    Lower {
        level: Level,
        path: String,
        item: String,
        lint: String,
        inner: bool,
    },
    /// An `allow` or `warn` that names no lint.
    EmptyLower { level: Level },
    /// An attribute in a macro body whose name or lint is a metavariable.
    MetavariableAttribute,
    /// A `#` in a macro body with no brackets after it.
    HashWithoutBrackets,
    /// A `#[path]` that does not name a `.rs` file the check reads.
    Path,
    /// An `include!` of anything but a `.rs` file the check reads.
    Include,
    /// An allowlist line that is not three fields.
    Malformed,
    /// A `.rs` file syn cannot parse.
    Unparsable,
    /// The allowlist itself is missing.
    MissingAllowlist,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = super::ALLOWLIST;
        match self {
            Fix::Unlisted { path, item, lint } => write!(
                f,
                "remove the #[expect], or add \"{path} {item} {lint}\" to {list} (the code owner \
                 approves it)"
            ),
            Fix::Stale { line } => write!(f, "delete line {line} of {list}"),
            Fix::Lower {
                level,
                path,
                item,
                lint,
                inner,
            } => {
                let bang = if *inner { "!" } else { "" };
                write!(
                    f,
                    "delete the {level}, or write #{bang}[expect({lint}, reason = \"...\")] in its \
                     place and add \"{path} {item} {lint}\" to {list} (the code owner approves it)"
                )
            }
            Fix::EmptyLower { level } => write!(f, "delete the {level}: it names no lint"),
            Fix::MetavariableAttribute => f.write_str(
                "write the attribute's name and each of its lints in the macro body itself, with \
                 no metavariable, so expect-allowlist can read them; a caller cannot pass in an \
                 attribute or a lint",
            ),
            Fix::HashWithoutBrackets => f.write_str(
                "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`, \
                 with its name and lints in the macro body itself",
            ),
            Fix::Path => f.write_str(
                "give #[path] a string literal naming a .rs file that git lists, relative to \
                 this file's folder, on a module at the top level of the file, or move the \
                 module to its default path; expect-allowlist reads only those files",
            ),
            Fix::Include => f.write_str(
                "move the code into a module, or include! only a .rs file that git lists, named \
                 by a string literal relative to this file's folder; expect-allowlist reads only \
                 those files",
            ),
            Fix::Malformed => write!(
                f,
                "write the line as \"<path> <item> <lint>\", three fields separated by spaces, \
                 as the comment at the top of {list} says, or delete it"
            ),
            Fix::Unparsable => write!(
                f,
                "make the file valid Rust that syn can parse: expect-allowlist reads every .rs \
                 file in the repo for its expect, allow and warn attributes"
            ),
            Fix::MissingAllowlist => write!(
                f,
                "restore {list}, one \"<path> <item> <lint>\" line per #[expect] site; the check \
                 matches the sites against it only once it is there"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
