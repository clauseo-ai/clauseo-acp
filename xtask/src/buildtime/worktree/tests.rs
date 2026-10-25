//! Tests for `worktree.rs`: whole runs on a scratch repo with real git (worktree add, ls-files,
//! remove, list) and fake cargo and pnpm, the refusals before anything is made, the JSON shape,
//! and how cargo's JSON names what it rebuilt, read from a real cargo build.

use super::super::verdict::ProbeStatus;
use super::*;
use crate::budgets::Gate;
use crate::process::{self, Cmd, Program, Ran, TEST_TIMEOUT};
use crate::scratch::Scratch;
use std::fs::{self, FileTimes};
use std::io;
use std::time::{Duration, SystemTime};

mod leftover;
mod parts;
mod run;
mod target;

/// A time no file made today has by chance.
fn old(offset: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000 + offset)
}

fn modified(path: &Path) -> SystemTime {
    fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .expect("a modification time")
}

fn git(dir: &Path, args: &[&str]) -> Ran {
    let ran = process::run_quiet(&Cmd::new(Program::Git, args, TEST_TIMEOUT), dir).expect("git");
    assert!(ran.success(), "git {args:?}: {}", ran.stderr);
    ran
}

/// A committed repo at `<scratch>/repo` with untracked build output in `target/`, old times on it.
fn repo(scratch: &Scratch) -> PathBuf {
    let root = scratch.join("repo");
    Scratch::create_dir_all(root.join("src")).expect("src");
    Scratch::create_dir_all(root.join("ui")).expect("ui");
    Scratch::write(root.join("Cargo.toml"), "[workspace]\n").expect("a manifest");
    Scratch::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("a source");
    Scratch::write(root.join("ui/package.json"), "{}\n").expect("the ui workspace");
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    let commit = [
        "-c",
        "user.name=x",
        "-c",
        "user.email=x@x",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "base",
    ];
    git(&root, &commit);
    Scratch::create_dir_all(root.join("target/debug")).expect("debug");
    Scratch::write(root.join("target/debug/app"), "app").expect("build output");
    Scratch::write(
        root.join("target/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55",
    )
    .expect("the tag");
    let binary = xtask_binary(&root);
    Scratch::create_dir_all(binary.parent().expect("a folder")).expect("xtask-run");
    Scratch::write(&binary, "xtask").expect("xtask's binary");
    let times = |offset| {
        FileTimes::new()
            .set_accessed(old(offset))
            .set_modified(old(offset))
    };
    Scratch::set_times(root.join("target/debug/app"), times(10)).expect("old");
    Scratch::set_times(&binary, times(20)).expect("old");
    root
}

/// The budget row as `budgets.toml` has it.
const LIMIT: Limit = Limit {
    id: "fresh_agent_worktree",
    value: 30.0,
    gate: Gate::Alert,
};

/// An artifact message of the first build: `package_id`'s package, rebuilt or fresh.
fn artifact(package_id: &str, kind: &str, fresh: bool) -> String {
    format!(
        r#"{{"reason":"compiler-artifact","package_id":"{package_id}","target":{{"kind":["{kind}"],"name":"x"}},"fresh":{fresh}}}"#
    )
}

/// A first build that rebuilt only the app: its build script and lib, with serde reused.
fn app_only() -> String {
    [
        artifact(
            "path+file:///w/crates/desktop#clauseo-desktop@0.0.0",
            "custom-build",
            false,
        ),
        artifact(
            "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.228",
            "lib",
            true,
        ),
        artifact(
            "path+file:///w/crates/desktop#clauseo-desktop@0.0.0",
            "lib",
            false,
        ),
        r#"{"reason":"build-finished","success":true}"#.to_owned(),
    ]
    .join("\n")
}

/// A way the fake tools differ from tools that work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trick {
    /// `cargo xtask --help` rebuilds xtask.
    RebuildXtask,
    /// `git worktree remove` fails.
    RefuseRemove,
    /// `git worktree add` makes the folder, then fails.
    AddFailsAfterMakingIt,
    /// `git worktree list` fails once a `git worktree add` ran.
    ListFailsAfterAdd,
    /// pnpm runs past its limit.
    PnpmTimesOut,
    /// Someone makes a folder at the worktree's path, holding `theirs.txt`, during the warm-up.
    FolderDuringWarmUp,
}

/// What the fake tools do, and what they saw.
#[derive(Default)]
struct Fake {
    /// The first build's stdout, one per run; the last repeats.
    builds: Vec<String>,
    warm_up_stderr: String,
    build_stderr: String,
    /// What the fake tools do differently.
    tricks: Vec<Trick>,
    /// The stderr of a warm-up or a pnpm install that fails.
    warm_up_fails: Option<String>,
    pnpm_fails: Option<String>,
    /// What `cargo metadata` prints instead of naming `<dir>/target` as both dirs.
    metadata: Option<String>,
    /// Each command and the folder it ran in.
    seen: Vec<(String, PathBuf)>,
    /// At each first build: a tracked file's time, the copied app's time, whether the tag came.
    at_build: Vec<(SystemTime, SystemTime, bool)>,
}

impl Fake {
    fn run(&mut self, cmd: &Cmd, dir: &Path) -> io::Result<Ran> {
        self.seen.push((cmd.to_string(), dir.to_path_buf()));
        let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
        match (&cmd.program, args.as_slice()) {
            (Program::Git, ["worktree", "remove", ..])
                if self.tricks.contains(&Trick::RefuseRemove) =>
            {
                Ok(Ran::exited(false, "", "fatal: refused by the test\n"))
            }
            (Program::Git, ["worktree", "add", "--detach", path, "HEAD"])
                if self.tricks.contains(&Trick::AddFailsAfterMakingIt) =>
            {
                Scratch::create_dir_all(path).expect("the folder git made");
                Ok(Ran::exited(false, "", "fatal: failed by the test\n"))
            }
            (Program::Git, ["worktree", "list", ..])
                if self.tricks.contains(&Trick::ListFailsAfterAdd)
                    && self.ran("git worktree add") > 0 =>
            {
                Ok(Ran::exited(false, "", "fatal: refused by the test\n"))
            }
            (Program::Git, _) => process::run_quiet(cmd, dir),
            (Program::Pnpm, _) if self.tricks.contains(&Trick::PnpmTimesOut) => {
                Ok(Ran::timed_out())
            }
            (Program::Cargo, ["metadata", ..]) => {
                let own = metadata_naming(&dir.join("target"), &dir.join("target"));
                let printed = self.metadata.clone().unwrap_or(own);
                Ok(Ran::exited(true, printed, ""))
            }
            (Program::Cargo, ["build", .., "--message-format=json", "-v"]) => {
                self.at_build.push((
                    modified(&dir.join("src/lib.rs")),
                    modified(&dir.join("target/debug/app")),
                    dir.join("target/CACHEDIR.TAG").is_file(),
                ));
                let run = self.at_build.len().min(self.builds.len()) - 1;
                Ok(Ran::exited(
                    true,
                    self.builds[run].clone(),
                    self.build_stderr.clone(),
                ))
            }
            (Program::Cargo, ["build", "-p", APP, "-v"]) => {
                if self.tricks.contains(&Trick::FolderDuringWarmUp) {
                    let theirs = place(dir).expect("a place");
                    Scratch::create_dir_all(&theirs).expect("their folder");
                    Scratch::write(theirs.join("theirs.txt"), "theirs").expect("their file");
                }
                match &self.warm_up_fails {
                    Some(stderr) => Ok(Ran::exited(false, "", stderr.clone())),
                    None => Ok(Ran::exited(true, "", self.warm_up_stderr.clone())),
                }
            }
            (Program::Cargo, ["xtask", "--help"]) => {
                if self.tricks.contains(&Trick::RebuildXtask) {
                    let times = FileTimes::new().set_modified(old(500));
                    Scratch::set_times(xtask_binary(dir), times).expect("a new binary");
                }
                Ok(Ran::exited(true, "Usage: cargo xtask <COMMAND>", ""))
            }
            (Program::Pnpm, _) => match &self.pnpm_fails {
                Some(stderr) => Ok(Ran::exited(false, "", stderr.clone())),
                None => Ok(Ran::exited(true, "", "")),
            },
            _ => Err(io::Error::other(format!("the test does not fake `{cmd}`"))),
        }
    }

    fn ran(&self, start: &str) -> usize {
        self.seen
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(start))
            .count()
    }
}

/// What `cargo metadata --format-version 1 --no-deps` prints, in part, for a target dir and a
/// build dir.
fn metadata_naming(target: &Path, build: &Path) -> String {
    serde_json::json!({
        "packages": [],
        "target_directory": target,
        "build_directory": build,
        "version": 1,
    })
    .to_string()
}

/// Measures `runs` runs of the worktree of `root` with `fake`.
fn measure_with(root: &Path, ci: bool, runs: u32, fake: &mut Fake) -> Result<Measured, Failure> {
    let worktree = Worktree {
        root,
        path: place(root).expect("a place"),
        ci,
    };
    measure(&worktree, runs, LIMIT, &mut |cmd: &Cmd, dir: &Path| {
        fake.run(cmd, dir)
    })
}

fn find<'a>(measured: &'a Measured, name: &str) -> &'a ProbeResult {
    measured
        .results
        .iter()
        .find(|result| result.name == name)
        .expect("the probe is there")
}

fn samples(result: &ProbeResult) -> Vec<f64> {
    match &result.outcome {
        Outcome::Measured { samples, .. } => samples.clone(),
        other => panic!("{} is not measured: {other:?}", result.name),
    }
}

/// Whether git still lists `path` as a worktree of `root`, or it is on disk.
fn left_behind(root: &Path, path: &Path) -> bool {
    let listed = git(root, &["worktree", "list", "--porcelain"]).stdout;
    let name = path
        .file_name()
        .expect("a name")
        .to_string_lossy()
        .into_owned();
    listed.contains(&name) || fs::symlink_metadata(path).is_ok()
}

#[test]
fn each_run_makes_the_worktree_times_six_steps_and_removes_it() {
    let scratch = Scratch::new("worktree-runs");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 2, &mut fake).expect("measured");
    let path = place(&root).expect("a place");
    assert_eq!(path.file_name().expect("a name"), "repo-buildtime-worktree");
    assert!(!left_behind(&root, &path), "the worktree is gone");
    let names: Vec<&str> = measured.results.iter().map(|result| result.name).collect();
    assert_eq!(
        names,
        [
            "worktree_add",
            "worktree_copy",
            "worktree_touch",
            "worktree_pnpm_install",
            "worktree_first_build",
            "worktree_first_xtask",
            "fresh_worktree"
        ]
    );
    assert!(
        STEPS
            .into_iter()
            .enumerate()
            .all(|(place, step)| step as usize == place),
        "a step's place in STEPS is its discriminant, as `measure` indexes them"
    );
    let steps: Vec<Vec<f64>> = STEPS
        .iter()
        .map(|step| samples(find(&measured, step.name())))
        .collect();
    for step in STEPS {
        let result = find(&measured, step.name());
        assert!(result.reported_only, "{}", step.name());
        assert_eq!(result.status(), ProbeStatus::Reported, "{}", step.name());
    }
    let total = find(&measured, TOTAL);
    assert!(!total.reported_only);
    for (run, sum) in samples(total).into_iter().enumerate() {
        let added: f64 = steps.iter().map(|secs| secs[run]).sum();
        assert!((sum - added).abs() < 1e-9, "run {run} adds up its steps");
    }
    let Outcome::Measured { budget, .. } = &total.outcome else {
        panic!("measured");
    };
    assert_eq!(budget.map(|budget| budget.id), Some("fresh_agent_worktree"));
    assert!(measured.passed(false) && measured.errors.is_empty());
    assert_eq!(
        fake.at_build,
        vec![(touched_time(), old(10), true); 2],
        "tracked files are touched, the build output keeps its time, and the tag comes along"
    );
    let ui = path.join("ui");
    let install = format!(
        "pnpm -C {} install --frozen-lockfile --prefer-offline",
        ui.display()
    );
    for (start, dir) in [
        ("cargo build -p clauseo-desktop -v", &root),
        ("git worktree add --detach", &root),
        (install.as_str(), &path),
        (
            "cargo build -p clauseo-desktop --message-format=json -v",
            &path,
        ),
        ("cargo xtask --help", &path),
    ] {
        let dirs: Vec<&PathBuf> = fake
            .seen
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(start))
            .map(|(_, dir)| dir)
            .collect();
        assert!(
            !dirs.is_empty() && dirs.iter().all(|seen| *seen == dir),
            "{start} ran in {}",
            dir.display()
        );
    }
    assert_eq!(
        fake.ran("cargo build -p clauseo-desktop -v"),
        1,
        "one warm-up"
    );
}

#[test]
fn uncommitted_changes_to_tracked_files_stop_the_run_before_anything_is_made() {
    let scratch = Scratch::new("worktree-dirty");
    let root = repo(&scratch);
    Scratch::write(root.join("src/lib.rs"), "pub fn g() {}\n").expect("an edit");
    let mut fake = Fake::default();
    let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
    assert_eq!(
        refused.message,
        "refusing to measure: tracked files have uncommitted changes (`git status --porcelain \
         --untracked-files=no` lists them), and the worktree is made from HEAD, so its first build \
         would not match the warm checkout"
    );
    assert_eq!(
        refused.fix,
        "commit them or stash them (`git stash`), then run again"
    );
    assert_eq!(fake.ran("cargo"), 0, "no warm-up");
    assert!(!left_behind(&root, &place(&root).expect("a place")));
}

#[test]
fn a_first_build_that_rebuilt_another_package_fails_and_its_run_gives_no_sample() {
    let scratch = Scratch::new("worktree-rebuilt-more");
    let root = repo(&scratch);
    let more = format!(
        "{}\n{}",
        artifact(
            "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.228",
            "lib",
            false
        ),
        app_only()
    );
    let mut fake = Fake {
        builds: vec![more, app_only()],
        build_stderr: "       Fresh libc v0.2.177\n       Dirty serde v1.0.228: the dependency \
                       build_script changed\n"
            .to_owned(),
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 2, &mut fake).expect("measured");
    let build = find(&measured, "worktree_first_build");
    let Outcome::Failed(failure) = &build.outcome else {
        panic!("the first build failed: {:?}", build.outcome);
    };
    assert_eq!(
        failure.message,
        "`cargo build -p clauseo-desktop --message-format=json -v` rebuilt more than \
         clauseo-desktop in the worktree: serde (lib)"
    );
    assert_eq!(failure.fix, REBUILT_FIX);
    assert_eq!(
        failure.output.as_deref(),
        Some("       Dirty serde v1.0.228: the dependency build_script changed\n"),
        "the reasons cargo gave"
    );
    assert_eq!(
        measured.notes,
        [
            "run 2, first build: rebuilt clauseo-desktop (custom-build), clauseo-desktop (lib); \
          fresh: 1"
        ],
        "a line for the build that passed"
    );
    assert!(
        build.reported_only && build.fails_run(true),
        "no number fails the run"
    );
    assert_eq!(
        samples(find(&measured, "worktree_first_xtask")).len(),
        1,
        "run 2 only"
    );
    assert_eq!(
        samples(find(&measured, TOTAL)).len(),
        1,
        "the failed run adds nothing"
    );
    assert!(!measured.passed(true));
    assert!(
        !left_behind(&root, &place(&root).expect("a place")),
        "removed after a failure"
    );
}

#[test]
fn an_xtask_that_rebuilt_fails_the_last_step_and_leaves_the_total_skipped() {
    let scratch = Scratch::new("worktree-xtask-rebuilt");
    let root = repo(&scratch);
    let mut fake = Fake {
        builds: vec![app_only()],
        tricks: vec![Trick::RebuildXtask],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    let binary = xtask_binary(&place(&root).expect("a place"));
    assert_eq!(
        find(&measured, "worktree_first_xtask").outcome,
        Outcome::Failed(Failure::new(
            format!(
                "`cargo xtask --help` rebuilt xtask in the worktree: {} changed",
                binary.display()
            ),
            XTASK_REBUILT_FIX
        ))
    );
    assert_eq!(
        find(&measured, TOTAL).outcome,
        Outcome::Skipped(NO_TOTAL.into())
    );
    assert!(!measured.passed(false));
}
