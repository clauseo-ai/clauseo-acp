//! A release bundle laid out in a scratch folder, its stetdoc a stand-in. The agents get stetdoc
//! as a command: its folder first on their `PATH` and the stetdoc line in their instructions,
//! whenever its `--version` works, with its skill or without. stetdoc 0.9.3's skill text (the
//! body alone, no frontmatter) loads under a frontmatter Clauseo writes, its first `# ` heading
//! as the description. A stetdoc whose `--version` fails is treated as absent; one whose first
//! `--version` only passes the first limit (`start::FIRST_LIMIT`, 3 s) stays on and is checked
//! again in the background, whose answer the next agent start gets.

use std::path::{Path, PathBuf};

use clauseo_host::ProfileHook as _;
use clauseo_profile::instructions::STETDOC_CLI;
use clauseo_profile::write_skills;
use clauseo_settings::Locations;
use serde_json::Value;

use super::super::{DesktopProfile, Layout};
use super::{CLAUDE, CODEX, agent, developer_instructions};
use crate::test_support::Scratch;

/// What `stetdoc skills get core` printed, byte for byte, from stetdoc 0.9.3 (macOS arm64).
const REAL_0_9_3: &[u8] =
    include_bytes!("../../../../profile/tests/fixtures/stetdoc-0.9.3-skills-get-core.md");

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

/// The profile for a macOS bundle under `scratch`, with the stand-in in
/// `Contents/Helpers/stetdoc` when `script` is given.
fn macos_bundle(scratch: &Scratch, stand_in: Option<(&[u8], &[u8])>) -> DesktopProfile {
    scratch.dir("Clauseo.app/Contents/MacOS");
    if let Some((script, skill)) = stand_in {
        stetdoc_at(
            scratch,
            "Clauseo.app/Contents/Helpers/stetdoc",
            script,
            skill,
        );
    }
    let layout = Layout {
        exe: scratch.path().join("Clauseo.app/Contents/MacOS/Clauseo"),
        macos: true,
        resource_dir: None,
        shell_browser: clauseo_profile::servers::shell_browser(),
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
/// the first limit (`start::FIRST_LIMIT`, 3 s) all told, and stays on without its skill.
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
        .recv_timeout(std::time::Duration::from_secs(7))
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
    let layout = Layout {
        exe: scratch.path().join("app").join("clauseo"),
        macos: false,
        resource_dir: Some(&resources),
        shell_browser: clauseo_profile::servers::shell_browser(),
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

/// A stand-in whose first `--version` takes 5 s (as a new program's first run while the OS
/// scans it), its later runs as `then` says: `ok` answers as [`STUB`], `fail` exits 3. Its
/// second run (the background check's `--version`) waits 1.5 s first, so a test sees the
/// state the build left before the check's answer lands.
fn slow_once(then: &str) -> Vec<u8> {
    format!(
        "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
         if [ ! -e \"$d/ran\" ]; then : > \"$d/ran\"; sleep 5\n\
         elif [ ! -e \"$d/ran2\" ]; then : > \"$d/ran2\"; sleep 1.5; fi\n\
         [ {then} = fail ] && exit 3\n\
         if [ \"$1\" = --version ]; then echo 'stetdoc 0.9.3'\n\
         elif [ \"$*\" = 'skills get core' ]; then cat \"$d/skill.txt\"\n\
         else exit 2; fi\n"
    )
    .into_bytes()
}

/// [`macos_bundle`] on a thread, which must return within 7 s: the first check stops at
/// the first limit (`start::FIRST_LIMIT`, 3 s).
fn bundle_within_7_s(
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
    rx.recv_timeout(std::time::Duration::from_secs(7))
        .expect("the build returns within 7 s")
}

/// Waits up to 10 s, in steps of 100 ms, for `done` to hold of `hook`: the background check's
/// answer lands about 1.5 s after the build.
fn eventually(hook: &DesktopProfile, done: impl Fn(&DesktopProfile) -> bool) -> bool {
    // A channel whose sender stays alive: each `recv_timeout` waits out its step.
    let (_tx, rx) = std::sync::mpsc::channel::<()>();
    for _ in 0..100 {
        if done(hook) {
            return true;
        }
        let _waited = rx.recv_timeout(std::time::Duration::from_millis(100));
    }
    done(hook)
}

/// A stetdoc whose first `--version` passes the first limit (`start::FIRST_LIMIT`, 3 s) stays
/// on at once: its folder on `PATH`, the blackline's program, and the instructions' stetdoc
/// line, with no skill (none cached). The background check that answers brings the skill to
/// the next agent start.
#[test]
fn a_stetdoc_slow_on_its_first_run_stays_on_and_gets_its_skill_later() {
    let (hook, scratch) =
        bundle_within_7_s(slow_once("ok"), REAL_0_9_3, "profile-release-slow-once");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
    assert!(
        eventually(&hook, |hook| written_skill(hook).is_some()),
        "the background check brings the skill"
    );
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}

/// A stetdoc whose first `--version` passes the limit and whose background check fails for
/// real: on at once, then the agents are no longer told about it. Its `PATH` entry and the
/// blackline's program were given to the host at its start and stay until the app restarts.
#[test]
fn a_stetdoc_slow_then_broken_is_no_longer_told_to_the_agents() {
    let (hook, scratch) = bundle_within_7_s(slow_once("fail"), b"", "profile-release-slow-broken");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
    assert!(
        eventually(&hook, |hook| both_have_the_stetdoc_line(hook) == [false; 2]),
        "the background check takes the stetdoc line out"
    );
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.stetdoc(), Some(program.as_path()));
}

/// A stetdoc that hangs on `--version` stays on at once, without a skill, its stetdoc line in
/// the instructions; the background check (30 s) is not waited for here.
#[test]
fn a_stetdoc_that_hangs_on_its_version_stays_on_for_now() {
    let script = b"#!/bin/sh\nsleep 60\n".to_vec();
    let (hook, scratch) = bundle_within_7_s(script, REAL_0_9_3, "profile-release-hangs");
    let program = helpers_stetdoc(&scratch);
    assert_eq!(written_skill(&hook), None);
    assert_eq!(hook.path_prepend(), folder_of(&program));
    assert_eq!(both_have_the_stetdoc_line(&hook), [true; 2]);
}
