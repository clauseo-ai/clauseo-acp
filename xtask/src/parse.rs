//! Parsers for the text that rustfmt and clippy print, and the findings they become.

use crate::process::Cmd;
use crate::violation::{Finding, Violation};
use crate::workspace::{Context, Member, package_for};

/// Repo-relative, `/`-separated form of a path a tool printed.
/// `root` is the workspace root as the tool printed it (absolute, or `C:\...` on Windows).
pub(crate) fn rel_path(raw: &str, root: &str) -> String {
    let path = raw.trim().replace('\\', "/");
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let rel = path
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(path.as_str());
    rel.strip_prefix("./").unwrap_or(rel).to_owned()
}

/// `(path, line)` for each file `cargo fmt --check` would change. Accepts the current
/// `Diff in <path>:<line>:` header and the older `Diff in <path> at line <line>:` header.
///
/// The header's line is where the hunk's context starts, not the changed line. The reported
/// line is the first removed or added line of the hunk (see `hunk_changed_line`).
pub(crate) fn fmt_diffs(output: &str, root: &str) -> Vec<(String, u32)> {
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

/// Line in the original file of the first changed line of a rustfmt hunk: the first removed line,
/// or, for a hunk that only adds lines, the line the first added line goes in front of.
/// Context lines (` `) and removed lines (`-`) both advance the original line number.
fn hunk_changed_line(start: u32, body: &[&str]) -> u32 {
    let mut original = start;
    for line in body {
        if line.starts_with('-') || line.starts_with('+') {
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
pub(crate) struct Diag {
    pub path: String,
    pub line: u32,
    /// Includes the level, e.g. `warning: unused variable: x` or `error[E0425]: cannot find value`.
    pub message: String,
    pub is_error: bool,
}

/// Every `path:line:col: warning|error...` diagnostic in `output`, deduplicated by
/// (path, line, message). Lines without a file prefix are ignored.
pub(crate) fn clippy_diags(output: &str, root: &str) -> Vec<Diag> {
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

/// Findings for `cargo fmt --check` output: one per file it would change, fixed by `cargo fmt --all`.
pub(crate) fn fmt_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    fmt_diffs(output, &root_str(ctx))
        .into_iter()
        .map(|(path, line)| Finding {
            violation: Violation {
                path,
                line,
                message: "rustfmt would reformat this code".into(),
                fix: "run `cargo fmt --all`".into(),
            },
            is_error: true,
        })
        .collect()
}

/// Findings for clippy's `--message-format=short` output. Each fix line is the command that shows
/// the full diagnostic, built from the args that ran (see `clippy_repro`).
pub(crate) fn clippy_findings(output: &str, ctx: &Context, cmd: &Cmd) -> Vec<Finding> {
    clippy_diags(output, &root_str(ctx))
        .into_iter()
        .map(|diag| {
            let fix = clippy_fix(cmd, &diag.path, &ctx.members);
            Finding {
                violation: Violation {
                    path: diag.path,
                    line: diag.line,
                    message: diag.message,
                    fix,
                },
                is_error: diag.is_error,
            }
        })
        .collect()
}

fn root_str(ctx: &Context) -> String {
    ctx.root.to_string_lossy().into_owned()
}

/// The fix line for a clippy finding in `path`: the command that shows the full diagnostic.
fn clippy_fix(cmd: &Cmd, path: &str, members: &[Member]) -> String {
    format!(
        "`{}` shows the full diagnostic and clippy's suggested change",
        clippy_repro(cmd, path, members)
    )
}

/// The clippy command narrowed to the package that owns `path`, built from the args that ran.
/// Drops `--workspace`, every `-p <name>` and `--message-format=short`, then inserts `-p <owner>`
/// after `clippy`. With no owner, the original selectors stay.
fn clippy_repro(cmd: &Cmd, path: &str, members: &[Member]) -> Cmd {
    let owner = package_for(path, members);
    let mut args = Vec::new();
    let mut rest = cmd.args.iter();
    args.extend(rest.next().cloned());
    if let Some(owner) = owner {
        args.extend(["-p".to_owned(), owner.name.clone()]);
    }
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            // Everything after a literal `--` is passed to clippy's lints, copied verbatim.
            "--" => {
                args.push(arg.clone());
                args.extend(rest.by_ref().cloned());
            }
            "--message-format=short" => {}
            "--workspace" if owner.is_some() => {}
            "-p" if owner.is_some() => {
                rest.next();
            }
            _ => args.push(arg.clone()),
        }
    }
    Cmd {
        program: cmd.program.clone(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clippy_repro_drops_the_workspace_flag_and_narrows_to_the_owner() {
        let cmd = Cmd::new(
            crate::process::Program::Cargo,
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--message-format=short",
                "--",
                "-D",
                "warnings",
            ],
        );
        let members = vec![Member {
            name: "xtask".into(),
            dir: "xtask".into(),
            path_deps: vec![],
        }];
        assert_eq!(
            clippy_repro(&cmd, "xtask/src/main.rs", &members).to_string(),
            "cargo clippy -p xtask --all-targets -- -D warnings"
        );
    }

    #[test]
    fn clippy_repro_copies_everything_after_the_separator_verbatim() {
        let cmd = Cmd::new(
            crate::process::Program::Cargo,
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-p",
                "odd",
                "--workspace",
                "--message-format=short",
            ],
        );
        let members = vec![Member {
            name: "xtask".into(),
            dir: "xtask".into(),
            path_deps: vec![],
        }];
        assert_eq!(
            clippy_repro(&cmd, "xtask/src/main.rs", &members).to_string(),
            "cargo clippy -p xtask --all-targets -- -p odd --workspace --message-format=short"
        );
    }

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
            vec![("xtask/src/main.rs".to_owned(), 3)]
        );
    }

    #[test]
    fn fmt_reports_the_changed_line_not_the_hunk_context_start() {
        // Real `cargo fmt --check` output for `fn  bad( ){}` appended at line 40.
        let out = "Diff in /ws/xtask/src/main.rs:37:\n         Commands::Setup => tools::setup(),\n     }\n }\n-fn  bad( ){}\n+fn bad() {}\n\n";
        assert_eq!(
            fmt_diffs(out, "/ws"),
            vec![("xtask/src/main.rs".to_owned(), 40)]
        );
    }

    #[test]
    fn fmt_points_at_the_inserted_line_for_an_add_only_hunk() {
        // Three context lines (37-39), then an added line that goes in front of original line 40.
        let out = "Diff in /ws/xtask/src/main.rs:37:\n         Commands::Setup => setup::run(),\n     }\n }\n+fn added() {}\n\n";
        assert_eq!(
            fmt_diffs(out, "/ws"),
            vec![("xtask/src/main.rs".to_owned(), 40)]
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
                ("xtask/src/main.rs".to_owned(), 3),
                ("xtask/src/lib.rs".to_owned(), 9)
            ]
        );
    }

    #[test]
    fn fmt_parses_windows_path() {
        let out = "Diff in C:\\ws\\xtask\\src\\main.rs:7:\n";
        assert_eq!(
            fmt_diffs(out, "C:\\ws"),
            vec![("xtask/src/main.rs".to_owned(), 7)]
        );
    }

    #[test]
    fn fmt_finds_nothing_in_a_syntax_error() {
        let out = "error: expected one of `!` or `::`, found `(`\n --> xtask/src/main.rs:1:4\n";
        assert_eq!(fmt_diffs(out, "/ws"), Vec::<(String, u32)>::new());
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
