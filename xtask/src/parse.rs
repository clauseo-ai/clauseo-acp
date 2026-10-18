//! Parsers for the text that rustfmt and clippy print.

/// Repo-relative, `/`-separated form of a path a tool printed.
/// `root` is the workspace root as the tool printed it (absolute, or `C:\...` on Windows).
pub fn rel_path(raw: &str, root: &str) -> String {
    let path = raw.trim().replace('\\', "/");
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let rel = path
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(path.as_str());
    rel.strip_prefix("./").unwrap_or(rel).to_string()
}

/// `(path, line)` for each file `cargo fmt --check` would change. Accepts the current
/// `Diff in <path>:<line>:` header and the older `Diff in <path> at line <line>:` header.
///
/// The header's line is where the hunk's context starts, not the changed line. The reported
/// line is the first removed line of the hunk (the header line when the hunk only adds lines).
pub fn fmt_diffs(output: &str, root: &str) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("Diff in ") else {
            continue;
        };
        let parsed = match rest.rsplit_once(" at line ") {
            Some((path, number)) => number
                .trim_end_matches(':')
                .trim()
                .parse::<u32>()
                .ok()
                .map(|n| (path, n)),
            None => rest
                .strip_suffix(':')
                .unwrap_or(rest)
                .rsplit_once(':')
                .and_then(|(path, number)| number.parse::<u32>().ok().map(|n| (path, n))),
        };
        let Some((path, start)) = parsed else {
            continue;
        };
        let body: Vec<&str> = lines[index + 1..]
            .iter()
            .take_while(|l| !l.starts_with("Diff in "))
            .copied()
            .collect();
        let entry = (rel_path(path, root), hunk_changed_line(start, &body));
        if !found.contains(&entry) {
            found.push(entry);
        }
    }
    found
}

/// Line in the original file of the first removed line of a rustfmt hunk.
/// Context lines (` `) and removed lines (`-`) both advance the original line number.
fn hunk_changed_line(start: u32, body: &[&str]) -> u32 {
    let mut original = start;
    for line in body {
        if line.starts_with('-') {
            return original;
        }
        if line.starts_with(' ') || line.is_empty() {
            original += 1;
        }
    }
    start
}

/// One clippy diagnostic from `--message-format=short`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub path: String,
    pub line: u32,
    /// Includes the level, e.g. `warning: unused variable: x` or `error[E0425]: cannot find value`.
    pub message: String,
    pub is_error: bool,
}

/// Every `path:line:col: warning|error...` diagnostic in `output`, deduplicated by
/// (path, line, message). Lines without a file prefix are ignored.
pub fn clippy_diags(output: &str, root: &str) -> Vec<Diag> {
    let mut diags: Vec<Diag> = Vec::new();
    for line in output.lines() {
        let Some(diag) = clippy_line(line, root) else {
            continue;
        };
        let duplicate = diags
            .iter()
            .any(|d| d.path == diag.path && d.line == diag.line && d.message == diag.message);
        if !duplicate {
            diags.push(diag);
        }
    }
    diags
}

fn clippy_line(line: &str, root: &str) -> Option<Diag> {
    let at = [": warning", ": error"]
        .iter()
        .filter_map(|marker| line.find(*marker))
        .min()?;
    let is_error = line[at..].starts_with(": error");
    let mut parts = line[..at].rsplitn(3, ':');
    let column = parts.next()?;
    let row = parts.next()?;
    let path = parts.next()?;
    let line_no: u32 = row.parse().ok()?;
    column.parse::<u32>().ok()?;
    if path.is_empty() {
        return None;
    }
    Some(Diag {
        path: rel_path(path, root),
        line: line_no,
        message: line[at + 2..].to_string(),
        is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_path_strips_root_and_normalises_separators() {
        assert_eq!(
            rel_path("/ws/xtask/src/main.rs", "/ws"),
            "xtask/src/main.rs"
        );
        assert_eq!(rel_path("./xtask/src/main.rs", "/ws"), "xtask/src/main.rs");
        assert_eq!(
            rel_path("C:\\ws\\xtask\\src\\main.rs", "C:\\ws"),
            "xtask/src/main.rs"
        );
    }

    #[test]
    fn fmt_parses_current_header() {
        let out = "Diff in /ws/xtask/src/main.rs:3:\n-fn  bad( ){}\n+fn bad() {}\n";
        assert_eq!(
            fmt_diffs(out, "/ws"),
            vec![("xtask/src/main.rs".to_string(), 3)]
        );
    }

    #[test]
    fn fmt_reports_the_changed_line_not_the_hunk_context_start() {
        // Real `cargo fmt --check` output for `fn  bad( ){}` appended at line 40.
        let out = "Diff in /ws/xtask/src/main.rs:37:\n         Commands::Setup => tools::setup(),\n     }\n }\n-fn  bad( ){}\n+fn bad() {}\n\n";
        assert_eq!(
            fmt_diffs(out, "/ws"),
            vec![("xtask/src/main.rs".to_string(), 40)]
        );
    }

    #[test]
    fn fmt_parses_old_header_and_dedupes() {
        let out = "Diff in /ws/xtask/src/main.rs at line 3:\n\
                   Diff in /ws/xtask/src/main.rs at line 3:\n\
                   Diff in /ws/xtask/src/lib.rs at line 9:\n";
        assert_eq!(
            fmt_diffs(out, "/ws"),
            vec![
                ("xtask/src/main.rs".to_string(), 3),
                ("xtask/src/lib.rs".to_string(), 9)
            ]
        );
    }

    #[test]
    fn fmt_parses_windows_path() {
        let out = "Diff in C:\\ws\\xtask\\src\\main.rs:7:\n";
        assert_eq!(
            fmt_diffs(out, "C:\\ws"),
            vec![("xtask/src/main.rs".to_string(), 7)]
        );
    }

    #[test]
    fn fmt_finds_nothing_in_a_syntax_error() {
        let out = "error: expected one of `!` or `::`, found `(`\n --> xtask/src/main.rs:1:4\n";
        assert!(fmt_diffs(out, "/ws").is_empty());
    }

    #[test]
    fn clippy_parses_warning_and_error_lines() {
        let out = "xtask/src/main.rs:4:9: warning: unused variable: `x`\n\
                   xtask/src/main.rs:6:5: error[E0425]: cannot find value `y` in this scope\n";
        let diags = clippy_diags(out, "/ws");
        assert_eq!(
            diags,
            vec![
                Diag {
                    path: "xtask/src/main.rs".into(),
                    line: 4,
                    message: "warning: unused variable: `x`".into(),
                    is_error: false,
                },
                Diag {
                    path: "xtask/src/main.rs".into(),
                    line: 6,
                    message: "error[E0425]: cannot find value `y` in this scope".into(),
                    is_error: true,
                },
            ]
        );
    }

    #[test]
    fn clippy_parses_windows_drive_paths() {
        let out = "C:\\ws\\xtask\\src\\main.rs:4:9: error: unused variable: `unused`\n";
        let diags = clippy_diags(out, "C:\\ws");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].path, "xtask/src/main.rs");
        assert_eq!(diags[0].line, 4);
        assert!(diags[0].is_error);
    }

    #[test]
    fn clippy_ignores_lines_without_a_file_prefix_and_dedupes() {
        let out = "warning: `xtask` (bin \"xtask\") generated 1 warning\n\
                   error: could not compile `xtask` (bin \"xtask\") due to 1 previous error\n\
                   xtask/src/main.rs:4:9: warning: unused variable: `x`\n\
                   xtask/src/main.rs:4:9: warning: unused variable: `x`\n";
        let diags = clippy_diags(out, "/ws");
        assert_eq!(diags.len(), 1);
        assert!(!diags[0].is_error);
    }
}
