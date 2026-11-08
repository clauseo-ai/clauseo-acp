//! Codex's index with packs, and Claude's `_meta` with the pack plugin.

use std::path::{Path, PathBuf};

use clauseo_profile::packs::index::{DESCRIPTION_CHARS, INDEX_CHARS, INDEX_ENTRIES, INDEX_WORDS};
use clauseo_profile::packs::{self, PacksWritten};
use clauseo_profile::skills::builtin::{BUILTIN, code_span};
use clauseo_profile::skills::stetdoc::STETDOC;
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths, SkillsWritten, write_skills};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;

use super::{Scratch, entry, manifest_json, skill_md};

fn abs(path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{path}"))
    } else {
        PathBuf::from(path)
    }
}

/// The launch profile under `root`, with stetdoc's skill and the three built-ins.
fn launch_profile(root: &Path) -> ClauseoProfile {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        abs("/app/clauseo"),
        abs("/app/stetdoc/stetdoc"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    )
    .unwrap();
    let stetdoc = clauseo_profile::skills::stetdoc::skill_source(
        "stetdoc 0.9.1",
        b"---\ndescription: Edit Word files as tracked changes.\n---\n# stetdoc\n",
    )
    .unwrap();
    let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(Some(stetdoc));
    assert!(refused.is_empty(), "{refused:?}");
    profile
}

/// The built-in plugin written, for its token.
fn builtin_written(profile: &ClauseoProfile) -> SkillsWritten {
    let writes = profile.launch_writes(Some(AgentKind::Codex));
    write_skills(&writes[0]).unwrap()
}

/// Packs `kit-<n>` for each count in `skills`, each skill described by `description`, loaded
/// and written into a launch plugin.
fn packs_written(scratch: &Scratch, skills: &[usize], description: &str) -> PacksWritten {
    let mut entries = Vec::new();
    for (n, count) in skills.iter().enumerate() {
        let id = format!("kit-{n}");
        let names: Vec<String> = (0..*count).map(|s| format!("s{s}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        scratch.put(
            &id,
            "clauseo-pack.json",
            manifest_json(&id, &names, true).as_bytes(),
        );
        for name in &names {
            scratch.put(
                &id,
                &format!("skills/{name}/SKILL.md"),
                skill_md(name, description).as_bytes(),
            );
        }
        entries.push(entry(&scratch.inspect(&id).unwrap()));
    }
    let loaded = packs::load(&scratch.packs(), &entries, false);
    assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
    packs::write_packs(&scratch.join("run/s1/skill-packs"), &loaded.packs).unwrap()
}

/// Codex's `developer_instructions`.
fn codex_text(
    profile: &ClauseoProfile,
    written: Option<&SkillsWritten>,
    packs: Option<&PacksWritten>,
) -> String {
    let keys = profile.codex_keys_with_packs(Some(AgentKind::Codex), written, packs);
    keys["developer_instructions"].as_str().unwrap().to_owned()
}

/// The index part of Codex's instructions.
fn index_of(text: &str) -> &str {
    text.find("## Skills").map_or("", |at| &text[at..])
}

/// The `- name` entries of an index, in order.
fn entries_of(index: &str) -> Vec<String> {
    index
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|line| line.split([' ', ':']).next().unwrap().to_owned())
        .collect()
}

#[test]
fn the_index_puts_builtins_first_and_stops_at_40_entries_or_600_words() {
    let scratch = Scratch::new("index-budget");
    let profile = launch_profile(&scratch);
    let written = builtin_written(&profile);
    // 64 short skills: the entry cap, or with this test folder's long paths the character
    // cap, binds first. (The entry cap alone is tested beside the index, with short paths.)
    let packs = packs_written(&scratch, &[32, 32], "Short.");
    let text = codex_text(&profile, Some(&written), Some(&packs));
    let index = index_of(&text);
    let names = entries_of(index);
    let chars = index.chars().count();
    assert!(chars <= INDEX_CHARS, "{chars}");
    assert!(
        names.len() == INDEX_ENTRIES || (names.len() < INDEX_ENTRIES && chars > INDEX_CHARS - 200),
        "{} entries, {chars} characters",
        names.len()
    );
    let builtins: Vec<&str> = [STETDOC].into_iter().chain(BUILTIN).collect();
    assert_eq!(&names[..4], builtins.as_slice());
    assert_eq!(names[4], "kit-0--s0");
    assert!(index.split_whitespace().count() <= INDEX_WORDS);
    // Long descriptions: the word cap binds first, and each is cut to 24 words.
    let scratch = Scratch::new("index-words");
    let profile = launch_profile(&scratch);
    let written = builtin_written(&profile);
    let long = vec!["word"; 60].join(" ");
    let packs = packs_written(&scratch, &[32], &long);
    let index = index_of(&codex_text(&profile, Some(&written), Some(&packs))).to_owned();
    let names = entries_of(&index);
    assert!(
        names.len() < INDEX_ENTRIES && names.len() > 4,
        "{}",
        names.len()
    );
    assert!(index.split_whitespace().count() <= INDEX_WORDS);
    let line = index
        .lines()
        .find(|line| line.starts_with("- kit-0--s0"))
        .unwrap();
    assert!(
        line.contains(&format!("({}…)", vec!["word"; 24].join(" "))),
        "{line}"
    );
}

/// The reviewer's case (D3): 40 descriptions of 1,000 characters with no whitespace passed both
/// word cuts. Each is now cut to 200 characters, and the index stops at 4,800 characters, the
/// built-in lines first and whole.
#[test]
fn review_index_word_cut_is_bypassed_without_whitespace() {
    let scratch = Scratch::new("index-chars");
    let profile = launch_profile(&scratch);
    let written = builtin_written(&profile);
    let long: String = "clause-".repeat(143).chars().take(1000).collect();
    assert!(!long.contains(char::is_whitespace) && long.chars().count() == 1000);
    let packs = packs_written(&scratch, &[32, 8], &long);
    let index = index_of(&codex_text(&profile, Some(&written), Some(&packs))).to_owned();
    assert!(
        index.chars().count() <= INDEX_CHARS,
        "{}",
        index.chars().count()
    );
    assert!(index.split_whitespace().count() <= INDEX_WORDS);
    let names = entries_of(&index);
    assert!(names.len() > 4 && names.len() <= INDEX_ENTRIES, "{names:?}");
    // The built-in lines are 0031's, whole, and first.
    let builtin = index_of(&codex_text(&profile, Some(&written), None)).to_owned();
    assert!(index.starts_with(builtin.trim_end()), "{index}");
    for line in index.lines().filter(|line| line.starts_with("- kit-")) {
        let text = line
            .split_once(" (")
            .unwrap()
            .1
            .split_once("): ")
            .unwrap()
            .0;
        assert!(text.chars().count() <= DESCRIPTION_CHARS, "{line}");
        assert!(text.ends_with('…'), "{line}");
        assert!(long.starts_with(text.trim_end_matches('…')), "{line}");
    }
}

/// A control or format character is filtered out at the render, so neither agent sees it
/// (`a_description_reaches_claude_without_control_or_format_characters`); a description the
/// filter still refuses, one holding a host placeholder, gets no use text.
#[test]
fn a_pack_description_with_a_control_or_format_character_gets_no_use_text() {
    let scratch = Scratch::new("index-filter");
    let profile = launch_profile(&scratch);
    for (description, use_text) in [
        ("Hidden\u{200B}text.", Some("Hiddentext.")),
        ("Right-to-left \u{202E}txet.", Some("Right-to-left txet.")),
        ("Fill {work_folder} in.", None),
    ] {
        let packs = packs_written(&scratch, &[1], description);
        let index = index_of(&codex_text(&profile, None, Some(&packs))).to_owned();
        let path = packs.skills_root().join("kit-0--s0").join("SKILL.md");
        let span = code_span(path.to_str().unwrap());
        let expected = match use_text {
            Some(text) => format!("- kit-0--s0 ({text}): {span}"),
            None => format!("- kit-0--s0: {span}"),
        };
        assert!(
            index.lines().any(|line| line == expected),
            "{description:?}: {index}"
        );
        support_clean(&scratch);
    }
}

/// Review nit: variation selectors (U+FE00 to U+FE0F, U+E0100 to U+E01EF) are not format
/// characters, but the filter refuses them too, so they reach neither agent.
#[test]
fn a_variation_selector_is_filtered_like_a_format_character() {
    let scratch = Scratch::new("index-selectors");
    let profile = launch_profile(&scratch);
    let selectors = ['\u{FE00}', '\u{FE0F}', '\u{E0100}', '\u{E01EF}'];
    let description = "Re\u{FE00}views\u{FE0F} N\u{E0100}DAs\u{E01EF}.\u{FE0E}";
    let packs = packs_written(&scratch, &[1], description);
    assert_eq!(packs.skills()[0].description, "Reviews NDAs.");
    let rendered =
        std::fs::read_to_string(packs.skills_root().join("kit-0--s0").join("SKILL.md")).unwrap();
    assert!(!rendered.contains(selectors), "{rendered:?}");
    let index = index_of(&codex_text(&profile, None, Some(&packs))).to_owned();
    assert!(index.contains("- kit-0--s0 (Reviews NDAs.): "), "{index}");
    // Just outside the ranges is ordinary text.
    super::support::remove_dir_all(&scratch.join("run")).unwrap();
    let packs = packs_written(&scratch, &[1], "Edge\u{FDFF}\u{FE10}\u{E01F0}.");
    assert_eq!(
        packs.skills()[0].description,
        "Edge\u{FDFF}\u{FE10}\u{E01F0}."
    );
}

/// Removes the launch plugin and the packs, so the next round writes them afresh.
fn support_clean(scratch: &Scratch) {
    super::support::remove_dir_all(&scratch.join("run")).unwrap();
    super::support::remove_dir_all(&scratch.packs().join("kit-0")).unwrap();
}

#[test]
fn the_pack_index_appears_only_with_a_packs_written_token() {
    let scratch = Scratch::new("index-token");
    let profile = launch_profile(&scratch);
    let written = builtin_written(&profile);
    let packs = packs_written(&scratch, &[2], "Reviews NDAs.");
    let without = codex_text(&profile, Some(&written), None);
    assert!(!without.contains("kit-0--"), "{without}");
    // The built-in index alone is 0031's, byte for byte.
    assert_eq!(
        profile.codex_keys_with_packs(Some(AgentKind::Codex), Some(&written), None),
        profile.codex_keys(Some(AgentKind::Codex), &written)
    );
    let with = codex_text(&profile, Some(&written), Some(&packs));
    let path = packs.skills_root().join("kit-0--s0").join("SKILL.md");
    let line = format!(
        "- kit-0--s0 (Reviews NDAs.): {}",
        code_span(path.to_str().unwrap())
    );
    assert!(with.lines().any(|held| held == line), "{with}");
    // Packs with no built-in token: the pack lines alone. Neither: no index.
    let packs_only = codex_text(&profile, None, Some(&packs));
    assert!(packs_only.contains("kit-0--s1") && !packs_only.contains("legal-drafting ("));
    assert_eq!(
        profile.codex_keys_with_packs(Some(AgentKind::Codex), None, None),
        profile.codex_keys_without_index(Some(AgentKind::Codex))
    );
    assert!(
        profile
            .codex_keys_with_packs(Some(AgentKind::Claude), None, Some(&packs))
            .is_empty()
    );
}

#[test]
fn the_claude_meta_names_the_pack_plugin_second() {
    let scratch = Scratch::new("meta");
    let profile = launch_profile(&scratch);
    let packs = packs_written(&scratch, &[1], "Reviews NDAs.");
    let meta = profile
        .session_meta_with_packs(Some(AgentKind::Claude), Some(&packs))
        .unwrap();
    let plugins = meta["claudeCode"]["options"]["plugins"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(plugins.len(), 2);
    assert_eq!(
        plugins[0]["path"],
        profile.paths().claude_plugin().to_str().unwrap()
    );
    assert_eq!(plugins[1]["path"], packs.plugin().to_str().unwrap());
    assert_eq!(plugins[1]["type"], "local");
    // With no token, the meta is 0031's.
    assert_eq!(
        profile.session_meta_with_packs(Some(AgentKind::Claude), None),
        profile.session_meta(Some(AgentKind::Claude))
    );
    assert_eq!(
        profile.session_meta_with_packs(Some(AgentKind::Codex), Some(&packs)),
        None
    );
    // The settings still disable skill shell execution.
    assert_eq!(
        meta["claudeCode"]["options"]["settings"]["disableSkillShellExecution"],
        true
    );
}
