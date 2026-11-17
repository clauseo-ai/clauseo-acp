//! A release bundle laid out in a scratch folder, its stetdoc a stand-in. The agents get stetdoc
//! as a command: its folder first on their `PATH` and the stetdoc line in their instructions,
//! whenever its `--version` works, with its skill or without. stetdoc 0.9.3's skill text (the
//! body alone, no frontmatter) loads under a frontmatter Clauseo writes, its first `# ` heading
//! as the description. A stetdoc whose `--version` fails at the start is treated as absent; one
//! that passes the start's limit, or was not seen to answer yet and so is not tried at the start,
//! stays on and is tried in the background, whose answer the next agent start gets. The stand-ins
//! here are seen before unless a test says otherwise, and the builds take [`LIMITS`], 3 s for
//! each try, so a stand-in's 5 s run passes them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::ProfileHook as _;
use clauseo_profile::instructions::STETDOC_CLI;
use clauseo_profile::skills::stetdoc::start::{Limits, seen};
use clauseo_profile::write_skills;
use clauseo_settings::Locations;
use serde_json::Value;

use super::super::{DesktopProfile, Layout, paths_for};
use super::{CLAUDE, CODEX, agent, developer_instructions};
use crate::test_support::Scratch;

/// What `stetdoc skills get core` printed, byte for byte, from stetdoc 0.9.3 (macOS arm64).
const REAL_0_9_3: &[u8] =
    include_bytes!("../../../../profile/tests/fixtures/stetdoc-0.9.3-skills-get-core.md");

/// The start's limits in these builds: 3 s for each try, seen before or not, so the stand-ins'
/// 5 s runs and hangs pass them; the background check's 30 s as in the app.
const LIMITS: Limits = Limits {
    seen: Duration::from_secs(3),
    unseen: Duration::from_secs(3),
    retry: Duration::from_secs(3),
    recheck: Duration::from_secs(30),
};

/// A stetdoc stand-in: `stetdoc 0.9.3` for `--version`, `skill.txt` beside it for `skills get
/// core` (written by [`stetdoc_at`]), exit 2 for anything else.
const STUB: &[u8] = b"#!/bin/sh
if [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'
elif [ \"$*\" = 'skills get core' ]; then cat \"$(dirname \"$0\")/skill.txt\"
else exit 2; fi
";

/// The stand-in at `<folder>/stetdoc` under `scratch` with `skill` as its skill text.
fn stetdoc_at(scratch: &Scratch, folder: &str, script: &[u8], skill: &[u8]) -> PathBuf {
    scratch.dir(folder);
    scratch.file(&format!("{folder}/skill.txt"), skill);
    let program = scratch.file(&format!("{folder}/stetdoc"), script);
    scratch.executable(&format!("{folder}/stetdoc"));
    program
}

/// Records `program` in the marker of the profile under `scratch`'s data as seen to answer, as
/// an earlier start with an answer left it: the start then tries it within `limits.seen`.
fn seen_before(scratch: &Scratch, exe: &Path, program: &Path) {
    let locations = Locations::under_root(&scratch.dir("data"));
    let paths = paths_for(&locations, exe.to_path_buf(), Some(program.to_path_buf())).unwrap();
    seen::record(&paths.stetdoc_seen(), paths.stetdoc());
}

/// The profile for a macOS bundle under `scratch`, with the stand-in in
/// `Contents/Helpers/stetdoc` when `stand_in` is given, seen to answer before, and [`LIMITS`].
fn macos_bundle(scratch: &Scratch, stand_in: Option<(&[u8], &[u8])>) -> DesktopProfile {
    bundle(scratch, stand_in, true, LIMITS)
}

/// [`macos_bundle`], the stand-in seen before when `seen_first`, with `limits`.
fn bundle(
    scratch: &Scratch,
    stand_in: Option<(&[u8], &[u8])>,
    seen_first: bool,
    limits: Limits,
) -> DesktopProfile {
    scratch.dir("Clauseo.app/Contents/MacOS");
    let exe = scratch.path().join("Clauseo.app/Contents/MacOS/Clauseo");
    if let Some((script, skill)) = stand_in {
        let program = stetdoc_at(
            scratch,
            "Clauseo.app/Contents/Helpers/stetdoc",
            script,
            skill,
        );
        if seen_first {
            seen_before(scratch, &exe, &program);
        }
    }
    let layout = Layout {
        exe,
        macos: true,
        resource_dir: None,
        shell_browser: clauseo_profile::servers::shell_browser(),
        limits,
    };
    let locations = Locations::under_root(&scratch.dir("data"));
    DesktopProfile::build_in(&locations, &layout, None).unwrap()
}

/// The bundled stetdoc of [`macos_bundle`].
fn helpers_stetdoc(scratch: &Scratch) -> PathBuf {
    scratch
        .path()
        .join("Clauseo.app/Contents/Helpers/stetdoc/stetdoc")
}

/// stetdoc's `SKILL.md` as the profile writes it for Claude, if it wrote one.
fn written_skill(hook: &DesktopProfile) -> Option<Vec<u8>> {
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    std::fs::read(token.root().join("stetdoc").join("SKILL.md")).ok()
}

/// The instructions Claude and Codex get.
fn instructions(hook: &DesktopProfile) -> [String; 2] {
    let meta = Value::Object(hook.session_meta(&agent(CLAUDE), None).unwrap());
    let claude = meta
        .pointer("/systemPrompt/append")
        .and_then(Value::as_str)
        .unwrap();
    let token = write_skills(&hook.launch_writes(&agent(CODEX))[0]).unwrap();
    let codex = developer_instructions(&hook.codex_config(&agent(CODEX), Some(&token), None));
    [claude.to_owned(), codex]
}

/// Whether both agents are told to use the `stetdoc` command.
fn both_have_the_stetdoc_line(hook: &DesktopProfile) -> [bool; 2] {
    instructions(hook).map(|text| text.contains(STETDOC_CLI.trim()))
}

fn folder_of(program: &Path) -> Option<PathBuf> {
    program.parent().map(Path::to_path_buf)
}

/// stetdoc's skill fails after a working `--version` (a frontmatter of stetdoc's with no
/// description, bytes that are not UTF-8, a `skills get core` that exits with an error): the
/// skill is left out, and stetdoc stays on for the agents and the app.
#[test]
fn a_release_bundle_whose_skill_fails_keeps_stetdoc_as_a_command() {
    let failing: [(&[u8], &[u8]); 3] = [
        (STUB, b"---\nname: stetdoc\n---\n# stetdoc\n"),
        (STUB, b"\xff\xfe not UTF-8\n"),
        (
            b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'; else exit 3; fi\n",
            b"",
        ),
    ];
    for (at, stand_in) in failing.into_iter().enumerate() {
        let scratch = Scratch::new(&format!("profile-release-noskill-{at}"), &[]);
        let hook = macos_bundle(&scratch, Some(stand_in));
        let program = helpers_stetdoc(&scratch);
        assert_eq!(written_skill(&hook), None, "case {at}");
        assert_eq!(hook.stetdoc(), Some(program.as_path()), "case {at}");
        assert_eq!(hook.path_prepend(), folder_of(&program), "case {at}");
        assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2], "case {at}");
    }
}

/// A stetdoc whose `--version` works but whose `skills get core` hangs is stopped within
/// the first try's limit ([`LIMITS`], 3 s) all told, with no second try (its `--version`
/// answered), and stays on without its skill.
#[test]
fn a_stetdoc_slow_to_print_its_skill_stays_on_without_it() {
    let scratch = Scratch::new("profile-release-slow-skill", &[]);
    let slow: &[u8] =
        b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'; else sleep 60; fi\n";
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hook = macos_bundle(&scratch, Some((slow, b"")));
        let _sent = tx.send((hook, scratch));
    });
    let (hook, scratch) = rx
        .recv_timeout(Duration::from_secs(7))
        .expect("the build returns within 7 s");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}

/// Windows and Linux: `<resources>/stetdoc/stetdoc`, Tauri's resource folder. stetdoc 0.9.3's
/// body-only skill text loads, its body unchanged.
#[test]
fn a_resource_folder_release_layout_keeps_stetdoc_as_a_command() {
    let scratch = Scratch::new("profile-release-resources", &["app"]);
    let program = stetdoc_at(&scratch, "resources/stetdoc", STUB, REAL_0_9_3);
    let resources = scratch.path().join("resources");
    let exe = scratch.path().join("app").join("clauseo");
    seen_before(&scratch, &exe, &program);
    let layout = Layout {
        exe,
        macos: false,
        resource_dir: Some(&resources),
        shell_browser: clauseo_profile::servers::shell_browser(),
        limits: LIMITS,
    };
    let locations = Locations::under_root(&scratch.dir("data"));
    let hook = DesktopProfile::build_in(&locations, &layout, None).unwrap();
    let skill = written_skill(&hook).expect("0.9.3's skill is written");
    let front = b"---\nname: stetdoc\ndescription: \"stetdoc: editing Word documents with tracked \
                  changes\"\n---\n";
    assert_eq!(skill, [front.as_slice(), REAL_0_9_3].concat());
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}

/// A skill text that parses loads, and the agents get the stetdoc line too.
#[test]
fn a_skill_that_parses_loads_beside_the_stetdoc_line() {
    let scratch = Scratch::new("profile-release-skill", &[]);
    let skill = b"---\nname: stetdoc\ndescription: Edits Word files.\n---\n# stetdoc\n";
    let hook = macos_bundle(&scratch, Some((STUB, skill)));
    let program = helpers_stetdoc(&scratch);
    assert_eq!(
        written_skill(&hook).as_deref(),
        Some(&b"---\nname: stetdoc\ndescription: \"Edits Word files.\"\n---\n# stetdoc\n"[..])
    );
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}

/// A stetdoc whose `--version` exits with an error or prints no version is treated as absent,
/// as is a bundle with no stetdoc: no skill, no `PATH` entry and no stetdoc line.
#[test]
fn a_missing_stetdoc_or_one_whose_version_fails_is_absent_for_the_agents() {
    let broken: [Option<&[u8]>; 3] = [
        None,
        Some(b"#!/bin/sh\nexit 3\n"),
        Some(b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo; else echo '# stetdoc'; fi\n"),
    ];
    for (at, script) in broken.into_iter().enumerate() {
        let scratch = Scratch::new(&format!("profile-release-broken-{at}"), &[]);
        let hook = macos_bundle(&scratch, script.map(|script| (script, REAL_0_9_3)));
        assert_eq!(written_skill(&hook), None, "case {at}");
        assert_eq!(hook.path_prepend(), None, "case {at}");
        assert_eq!(both_have_the_stetdoc_line(&hook), [false; 2], "case {at}");
        if script.is_none() {
            assert_eq!(hook.stetdoc(), None);
        }
    }
}

/// A stand-in whose first `slow` runs take 5 s each (as a new program's first runs while the OS
/// scans it), its later runs as `then` says: `ok` answers as [`STUB`], `fail` exits 3. With
/// `pause`, the run after the slow ones (the background check's `--version`) waits 1.5 s first,
/// so a test sees the state the build left before the check's answer lands. It counts its runs
/// in `runs` beside it; the runs are one after another.
fn slow_runs(slow: u32, then: &str, pause: bool) -> Vec<u8> {
    format!(
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         n=$(cat \"$d/runs\" 2>/dev/null || echo 0); echo $((n + 1)) > \"$d/runs\"\n\
         if [ \"$n\" -lt {slow} ]; then sleep 5\n\
         elif [ \"$n\" -eq {slow} ] && [ {pause} = true ]; then sleep 1.5; fi\n\
         [ {then} = fail ] && exit 3\n\
         if [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'\n\
         elif [ \"$*\" = 'skills get core' ]; then cat \"$d/skill.txt\"\n\
         else exit 2; fi\n"
    )
    .into_bytes()
}

/// [`macos_bundle`] on a thread, which must return within 10 s: the start's one try stops at
/// its limit ([`LIMITS`], 3 s).
fn bundle_within_10_s(
    script: Vec<u8>,
    skill: &'static [u8],
    name: &str,
) -> (DesktopProfile, Scratch) {
    let scratch = Scratch::new(name, &[]);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hook = macos_bundle(&scratch, Some((script.as_slice(), skill)));
        let _sent = tx.send((hook, scratch));
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the build returns within 10 s")
}

/// Waits up to 10 s, in steps of 100 ms, for `done` to hold of `hook`: the background try's
/// answer lands a few seconds after the build at most.
fn eventually(hook: &DesktopProfile, done: impl Fn(&DesktopProfile) -> bool) -> bool {
    // A channel whose sender stays alive: each `recv_timeout` waits out its step.
    let (_tx, rx) = std::sync::mpsc::channel::<()>();
    for _ in 0..100 {
        if done(hook) {
            return true;
        }
        let _waited = rx.recv_timeout(Duration::from_millis(100));
    }
    done(hook)
}

/// A stand-in not seen yet whose first run waits for the test's `go` beside it (as a new
/// program held while the OS scans it), then answers as [`STUB`].
const GATED: &[u8] = b"#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
while [ ! -e \"$d/go\" ]; do sleep 0.1; done\n\
if [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'\n\
elif [ \"$*\" = 'skills get core' ]; then cat \"$d/skill.txt\"\n\
else exit 2; fi\n";

/// A stetdoc not seen yet holds nothing at the start: with 60 s for its background try, the
/// build still returns within 10 s, stetdoc on (its folder on `PATH`, the stetdoc line) with no
/// skill, which cannot have loaded before the test lets the stand-in answer. A launch meanwhile
/// waits for the try (`ProfileHook::launch_wait`); once the stand-in answers, that launch and
/// those that follow get the skill, and a launch after it does not wait.
#[test]
fn an_unseen_stetdoc_holds_nothing_at_the_start_and_gets_its_skill_in_the_background() {
    let long = Limits {
        unseen: Duration::from_secs(60),
        retry: Duration::from_secs(60),
        recheck: Duration::from_secs(60),
        ..LIMITS
    };
    let scratch = Scratch::new("profile-release-unseen", &[]);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hook = bundle(&scratch, Some((GATED, REAL_0_9_3)), false, long);
        let _sent = tx.send((hook, scratch));
    });
    let (hook, scratch) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the build does not wait for an unseen stetdoc");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
    // A launch now waits for the try in flight, and gets the skill once the try answers.
    let launch = hook
        .launch_wait(&agent(CLAUDE))
        .expect("a launch waits while the try is in flight");
    assert_eq!(hook.generation(), 0);
    let change = hook.changed(0).expect("the profile may change");
    scratch.file("Clauseo.app/Contents/Helpers/stetdoc/go", b"");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _sent = tx.send(futures::executor::block_on(launch));
    });
    let skills = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the launch's wait ends with the try");
    assert_eq!(skills.generation, 1, "the writes are of the new profile");
    assert_eq!(hook.generation(), 1);
    let token = write_skills(&skills.writes[0]).unwrap();
    assert!(token.root().join("stetdoc").join("SKILL.md").is_file());
    assert!(
        written_skill(&hook).is_some(),
        "later launches get the skill too"
    );
    let changed = futures::executor::block_on(change);
    assert_eq!(changed, 1, "the host is told of the change");
    assert!(
        hook.launch_wait(&agent(CLAUDE)).is_none(),
        "no try in flight: a launch does not wait"
    );
}

/// A stetdoc seen before whose `--version` passes the start's limit (Defender scanning it
/// again after a signature update) holds the build about that limit (3 s here) and stays on at
/// once, with no skill (none cached); the background retry answers and brings the skill to the
/// next agent start.
#[test]
fn a_seen_stetdoc_slow_at_the_start_stays_on_and_gets_its_skill_from_the_retry() {
    let (hook, scratch) = bundle_within_10_s(
        slow_runs(1, "ok", true),
        REAL_0_9_3,
        "profile-release-slow-once",
    );
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
    assert!(
        eventually(&hook, |hook| written_skill(hook).is_some()),
        "the background retry brings the skill"
    );
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}

/// A stetdoc slow at the start, with its skill cached by an earlier start's launch: the agents
/// start with the cached skill, and the background retry, which answers with the same skill
/// (same name, version and bytes), puts nothing in place: the generation stays 0, so no agent
/// is marked to relaunch.
#[test]
fn an_unchanged_cached_skill_leaves_the_generation_at_0() {
    let scratch = Scratch::new("profile-release-cached-same", &[]);
    let earlier = macos_bundle(&scratch, Some((STUB, REAL_0_9_3)));
    let cached = written_skill(&earlier).expect("the earlier start's launch caches the skill");
    drop(earlier);
    let script = slow_runs(1, "ok", true);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let hook = macos_bundle(&scratch, Some((script.as_slice(), REAL_0_9_3)));
        let _sent = tx.send((hook, scratch));
    });
    let (hook, scratch) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the build returns within 10 s");
    assert!(
        hook.launch_wait(&agent(CLAUDE)).is_some(),
        "the background retry is in flight"
    );
    assert!(
        eventually(&hook, |hook| hook.launch_wait(&agent(CLAUDE)).is_none()),
        "the background retry ends"
    );
    let folder = folder_of(&helpers_stetdoc(&scratch)).unwrap();
    let runs = std::fs::read_to_string(folder.join("runs")).unwrap();
    let runs: u32 = runs.trim().parse().unwrap();
    assert!(
        runs >= 2,
        "the retry ran stetdoc after the start's try ({runs} runs)"
    );
    assert_eq!(hook.generation(), 0);
    assert_eq!(written_skill(&hook), Some(cached));
}

/// A stetdoc slow at the start whose background retry fails for real: on at once, then the
/// agents are no longer told about it. Its `PATH` entry and the blackline's program were given
/// to the host at its start and stay until the app restarts.
#[test]
fn a_stetdoc_slow_then_broken_is_no_longer_told_to_the_agents() {
    let (hook, scratch) = bundle_within_10_s(
        slow_runs(1, "fail", true),
        b"",
        "profile-release-slow-broken",
    );
    let program = helpers_stetdoc(&scratch);
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
    assert!(
        eventually(&hook, |hook| both_have_the_stetdoc_line(hook) == [false; 2]),
        "the background retry takes the stetdoc line out"
    );
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
}

/// A stetdoc that hangs on `--version` is killed at the start's limit and stays on, without a
/// skill, its stetdoc line in the instructions; the background tries are not waited for.
#[test]
fn a_stetdoc_that_hangs_on_its_version_stays_on_for_now() {
    let script = b"#!/bin/sh\nsleep 60\n".to_vec();
    let (hook, scratch) = bundle_within_10_s(script, REAL_0_9_3, "profile-release-hangs");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}
