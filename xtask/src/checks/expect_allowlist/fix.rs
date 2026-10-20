//! The fix lines of the expect-allowlist check, one variant per kind of failure.

use std::fmt;

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
    /// An `allow` of `lint`; `inner` for `#![allow]`.
    Allow {
        path: String,
        item: String,
        lint: String,
        inner: bool,
    },
    /// An `allow` that names no lint.
    EmptyAllow,
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
            Fix::Allow {
                path,
                item,
                lint,
                inner,
            } => {
                let bang = if *inner { "!" } else { "" };
                write!(
                    f,
                    "delete the allow, or write #{bang}[expect({lint}, reason = \"...\")] in its \
                     place and add \"{path} {item} {lint}\" to {list} (the code owner approves it)"
                )
            }
            Fix::EmptyAllow => write!(f, "delete the allow: it names no lint"),
            Fix::Malformed => write!(
                f,
                "write the line as \"<path> <item> <lint>\", three fields separated by spaces, \
                 as the comment at the top of {list} says, or delete it"
            ),
            Fix::Unparsable => write!(
                f,
                "make the file valid Rust that syn can parse: expect-allowlist reads every .rs \
                 file in the repo for its #[expect] and #[allow] sites"
            ),
            Fix::MissingAllowlist => write!(
                f,
                "restore {list}, one \"<path> <item> <lint>\" line per #[expect] site; the check \
                 matches the sites against it only once it is there"
            ),
        }
    }
}
