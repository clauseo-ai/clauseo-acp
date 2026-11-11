//! stetdoc as the agents get it: the command, told in the instructions' stetdoc line, with its
//! skill optional. stetdoc 0.9.3 prints its skill's body alone, no frontmatter
//! (`fixtures/stetdoc-0.9.3-skills-get-core.md`, captured from the release binary), so that
//! text is refused as a skill while its `--version` works.

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::instructions::{Instructions, STETDOC, STETDOC_CLI, stetdoc_part};
use clauseo_profile::skills::stetdoc::{self, load_skill, load_skill_at, skill_source};
use clauseo_profile::{ClauseoProfile, ProfileError};
use futures::executor::block_on;

use super::{Scratch, profile_paths, stub, write};

/// What `stetdoc skills get core` printed, byte for byte, from stetdoc 0.9.3 (macOS arm64).
const REAL_0_9_3: &[u8] = include_bytes!("../fixtures/stetdoc-0.9.3-skills-get-core.md");

#[test]
fn stetdoc_0_9_3s_body_only_text_is_refused_as_a_skill() {
    assert!(
        REAL_0_9_3.starts_with(b"# stetdoc"),
        "the fixture has no frontmatter"
    );
    let error = skill_source("stetdoc 0.9.3", REAL_0_9_3).unwrap_err();
    assert!(matches!(error, ProfileError::Skill { .. }), "{error:?}");
}

/// Through the binary: `--version` works, the skill alone fails, and the error says it is the
/// skill's text, not stetdoc, that failed.
#[test]
fn stetdoc_0_9_3s_version_works_and_only_its_skill_fails() {
    let root = Scratch::new("stetdoc-0.9.3");
    let bin = root.join("bin");
    write::fresh_dir(&bin).unwrap();
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let real = std::str::from_utf8(REAL_0_9_3).unwrap();
    let program = stub(&bin, "stetdoc 0.9.3\n", real);
    let version = block_on(stetdoc::version(&launcher, &AsyncIoTimer, &program)).unwrap();
    assert_eq!(version, "stetdoc 0.9.3");
    let skills = root.join("skills");
    let error = block_on(load_skill_at(
        &launcher,
        &AsyncIoTimer,
        &program,
        &version,
        &skills,
    ))
    .unwrap_err();
    assert!(matches!(error, ProfileError::Skill { .. }), "{error:?}");
    let error = block_on(load_skill(&launcher, &AsyncIoTimer, &program, &skills)).unwrap_err();
    assert!(matches!(error, ProfileError::Skill { .. }), "{error:?}");
}

#[test]
fn a_version_that_fails_or_is_empty_is_an_error_from_stetdoc() {
    let root = Scratch::new("stetdoc-no-version");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let program = stub(&root, "\n", "");
    let error = block_on(stetdoc::version(&launcher, &AsyncIoTimer, &program)).unwrap_err();
    assert!(matches!(error, ProfileError::Stetdoc { .. }), "{error:?}");
    write::remove_file(&root.join("stub-version.txt")).unwrap();
    let error = block_on(stetdoc::version(&launcher, &AsyncIoTimer, &program)).unwrap_err();
    assert!(matches!(error, ProfileError::Stetdoc { .. }), "{error:?}");
}

#[test]
fn the_stetdoc_line_is_in_the_instructions_only_when_stetdoc_was_found() {
    let line = "For Word documents, use the `stetdoc` command; run `stetdoc --help` and \
                `stetdoc <command> --help` to learn it.";
    assert_eq!(STETDOC_CLI.trim(), line);
    assert_eq!(stetdoc_part(true), format!(" {line}"));
    assert_eq!(stetdoc_part(false), "");
    let launch = Instructions::launch();
    let with = launch.compose_for("", true);
    let without = launch.compose_for("", false);
    assert!(
        with.contains(&format!("(`python` on Windows). {line} Edit Word files")),
        "{with}"
    );
    assert!(
        without.contains("(`python` on Windows). Edit Word files"),
        "{without}"
    );
    assert_eq!(launch.compose(""), without);
    for text in [&with, &without] {
        assert!(!text.contains(STETDOC), "{text}");
    }
    let root = Scratch::new("stetdoc-line");
    let profile = ClauseoProfile::new(profile_paths(&root), None);
    assert!(!profile.instructions().contains(line));
    assert!(profile.with_stetdoc_cli(true).instructions().contains(line));
}
