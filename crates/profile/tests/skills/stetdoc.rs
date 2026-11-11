//! stetdoc as the agents get it: the command, told in the instructions' stetdoc line, with its
//! skill optional. stetdoc 0.9.3 prints its skill's body alone, no frontmatter
//! (`fixtures/stetdoc-0.9.3-skills-get-core.md`, captured from the macOS release binary; the
//! Windows one prints it with mostly CRLF line ends), so Clauseo writes the frontmatter: the
//! description is its first `# ` heading, the body its text, byte for byte.

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::instructions::{
    Instructions, STETDOC, STETDOC_CLI, TOOLS, stetdoc_part, word_count,
};
use clauseo_profile::skills::stetdoc::{
    self, FALLBACK_DESCRIPTION, STETDOC as SKILL, load_skill, load_skill_at, skill_source,
};
use clauseo_profile::skills::{self, render, write_rendered};
use clauseo_profile::{ClauseoProfile, ProfileError, SkillSource};
use futures::executor::block_on;

use super::{Scratch, profile_paths, stub, write};

/// What `stetdoc skills get core` printed, byte for byte, from stetdoc 0.9.3 (macOS arm64).
const REAL_0_9_3: &[u8] = include_bytes!("../fixtures/stetdoc-0.9.3-skills-get-core.md");

/// stetdoc 0.9.3's first line, which becomes the skill's description.
const HEADING: &str = "stetdoc: editing Word documents with tracked changes";

/// `SKILL.md`'s text as the profile renders it.
#[cfg(test)]
fn skill_text(skill: &SkillSource) -> String {
    String::from_utf8(skill.files[0].bytes.clone()).unwrap()
}

/// The frontmatter Clauseo writes for a text with none, with `description`.
#[cfg(test)]
fn own_front(description: &str) -> String {
    format!("---\nname: stetdoc\ndescription: \"{description}\"\n---\n")
}

/// The fixture as the Windows binary prints it (a laptop's capture): CRLF line ends except for
/// lines 216-235 and 279-293 (1-based), which end in a bare LF; the last line ends in CRLF.
#[cfg(test)]
fn windows_0_9_3() -> Vec<u8> {
    let text = std::str::from_utf8(REAL_0_9_3).unwrap();
    let mut out = String::new();
    for (at, line) in text.split_inclusive('\n').enumerate() {
        let number = at + 1;
        let bare = (216..=235).contains(&number) || (279..=293).contains(&number);
        match line.strip_suffix('\n') {
            Some(line) if !bare => {
                out.push_str(line);
                out.push_str("\r\n");
            }
            _ => out.push_str(line),
        }
    }
    out.into_bytes()
}

#[test]
fn stetdoc_0_9_3s_body_only_text_loads_under_its_first_heading() {
    assert!(
        REAL_0_9_3.starts_with(b"# stetdoc"),
        "the fixture has no frontmatter"
    );
    let text = std::str::from_utf8(REAL_0_9_3).unwrap();
    assert!(
        text.lines().skip(1).any(|line| line == "---"),
        "a rule mid-body"
    );
    let loaded = skill_source("stetdoc 0.9.3", REAL_0_9_3).unwrap();
    assert_eq!(skill_text(&loaded), format!("{}{text}", own_front(HEADING)));
    assert_eq!(loaded.version, "stetdoc 0.9.3");
    assert!(render(&[loaded]).is_ok());
}

/// The Windows output (mixed CRLF and LF, a `---` rule mid-body) loads with the same
/// description, its body byte for byte; with a BOM too. Written and read back from the cache,
/// it is current, and loads the same, so `skills get core` is not run again on each start.
#[test]
fn stetdoc_0_9_3s_windows_text_loads_the_same_and_stays_cached() {
    let windows = windows_0_9_3();
    assert_eq!(windows.split(|&byte| byte == b'\n').count(), 300);
    assert_eq!(
        windows.windows(2).filter(|pair| pair == b"\r\n").count(),
        264
    );
    assert!(windows.ends_with(b"\r\n"));
    let front = own_front(HEADING).into_bytes();
    let loaded = skill_source("stetdoc 0.9.3", &windows).unwrap();
    assert_eq!(loaded.files[0].bytes, [front.as_slice(), &windows].concat());
    let with_bom = [b"\xef\xbb\xbf".as_slice(), &windows].concat();
    assert_eq!(skill_source("stetdoc 0.9.3", &with_bom).unwrap(), loaded);

    let root = Scratch::new("stetdoc-windows-cache");
    let cached = root.join("skills");
    write_rendered(&cached, &render(std::slice::from_ref(&loaded)).unwrap()).unwrap();
    assert!(skills::is_current(&cached, SKILL, "stetdoc 0.9.3").unwrap());
    // A stetdoc whose `skills get core` would print something else: the cache is read.
    let bin = root.join("bin");
    write::fresh_dir(&bin).unwrap();
    let program = stub(&bin, "stetdoc 0.9.3\n", "# other\n");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let again = block_on(load_skill(&launcher, &AsyncIoTimer, &program, &cached)).unwrap();
    assert_eq!(again, loaded);
}

/// A text with no heading near its top gets the fixed description; stetdoc's own frontmatter
/// still wins, LF or CRLF; text that is not UTF-8 is refused.
#[test]
fn a_text_with_no_heading_gets_the_fallback_and_stetdocs_frontmatter_wins() {
    // A second-level heading is not the title.
    let plain = "Use stetdoc.\n\n## Commands\n";
    let loaded = skill_source("stetdoc 1", plain.as_bytes()).unwrap();
    assert_eq!(
        skill_text(&loaded),
        format!("{}{plain}", own_front(FALLBACK_DESCRIPTION))
    );
    let late = "a\nb\nc\nd\ne\n# Too late\n";
    let loaded = skill_source("stetdoc 1", late.as_bytes()).unwrap();
    assert!(skill_text(&loaded).starts_with(&own_front(FALLBACK_DESCRIPTION)));
    let crlf_heading = "\r\n#  Edits Word.  \r\nbody\r\n";
    let loaded = skill_source("stetdoc 1", crlf_heading.as_bytes()).unwrap();
    assert_eq!(
        skill_text(&loaded),
        format!("{}{crlf_heading}", own_front("Edits Word."))
    );
    // An empty heading is passed over for the next one.
    let empty_first = "# \n#\n# Edits Word.\nbody\n";
    let loaded = skill_source("stetdoc 1", empty_first.as_bytes()).unwrap();
    assert_eq!(
        skill_text(&loaded),
        format!("{}{empty_first}", own_front("Edits Word."))
    );

    let own = "---\nname: other\ndescription: Edit Word files.\n---\n# stetdoc\n";
    let expected = "---\nname: stetdoc\ndescription: \"Edit Word files.\"\n---\n# stetdoc\n";
    assert_eq!(
        skill_text(&skill_source("v", own.as_bytes()).unwrap()),
        expected
    );
    // A CRLF frontmatter is read with LF ends; the body after it stays byte for byte, a
    // `---` rule in it included.
    let own_crlf = "---\r\nname: other\r\ndescription: Edit Word files.\r\n---\r\n\
                    # stetdoc\r\n---\r\nmixed\nends\r\n";
    let expected_crlf = "---\nname: stetdoc\ndescription: \"Edit Word files.\"\n---\n\
                         # stetdoc\r\n---\r\nmixed\nends\r\n";
    let bom_crlf = format!("\u{feff}{own_crlf}");
    for text in [own_crlf, bom_crlf.as_str()] {
        assert_eq!(
            skill_text(&skill_source("v", text.as_bytes()).unwrap()),
            expected_crlf
        );
    }
    // Read back from that form (as from the cache), it is the same.
    assert_eq!(
        skill_text(&skill_source("v", expected_crlf.as_bytes()).unwrap()),
        expected_crlf
    );
    // A frontmatter of stetdoc's with no description is still refused, not given a heading's.
    let error = skill_source("v", b"---\nname: stetdoc\n---\n# stetdoc\n").unwrap_err();
    assert!(matches!(error, ProfileError::Skill { .. }), "{error:?}");
    let error = skill_source("v", b"\xff\xfe not UTF-8\n").unwrap_err();
    assert!(matches!(error, ProfileError::Skill { .. }), "{error:?}");
}

/// Through the binary: `--version` works and 0.9.3's text loads as the skill.
#[test]
fn stetdoc_0_9_3s_version_works_and_its_skill_loads() {
    let root = Scratch::new("stetdoc-0.9.3");
    let bin = root.join("bin");
    write::fresh_dir(&bin).unwrap();
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let real = std::str::from_utf8(REAL_0_9_3).unwrap();
    let program = stub(&bin, "stetdoc 0.9.3\n", real);
    let version = block_on(stetdoc::version(&launcher, &AsyncIoTimer, &program)).unwrap();
    assert_eq!(version, "stetdoc 0.9.3");
    let skills = root.join("skills");
    let at = block_on(load_skill_at(
        &launcher,
        &AsyncIoTimer,
        &program,
        &version,
        &skills,
    ))
    .unwrap();
    assert_eq!(skill_text(&at), format!("{}{real}", own_front(HEADING)));
    let loaded = block_on(load_skill(&launcher, &AsyncIoTimer, &program, &skills)).unwrap();
    assert_eq!(loaded, at);
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
    let line = "For Word documents, use the `stetdoc` command; learn it from `stetdoc skills \
                get core` and `stetdoc <command> --help`. Tracked changes are only for changing a \
                paper that already exists. Make a new document clean: write it as a Markdown file \
                in `{work_folder}`, then run `pandoc \"{work_folder}/<file>.md\" -o \
                \"<Name>.docx\"` into the matter. Never pipe into pandoc. Headings and numbered \
                paragraphs in the Markdown become Word styles. Then delete the Markdown file.";
    assert_eq!(STETDOC_CLI.trim(), line);
    assert!(
        word_count(TOOLS) + word_count(line) < 300,
        "the tools part with the stetdoc line"
    );
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
