//! The codex row (G6 row 13, openai/codex#52712: Codex on Windows ARM64) and prepare's copy of
//! the codex package.
//!
//! The mock (`mock-responses.mjs`, copied from the MXC spike) runs as the harness user on a
//! loopback port; `<state A>\codex-home\config.toml` points codex at it; `codex exec` runs
//! through the runner as A with full access (tier B's choice, D3 (c)). The mock asks for one
//! shell call that writes `s15-codex.txt` in A's matter and runs `probe job-ui <atom>` (the
//! harness's global atom's name, in the command the mock asks for). Passes when the file holds
//! `s15-codex-ok`, its owner is A's pool SID, the mock's log shows the call's output came back,
//! and the job-ui lines pass `checks_ui::ui_limits` (the atom lookup is information).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::launch::{Spec, deadline, launch, pool_of, run_command, summary, text};
use super::manifest::Manifest;
use super::prepare::{MOCK_FILE, copy_tree};
use super::rows::{Ctx, Outcome};
use super::sys::{make_dir, remove_file, say, write_file};
use super::win;

/// The file the mock's shell call writes in A's matter, and what it holds.
const SHELL_FILE: &str = "s15-codex.txt";
const SHELL_TEXT: &str = "s15-codex-ok";
/// The file the same call writes `probe job-ui`'s output to.
const JOB_FILE: &str = "s15-codex-job.txt";

/// Copies codex into `bin\codex`: `given` is the npm package folder (searched for the vendor
/// `codex.exe`) or `codex.exe` itself. The exe's folder is copied, and its sibling `path`
/// folder (`rg.exe`) when there is one.
pub(crate) fn copy_codex(given: &Path, m: &mut Manifest) -> Result<(), String> {
    let exe = if given.is_file() {
        given.to_path_buf()
    } else {
        find_codex(given, 0).ok_or_else(|| format!("no codex.exe under {}", given.display()))?
    };
    let from = exe.parent().ok_or("codex.exe has no folder")?;
    let to = m.bin().join("codex");
    make_dir(&to).map_err(|e| format!("making {}: {e}", to.display()))?;
    copy_tree(from, &to.join("codex"))?;
    m.codex = Some(to.join("codex").join("codex.exe"));
    if let Some(path) = from.parent().map(|p| p.join("path")).filter(|p| p.is_dir()) {
        copy_tree(&path, &to.join("path"))?;
        m.codex_path = Some(to.join("path"));
    }
    say(&format!(
        "prepare: copied {} to {}",
        from.display(),
        to.display()
    ));
    Ok(())
}

/// The vendor `codex.exe` under `dir`: one in a path with `vendor` in it first.
fn find_codex(dir: &Path, depth: u32) -> Option<PathBuf> {
    if depth > 10 {
        return None;
    }
    let mut found: Vec<PathBuf> = Vec::new();
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            dirs.push(entry.path());
        } else if kind.is_file() && entry.file_name().eq_ignore_ascii_case("codex.exe") {
            found.push(entry.path());
        }
    }
    found.extend(dirs.iter().filter_map(|d| find_codex(d, depth + 1)));
    found
        .iter()
        .find(|p| text(p).to_ascii_lowercase().contains("vendor"))
        .or_else(|| found.first())
        .cloned()
}

/// The row.
pub(crate) fn row(ctx: &Ctx<'_>) -> Outcome {
    let m = ctx.manifest;
    let (Some(codex), Some(node)) = (&m.codex, &m.node) else {
        return Outcome::Skip("needs --codex and --node at prepare".to_owned());
    };
    match super::checks_ui::with_atom(ctx, |atom| run(ctx, codex, node, atom)) {
        Ok(note) => Outcome::Pass(note),
        Err(why) => Outcome::Fail(why),
    }
}

fn run(ctx: &Ctx<'_>, codex: &Path, node: &Path, atom: &str) -> Result<String, String> {
    let m = ctx.manifest;
    let until = deadline(ctx.limit);
    let matter = m.matter("A");
    let target = matter.join(SHELL_FILE);
    drop(remove_file(&target));
    let job_file = matter.join(JOB_FILE);
    drop(remove_file(&job_file));
    if text(&m.harness_exe()).contains(' ') {
        return Err(
            "the harness root has a space; the mock's shell call can't name the probe".into(),
        );
    }
    let log = m.logs().join("codex-mock.jsonl");
    drop(remove_file(&log));
    let mut command = Command::new(node);
    command
        .arg(m.root.join(MOCK_FILE))
        .arg("port=0")
        .arg(format!("matter={}", text(&matter)))
        .arg(format!("log={}", text(&log)))
        .arg(format!("probe={}", text(&m.harness_exe())))
        .arg(format!("atom={atom}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut mock = run_command(&mut command)?;
    let listening = mock
        .line_after("{", until)
        .ok_or("the mock printed no listening line")?;
    let port = port_of(&listening).ok_or_else(|| format!("no port in {listening:?}"))?;
    ctx.note(&format!("mock on 127.0.0.1:{port} (pid {})", mock.pid()));

    let home = m.state("A").join("codex-home");
    make_dir(&home).map_err(|e| format!("making {}: {e}", home.display()))?;
    let config = format!(
        "model = \"gpt-mock\"\nmodel_provider = \"mock\"\napproval_policy = \"never\"\n\
         sandbox_mode = \"danger-full-access\"\n\n[model_providers.mock]\nname = \"mock\"\n\
         base_url = \"http://127.0.0.1:{port}/v1\"\nwire_api = \"responses\"\n\
         env_key = \"MOCK_API_KEY\"\n"
    );
    write_file(&home.join("config.toml"), config.as_bytes())
        .map_err(|e| format!("writing config.toml: {e}"))?;

    let mut path = vec![codex.parent().map(Path::to_path_buf).unwrap_or_default()];
    path.extend(m.codex_path.clone());
    let spec = Spec {
        program: text(codex),
        args: [
            "exec",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
            "-C",
            &text(&matter),
            "Run the S15 harness test.",
        ]
        .map(str::to_owned)
        .to_vec(),
        env: vec![
            ("CODEX_HOME".to_owned(), text(&home)),
            (
                "MOCK_API_KEY".to_owned(),
                "mock-key-not-a-secret".to_owned(),
            ),
            ("NO_COLOR".to_owned(), "1".to_owned()),
        ],
        path,
        ..Spec::default()
    };
    let done = launch(m, "A", &spec)?.finish(until)?;
    ctx.note(&summary(&done));
    let all: Vec<&String> = done.stdout.iter().chain(&done.stderr).collect();
    for line in all.iter().skip(all.len().saturating_sub(40)) {
        ctx.note(&format!("codex: {line}"));
    }
    mock.kill();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    for line in log_text.lines() {
        ctx.note(&format!("mock: {line}"));
    }
    let written = std::fs::read_to_string(&target).map_err(|e| {
        format!(
            "{} wasn't written ({e}); codex {}",
            target.display(),
            summary(&done)
        )
    })?;
    if written.trim() != SHELL_TEXT {
        return Err(format!(
            "{} holds {written:?}, not {SHELL_TEXT:?}",
            target.display()
        ));
    }
    let owner = win::file_owner(&text(&target)).map_err(|e| format!("its owner: {e}"))?;
    let pool = pool_of(m, "A")?;
    if owner != pool.sid {
        return Err(format!(
            "{SHELL_FILE} is owned by {owner}, not {} ({})",
            pool.name, pool.sid
        ));
    }
    let came_back = log_text
        .lines()
        .any(|line| line.contains("\"api\":\"responses\"") && !line.contains("\"outputs\":[]"));
    if !came_back {
        return Err("the mock's log shows no request carrying the shell call's output".to_owned());
    }
    let limits = job_ui(ctx, &job_file, atom)?;
    if done.code != 0 {
        return Err(format!(
            "the file is right but codex exited {}",
            super::launch::describe(done.code)
        ));
    }
    Ok(format!(
        "{SHELL_FILE} holds {SHELL_TEXT}, owned by {}; the call's output came back; codex's \
         shell call: {limits}",
        pool.name
    ))
}

/// The `probe job-ui <atom>` lines codex's shell call wrote to `job_file`, checked by
/// `checks_ui::ui_limits`, with the atom lookup noted as information.
fn job_ui(ctx: &Ctx<'_>, job_file: &Path, atom: &str) -> Result<String, String> {
    let lines: Vec<String> = std::fs::read_to_string(job_file)
        .map_err(|e| format!("{} wasn't written ({e})", job_file.display()))?
        .lines()
        .map(|l| l.trim().to_owned())
        .collect();
    for line in &lines {
        ctx.note(&format!("job-ui: {line}"));
    }
    let limits = super::checks_ui::ui_limits(ctx, &lines)?;
    super::checks_ui::atom_info(ctx, &lines, atom);
    Ok(limits)
}

/// The port in the mock's `listening` line.
fn port_of(line: &str) -> Option<u16> {
    let after = line.split("\"port\":").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}
