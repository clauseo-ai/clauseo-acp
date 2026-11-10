//! A release bundle laid out in a scratch folder, its stetdoc a stand-in. The agents get stetdoc
//! as a command: its folder first on their `PATH` and the stetdoc line in their instructions,
//! whenever its `--version` works, with its skill or without. stetdoc 0.9.3's skill text (the
//! body alone, no frontmatter) is refused, and only the skill is left out. A stetdoc whose
//! `--version` fails is treated as absent.

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

/// stetdoc's skill fails after a working `--version` (the real 0.9.3 body-only text, a
/// frontmatter with no description, bytes that are not UTF-8, a `skills get core` that exits
/// with an error): the skill is left out, and stetdoc stays on for the agents and the app.
#[test]
fn a_release_bundle_whose_skill_fails_keeps_stetdoc_as_a_command() {
    let failing: [(&[u8], &[u8]); 4] = [
        (STUB, REAL_0_9_3),
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
/// `STETDOC_START_LIMIT` all told, and stays on without its skill.
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

/// Windows and Linux: `<resources>/stetdoc/stetdoc`, Tauri's resource folder.
#[test]
fn a_resource_folder_release_layout_keeps_stetdoc_as_a_command() {
    let scratch = Scratch::new("profile-release-resources", &["app"]);
    let program = stetdoc_at(&scratch, "resources/stetdoc", STUB, REAL_0_9_3);
    let resources = scratch.path().join("resources");
    let layout = Layout {
        exe: scratch.path().join("app").join("clauseo"),
        macos: false,
        resource_dir: Some(&resources),
    };
    let locations = Locations::under_root(&scratch.dir("data"));
    let hook = DesktopProfile::build_in(&locations, &layout, None).unwrap();
    assert_eq!(written_skill(&hook), None);
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

/// A stetdoc whose `--version` exits with an error, prints no version or hangs past
/// `STETDOC_START_LIMIT` is treated as absent, as is a bundle with no stetdoc: no skill, no
/// `PATH` entry and no stetdoc line.
#[test]
fn a_missing_stetdoc_or_one_whose_version_fails_is_absent_for_the_agents() {
    let broken: [Option<&[u8]>; 4] = [
        None,
        Some(b"#!/bin/sh\nexit 3\n"),
        Some(b"#!/bin/sh\nif [ \"$1\" = --version ]; then echo; else echo '# stetdoc'; fi\n"),
        Some(b"#!/bin/sh\nsleep 60\n"),
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
