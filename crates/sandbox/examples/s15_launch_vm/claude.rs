//! The claude-mock row (which shell tool Claude Code offers and uses as a pool user) and
//! prepare's copy of Claude Code.
//!
//! The mock (`mock-anthropic.mjs`) runs as the harness user on a loopback port in
//! `TIER_B_PORTS`, as the codex row's does (the only ports tier B's WFP filters let A reach).
//! Claude Code runs through the runner as A in print mode, under `probe stdin-null` (print mode
//! reads a piped stdin to its end before it starts), with `ANTHROPIC_BASE_URL` at the mock, a
//! dummy `ANTHROPIC_API_KEY`, and `CLAUDE_CONFIG_DIR` at `<state A>\claude-home` holding the
//! `settings.json` the host writes (`crates/host/src/agent/home.rs`). The mock asks for one call
//! of the shell tool the request offers (`Bash`, else `PowerShell`) running `echo
//! clauseo-mock-ok`, and answers the result by quoting it. The row reports the exit code, whether
//! stdout holds the marker, the shell tool names offered and the tool called; it passes when
//! stdout holds the marker and Claude Code exited 0.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::codex::{port_of, tier_b_port};
use super::launch::{Spec, deadline, launch, run_command, summary, text};
use super::manifest::Manifest;
use super::prepare::{MOCK_ANTHROPIC_FILE, copy_tree};
use super::rows::{Ctx, Outcome};
use super::sys::{copy_file, make_dir, remove_file, say, write_file};

/// What the mock's shell call prints.
const MARKER: &str = "clauseo-mock-ok";
/// The dummy key; the mock checks none.
const DUMMY_KEY: &str = "dummy-not-a-secret";

/// Copies Claude Code into `bin\claude`. `given` is `claude.exe` (copied alone), `cli.js` (its
/// package folder is copied; it runs under `--node`), or a folder searched for `claude.exe`
/// first, then for `@anthropic-ai\claude-code\cli.js`, whose folder is copied.
pub(crate) fn copy_claude(given: &Path, m: &mut Manifest) -> Result<(), String> {
    let entry = if given.is_file() {
        given.to_path_buf()
    } else {
        find(given, "claude.exe", 0)
            .or_else(|| find(given, "cli.js", 0).filter(|p| text(p).contains("claude-code")))
            .ok_or_else(|| {
                format!(
                    "no claude.exe or claude-code cli.js under {}",
                    given.display()
                )
            })?
    };
    let name = entry.file_name().ok_or("Claude Code's entry has no name")?;
    let to = m.bin().join("claude");
    make_dir(&to).map_err(|e| format!("making {}: {e}", to.display()))?;
    let copied = if is_js(&entry) {
        let from = entry.parent().ok_or("cli.js has no folder")?;
        copy_tree(from, &to.join("package"))?;
        to.join("package").join(name)
    } else {
        let target = to.join(name);
        copy_file(&entry, &target).map_err(|e| format!("copying {}: {e}", entry.display()))?;
        target
    };
    say(&format!(
        "prepare: copied Claude Code {} to {}",
        entry.display(),
        copied.display()
    ));
    m.claude = Some(copied);
    Ok(())
}

fn is_js(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("js") || e.eq_ignore_ascii_case("mjs"))
}

/// The first file named `name` under `dir`, by depth (no more than 6 folders down).
fn find(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    if depth > 6 {
        return None;
    }
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() && entry.file_name().eq_ignore_ascii_case(name) {
            return Some(entry.path());
        }
        if kind.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.iter().find_map(|d| find(d, name, depth + 1))
}

/// The row.
pub(crate) fn row(ctx: &Ctx<'_>) -> Outcome {
    let m = ctx.manifest;
    let (Some(claude), Some(node)) = (&m.claude, &m.node) else {
        return Outcome::Skip("needs --claude and --node at prepare".to_owned());
    };
    ctx.outcome(run(ctx, claude, node))
}

fn run(ctx: &Ctx<'_>, claude: &Path, node: &Path) -> Result<String, String> {
    let m = ctx.manifest;
    let until = deadline(ctx.limit);
    let log = m.logs().join("claude-mock.jsonl");
    drop(remove_file(&log));
    let want = tier_b_port()?;
    let mut command = Command::new(node);
    command
        .arg(m.root.join(MOCK_ANTHROPIC_FILE))
        .arg(format!("port={want}"))
        .arg(format!("log={}", text(&log)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut mock = run_command(&mut command)?;
    let listening = mock
        .line_after("{", until)
        .ok_or("the mock printed no listening line")?;
    let port = port_of(&listening).ok_or_else(|| format!("no port in {listening:?}"))?;
    ctx.note(&format!("mock on 127.0.0.1:{port} (pid {})", mock.pid()));

    // What the host forces on a Claude launch: its own Claude home as CLAUDE_CONFIG_DIR, with
    // the settings it writes there.
    let home = m.state("A").join("claude-home");
    make_dir(&home).map_err(|e| format!("making {}: {e}", home.display()))?;
    write_file(
        &home.join("settings.json"),
        b"{\"syncClaudeAiSkills\": false}\n",
    )
    .map_err(|e| format!("writing settings.json: {e}"))?;

    let mut args = vec!["probe".to_owned(), "stdin-null".to_owned()];
    let mut path = Vec::new();
    if is_js(claude) {
        args.push(text(node));
        path.extend(node.parent().map(Path::to_path_buf));
    }
    args.push(text(claude));
    args.extend(
        [
            "-p",
            "run the echo",
            "--dangerously-skip-permissions",
            "--allowedTools",
            "Bash,PowerShell",
            "--output-format",
            "text",
            "--max-turns",
            "4",
        ]
        .map(str::to_owned),
    );
    path.extend(claude.parent().map(Path::to_path_buf));
    let mut env: Vec<(String, String)> = [
        ("ANTHROPIC_BASE_URL", format!("http://127.0.0.1:{port}")),
        ("ANTHROPIC_API_KEY", DUMMY_KEY.to_owned()),
        ("CLAUDE_CONFIG_DIR", text(&home)),
        ("DISABLE_AUTOUPDATER", "1".to_owned()),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_owned()),
        ("NO_COLOR", "1".to_owned()),
    ]
    .map(|(k, v)| (k.to_owned(), v))
    .to_vec();
    // Git Bash, when this machine names one: Claude Code's Bash tool on Windows runs it.
    if let Ok(bash) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        ctx.note(&format!(
            "CLAUDE_CODE_GIT_BASH_PATH={bash} (from the harness's environment)"
        ));
        env.push(("CLAUDE_CODE_GIT_BASH_PATH".to_owned(), bash));
    }
    let spec = Spec {
        program: text(&m.harness_exe()),
        args,
        env,
        path,
        ..Spec::default()
    };
    let done = launch(ctx, "A", &spec)?.finish(until)?;
    ctx.note(&summary(&done));
    let all: Vec<&String> = done.stdout.iter().chain(&done.stderr).collect();
    for line in all.iter().skip(all.len().saturating_sub(40)) {
        ctx.note(&format!("claude: {line}"));
    }
    mock.kill();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    for line in log_text.lines() {
        ctx.note(&format!("mock: {line}"));
    }
    let seen = Seen::read(&log_text);
    let marked = done.stdout.iter().any(|l| l.contains(MARKER));
    let report = format!(
        "exit {}; stdout has {MARKER}: {}; shell tools offered: [{}]; the mock called: {}; \
         a tool result came back: {}",
        super::launch::describe(done.code),
        if marked { "yes" } else { "no" },
        seen.offered.join(", "),
        seen.called.as_deref().unwrap_or("none"),
        seen.result.as_deref().unwrap_or("no"),
    );
    ctx.note(&report);
    if marked && done.code == 0 {
        Ok(report)
    } else {
        Err(report)
    }
}

/// What the mock's log shows.
#[derive(Debug, Default)]
struct Seen {
    /// The shell tool names any `/v1/messages` request offered, first seen first.
    offered: Vec<String>,
    /// The tool the mock called (`tool_use:<name>`), the first time.
    called: Option<String>,
    /// The first tool result the mock got back, as logged.
    result: Option<String>,
}

impl Seen {
    fn read(log: &str) -> Seen {
        let mut seen = Seen::default();
        for line in log.lines().filter(|l| l.contains("\"kind\":\"messages\"")) {
            for name in string_list(line, "\"shells\":[") {
                if !seen.offered.contains(&name) {
                    seen.offered.push(name);
                }
            }
            if seen.called.is_none() {
                seen.called = line
                    .split("\"reply\":\"tool_use:")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                    .map(str::to_owned);
            }
            if seen.result.is_none() && !line.contains("\"results\":[]") {
                seen.result = line
                    .split("\"results\":")
                    .nth(1)
                    .and_then(|rest| rest.split("],\"reply\"").next())
                    .map(|r| format!("{r}]"));
            }
        }
        seen
    }
}

/// The plain strings of the JSON list starting at `key` in `line` (names hold no quote).
fn string_list(line: &str, key: &str) -> Vec<String> {
    let Some(rest) = line.split(key).nth(1) else {
        return Vec::new();
    };
    let inner = rest.split(']').next().unwrap_or_default();
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Seen;

    #[test]
    fn reads_the_mock_log() {
        let log = concat!(
            "{\"kind\":\"listening\",\"port\":1}\n",
            "{\"kind\":\"messages\",\"tools\":[],\"shells\":[],\"results\":[],\"reply\":\"text-side\"}\n",
            "{\"kind\":\"messages\",\"tools\":[\"Read\",\"PowerShell\"],\"shells\":[\"PowerShell\"],",
            "\"results\":[],\"reply\":\"tool_use:PowerShell\"}\n",
            "{\"kind\":\"messages\",\"tools\":[\"PowerShell\"],\"shells\":[\"PowerShell\"],",
            "\"results\":[{\"is_error\":false,\"text\":\"clauseo-mock-ok\"}],",
            "\"reply\":\"text-after-result\"}\n",
        );
        let seen = Seen::read(log);
        assert_eq!(seen.offered, ["PowerShell"]);
        assert_eq!(seen.called.as_deref(), Some("PowerShell"));
        assert_eq!(
            seen.result.as_deref(),
            Some("[{\"is_error\":false,\"text\":\"clauseo-mock-ok\"}]")
        );
    }
}
