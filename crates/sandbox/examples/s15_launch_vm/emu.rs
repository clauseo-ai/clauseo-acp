//! EXPERIMENT (branch exp-arm64-x64emu, never for main): do x64 runtimes work under Windows'
//! x64 emulation on ARM64 when tier B's runner starts them as a pool user, under the sandbox
//! token? The same rows run with native ARM64 programs as the control.
//!
//! Each `emu-*` row starts its program through the runner as matter A's pool user twice (the
//! emulator's compile cache is cold the first time) and prints, for each run, one line
//! `EMU row=<row> run=<n> exit=<code> wall_ms=<ms> first_line_ms=<ms> ok=<yes|no>` and every
//! stdout and stderr line. Wall time runs from the outer runner's start to its exit, so it holds
//! the pool user's logon; `emu-baseline` (native `cmd /c echo`) measures that overhead.
//! A row passes when both runs exited 0 with the output expected.

use std::path::{Path, PathBuf};

use super::launch::{Finished, Spec, deadline, describe, launch, pool_of, text};
use super::rows::{Ctx, Outcome};

/// The experiment's rows, in the order the workflow runs them.
pub(crate) const EMU_ROWS: [&str; 6] = [
    "emu-baseline",
    "emu-node-arch",
    "emu-node-file",
    "emu-codex-version",
    "emu-claude-version",
    "emu-node-child",
];

/// The `process.arch` node must report: the harness's own architecture, since the workflow
/// pairs an x64 build with x64 runtimes and an ARM64 build with ARM64 ones.
const WANT_ARCH: &str = if cfg!(target_arch = "x86_64") {
    "x64"
} else if cfg!(target_arch = "aarch64") {
    "arm64"
} else {
    "other"
};

/// What a run's output must hold: `Err` with why it doesn't.
type Expect = Box<dyn Fn(&[String]) -> Result<(), String>>;

/// The row's program and what its output must hold; `Err` with the row's outcome when it
/// can't run.
fn case(ctx: &Ctx<'_>) -> Result<(Spec, Expect), Outcome> {
    let m = ctx.manifest;
    let pool = pool_of(m, "A").map_err(Outcome::Fail)?.name.clone();
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_unset| r"C:\Windows".to_owned());
    let cmd = format!(r"{system_root}\System32\cmd.exe");
    let node = |script: &str| {
        m.node.as_ref().map(|node| Spec {
            program: text(node),
            args: vec!["-e".to_owned(), script.to_owned()],
            path: node.parent().map(Path::to_path_buf).into_iter().collect(),
            ..Spec::default()
        })
    };
    let exe = |exe: &Option<PathBuf>, extra: Option<&PathBuf>| {
        exe.as_ref().map(|exe| Spec {
            program: text(exe),
            args: vec!["--version".to_owned()],
            path: exe
                .parent()
                .map(Path::to_path_buf)
                .into_iter()
                .chain(extra.cloned())
                .collect(),
            ..Spec::default()
        })
    };
    let (spec, expect): (Option<Spec>, Expect) = match ctx.row {
        "emu-baseline" => (
            Some(Spec {
                program: cmd,
                args: vec!["/c".to_owned(), "echo emu-base".to_owned()],
                ..Spec::default()
            }),
            Box::new(|out: &[String]| has(out, "emu-base")),
        ),
        "emu-node-arch" => {
            let want = format!("{WANT_ARCH} win32 {pool}").to_ascii_lowercase();
            (
                node(
                    "console.log(process.arch, process.platform, \
                      require('os').userInfo().username);\
                      const os=require('os');\
                      console.log('DETAIL machine='+os.machine()+' arch_env='+\
                      process.env.PROCESSOR_ARCHITECTURE+' w6432='+\
                      process.env.PROCESSOR_ARCHITEW6432+' node='+process.version+\
                      ' uptime_ms='+Math.round(process.uptime()*1000))",
                ),
                Box::new(move |out: &[String]| {
                    let first = out.first().map(|l| l.trim().to_ascii_lowercase());
                    if first.as_deref() == Some(want.as_str()) {
                        Ok(())
                    } else {
                        Err(format!("first line {first:?}, wanted {want:?}"))
                    }
                }),
            )
        }
        "emu-node-file" => (
            node(
                "const fs=require('fs');\
                  const p=require('path').join(process.cwd(),'emu-probe.txt');\
                  const t='emu-'+process.pid;fs.writeFileSync(p,t);\
                  const r=fs.readFileSync(p,'utf8');fs.unlinkSync(p);\
                  console.log(r===t?'FILE ok '+p:'FILE mismatch '+JSON.stringify(r));\
                  console.log('DETAIL arch='+process.arch+' uptime_ms='+\
                  Math.round(process.uptime()*1000))",
            ),
            Box::new(|out: &[String]| has(out, "FILE ok ")),
        ),
        "emu-codex-version" => (
            exe(&m.codex, m.codex_path.as_ref()),
            Box::new(|out: &[String]| has(out, "codex")),
        ),
        "emu-claude-version" => (
            exe(&m.claude, None),
            Box::new(|out: &[String]| {
                if out.iter().any(|l| !l.trim().is_empty()) {
                    Ok(())
                } else {
                    Err("no stdout".to_owned())
                }
            }),
        ),
        "emu-node-child" => (
            node(
                "const t=Date.now();\
                  const o=require('child_process').execSync('cmd /c echo hi').toString().trim();\
                  console.log('CHILD '+o);\
                  console.log('DETAIL arch='+process.arch+' child_ms='+(Date.now()-t)+\
                  ' uptime_ms='+Math.round(process.uptime()*1000))",
            ),
            Box::new(|out: &[String]| has(out, "CHILD hi")),
        ),
        other => return Err(Outcome::Fail(format!("no emu row {other:?}"))),
    };
    let spec = spec
        .ok_or_else(|| Outcome::Skip("needs --node, --codex or --claude at prepare".to_owned()))?;
    Ok((spec, expect))
}

/// The row: its program twice through the runner as A.
pub(crate) fn row(ctx: &Ctx<'_>) -> Outcome {
    let (spec, expect) = match case(ctx) {
        Ok(case) => case,
        Err(outcome) => return outcome,
    };
    ctx.note(&format!(
        "harness arch {WANT_ARCH}; program {} {:?}",
        spec.program, spec.args
    ));
    let mut fails = Vec::new();
    let mut walls = Vec::new();
    for run in 1..=2 {
        match run_once(ctx, &spec) {
            Ok(done) => {
                let verdict = if done.code == 0 {
                    expect(&done.stdout)
                } else {
                    Err(format!("exit {}", describe(done.code)))
                };
                ctx.note(&format!(
                    "EMU row={} run={run} exit={} wall_ms={} first_line_ms={} ok={}",
                    ctx.row,
                    done.code,
                    done.wall_ms,
                    done.first_line_ms
                        .map_or_else(|| "none".to_owned(), |ms| ms.to_string()),
                    if verdict.is_ok() { "yes" } else { "no" }
                ));
                for line in &done.stdout {
                    ctx.note(&format!("run {run} stdout: {line}"));
                }
                for line in &done.stderr {
                    ctx.note(&format!("run {run} stderr: {line}"));
                }
                walls.push(format!("run {run} {} ms", done.wall_ms));
                if let Err(why) = verdict {
                    fails.push(format!("run {run}: {why}"));
                }
            }
            Err(why) => {
                ctx.note(&format!(
                    "EMU row={} run={run} exit=none wall_ms=none first_line_ms=none ok=no",
                    ctx.row
                ));
                ctx.note(&format!("run {run} error: {why}"));
                fails.push(format!("run {run}: {why}"));
            }
        }
    }
    if fails.is_empty() {
        Outcome::Pass(format!("{WANT_ARCH}: {}", walls.join(", ")))
    } else {
        Outcome::Fail(format!("{WANT_ARCH}: {}", fails.join("; ")))
    }
}

/// One run through the runner as A, its share of the row's limit at most.
fn run_once(ctx: &Ctx<'_>, spec: &Spec) -> Result<Finished, String> {
    let running = launch(ctx, "A", spec)?;
    running.finish(deadline(ctx.limit / 3))
}

/// `Ok` when a stdout line holds `needle`.
fn has(out: &[String], needle: &str) -> Result<(), String> {
    if out.iter().any(|line| line.contains(needle)) {
        Ok(())
    } else {
        Err(format!("no stdout line holds {needle:?}"))
    }
}
