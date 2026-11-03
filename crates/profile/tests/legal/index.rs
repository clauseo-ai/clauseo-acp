//! Codex's skills index (decision record 0031): what it lists, how it quotes each path, what it
//! leaves out, its budget, and the Codex keys without it.

use std::path::Path;

use clauseo_profile::ClauseoProfile;
use clauseo_profile::instructions::word_count;
use clauseo_profile::placeholders::{IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER};
use clauseo_profile::skills::builtin::{BUILTIN, code_span, codex_index, sources};
use clauseo_profile::skills::stetdoc::STETDOC;
use clauseo_profile::skills::{SKILL_MD, render};
use clauseo_protocol::AgentKind;

use super::{launch_profile, paths_under};

#[test]
fn the_index_lists_every_rendered_skill_within_150_words() {
    let profile = launch_profile(Path::new("/data"));
    let index = profile.skills_index();
    assert!(word_count(&index) <= 150, "{} words", word_count(&index));
    for name in [STETDOC].into_iter().chain(BUILTIN) {
        let path = profile.paths().claude_skills().join(name).join(SKILL_MD);
        assert!(
            index.contains(&code_span(&path.to_string_lossy())),
            "{name}"
        );
    }
    let claude = profile.claude_options().meta_fragment();
    assert!(
        !serde_json::to_string(&claude)
            .unwrap()
            .contains("## Skills")
    );
    insta::assert_snapshot!(index.replace('\\', "/"));
}

#[test]
fn no_skill_rendered_means_no_index() {
    let profile = ClauseoProfile::new(paths_under(Path::new("/data")), None);
    assert_eq!(profile.skills_index(), "");
    let keys = profile.codex_keys_without_index(Some(AgentKind::Codex));
    assert_eq!(
        keys["developer_instructions"],
        profile.instructions().as_str()
    );
}

#[test]
fn the_index_quotes_any_path_as_one_code_span() {
    let rendered = render(&sources().unwrap()).unwrap();
    for root in [
        r#"/Users/A `B` "q" 'r'/Application Support/skills"#,
        r"C:\Users\Ann O'Neil\AppData\Roaming\Clau``seo\skills",
        "`/edge ",
    ] {
        let index = codex_index(Path::new(root), &rendered);
        for name in BUILTIN {
            let path = Path::new(root).join(name).join(SKILL_MD);
            let path = path.to_string_lossy();
            let line = index
                .lines()
                .find(|line| line.starts_with(&format!("- {name} (")))
                .unwrap();
            let span = line.split_once("): ").unwrap().1;
            assert_eq!(span, code_span(&path), "{root}");
            assert_eq!(unspan(span), path, "{root}");
        }
    }
}

/// The text of a code span per `CommonMark`: strip the fences, then one space from each side when
/// both sides hold one and the text is not all spaces.
fn unspan(span: &str) -> String {
    let fence = span.chars().take_while(|c| *c == '`').count();
    let inner = &span[fence..span.len() - fence];
    let longest_run = inner.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    assert!(longest_run < fence, "{span}");
    if inner.len() >= 2 && inner.starts_with(' ') && inner.ends_with(' ') && inner.trim() != "" {
        inner[1..inner.len() - 1].to_owned()
    } else {
        inner.to_owned()
    }
}

#[test]
fn a_path_holding_a_placeholder_or_a_line_break_is_left_out_of_the_index() {
    let rendered = render(&sources().unwrap()).unwrap();
    assert_eq!(codex_index(Path::new("/x/{work_folder}"), &rendered), "");
    for odd in [
        "\n",
        "\r",
        "\u{0b}",
        "\u{0c}",
        "\u{85}",
        "\u{2028}",
        "\u{2029}",
        "\u{1b}",
        // Format (Cf) characters: bidi controls, zero-width, BOM, soft hyphen, a tag.
        "\u{202e}",
        "\u{2066}",
        "\u{200b}",
        "\u{200d}",
        "\u{feff}",
        "\u{ad}",
        "\u{e0041}",
    ] {
        let root = format!("/x/a{odd}b");
        assert_eq!(codex_index(Path::new(&root), &rendered), "", "{odd:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_left_out_of_the_index() {
    use std::os::unix::ffi::OsStrExt as _;
    let rendered = render(&sources().unwrap()).unwrap();
    let root = Path::new(std::ffi::OsStr::from_bytes(b"/x/\xff/skills"));
    assert_eq!(codex_index(root, &rendered), "");
}

/// A macOS data folder whose path holds spaces, with the host's per-session folders filled.
#[test]
fn the_budgets_hold_for_a_long_macos_path_with_the_folders_filled() {
    let data = "/Users/Ann Mary/Library/Application Support/Clauseo";
    let profile = launch_profile(Path::new(data));
    let index = profile.skills_index();
    assert!(word_count(&index) <= 150, "{} words", word_count(&index));
    let work = format!("{data}/run/0b7e3c1a-5d2f-4e8a-9c61-2f4d8e7a1b30/work");
    let notes = format!("{data}/agent-state/matter-4821/notes");
    let fill = |text: &str| {
        clauseo_profile::placeholders::fill(
            text,
            &IN_INSTRUCTIONS,
            &[(WORK_FOLDER, &work), (MATTER_NOTES, &notes)],
        )
        .unwrap()
    };
    let shared = fill(&profile.instructions());
    assert!(word_count(&shared) <= 1500, "{} words", word_count(&shared));
    // What `codex_keys` sends after a successful write (checked on disk in `tests/skills`).
    let codex = fill(&format!("{}\n\n{index}", profile.instructions()));
    assert!(word_count(&codex) <= 1500, "{} words", word_count(&codex));
}

#[test]
fn the_codex_keys_without_index_are_the_instructions_alone() {
    let profile = launch_profile(Path::new("/data"));
    let keys = profile.codex_keys_without_index(Some(AgentKind::Codex));
    assert_eq!(
        keys["developer_instructions"],
        profile.instructions().as_str()
    );
    assert!(
        profile
            .codex_keys_without_index(Some(AgentKind::Claude))
            .is_empty()
    );
    assert!(profile.codex_keys_without_index(None).is_empty());
}
