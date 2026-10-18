use std::fmt;

/// One problem a check found. Displays as `path:line: message` and an indented `fix:` line.
/// Whole-file problems use line 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Violation {
    pub path: String,
    pub line: u32,
    pub message: String,
    pub fix: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}\n    fix: {}",
            self.path, self.line, self.message, self.fix
        )
    }
}

/// A problem a tool reported. An error fails the step; a warning is printed and counted.
pub(crate) struct Finding {
    pub violation: Violation,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_path_line_message_and_indented_fix() {
        let v = Violation {
            path: "crates/a/src/lib.rs".into(),
            line: 12,
            message: "rustfmt would reformat this code".into(),
            fix: "run `cargo fmt --all`".into(),
        };
        assert_eq!(
            v.to_string(),
            "crates/a/src/lib.rs:12: rustfmt would reformat this code\n    fix: run `cargo fmt --all`"
        );
    }
}
