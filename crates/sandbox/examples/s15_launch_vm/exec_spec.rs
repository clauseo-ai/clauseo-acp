//! `exec-spec`: starts the launch spec a host built (`--spec-json`, read by `spec_json.rs`) as
//! `--matter`'s pool user, through [`launch`] as every row does, under `--row-timeout`. The
//! environment is the file's alone ([`Spec::exact_env`]): the runner's own rules still apply
//! (`runner/env.rs`), and that is what the row measures.

use std::sync::Mutex;

use super::cli::Opts;
use super::launch::{Spec, deadline, describe, launch, summary};
use super::manifest::Manifest;
use super::rows::Ctx;
use super::spec_json;
use super::sys::say;

/// Starts the spec in `opts.spec_json` as `opts.matter`'s pool user and waits for it; true when
/// it exits 0. Prints the summary, the stdout and stderr, and a `PASS` or `FAIL` line.
pub(crate) fn run(opts: &Opts, manifest: &Manifest) -> bool {
    let (Some(which), Some(file)) = (opts.matter.as_deref(), opts.spec_json.as_deref()) else {
        say("FAIL exec-spec: needs --matter and --spec-json");
        return false;
    };
    let json = match spec_json::load(file) {
        Ok(json) => json,
        Err(why) => {
            say(&format!("FAIL exec-spec: {why}"));
            return false;
        }
    };
    let spec = Spec {
        program: json.program,
        args: json.args,
        env: json.env,
        exact_env: true,
        ..Spec::default()
    };
    let ctx = Ctx {
        manifest,
        limit: opts.row_timeout,
        row: "exec-spec",
        strict: opts.interactive_desktop,
        info: Mutex::new(None),
    };
    let done = match launch(&ctx, which, &spec).and_then(|run| run.finish(deadline(ctx.limit))) {
        Ok(done) => done,
        Err(why) => {
            say(&format!("FAIL exec-spec: {why}"));
            return false;
        }
    };
    ctx.note(&format!(
        "{which}: {} {}: {}",
        spec.program,
        spec.args.join(" "),
        summary(&done)
    ));
    for line in &done.stdout {
        ctx.note(&format!("stdout: {line}"));
    }
    for line in &done.stderr {
        ctx.note(&format!("stderr: {line}"));
    }
    if done.code == 0 {
        say("PASS exec-spec");
        return true;
    }
    say(&format!("FAIL exec-spec: exit {}", describe(done.code)));
    false
}
