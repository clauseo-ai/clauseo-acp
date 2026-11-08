//! Which section a misnamed policy name is about, by its first segment.

use super::{about_section, distance, misnamed_as, misspelt, starts_near};
use crate::leaf::SECURITY_KEYS;
use crate::schema::SECTIONS;

/// The sections that hold a security key, from [`SECURITY_KEYS`].
fn security_sections() -> Vec<&'static str> {
    let mut sections: Vec<&str> = SECURITY_KEYS
        .iter()
        .map(|key| key.split('.').next().unwrap_or(key))
        .collect();
    sections.dedup();
    sections
}

// A new section near a known one would read as a misspelling of it: this fails first.
#[test]
fn no_two_known_sections_are_within_the_misspelling_distance() {
    for (at, a) in SECTIONS.iter().enumerate() {
        for b in &SECTIONS[at + 1..] {
            assert!(distance(a, b) > misspelt(a).max(misspelt(b)), "{a} and {b}");
        }
    }
    for section in security_sections() {
        assert!(SECTIONS.contains(&section), "{section}");
    }
    // Nor does any known section start like, or hold, another security section's name, so the
    // shape-free rule would not read one as another either.
    for known in SECTIONS {
        for section in security_sections() {
            if known != section {
                assert!(!known.contains(section), "{known} holds {section}");
                assert!(
                    !starts_near(known, section),
                    "{known} starts like {section}"
                );
            }
        }
    }
}

#[test]
fn the_edit_distance_counts_inserts_deletes_and_replacements() {
    assert_eq!(distance("skills", "skills"), 0);
    assert_eq!(distance("skill", "skills"), 1);
    assert_eq!(distance("skils", "skills"), 1);
    assert_eq!(distance("instal", "install"), 1);
    assert_eq!(distance("tols", "tools"), 1);
    assert_eq!(distance("tooos", "tools"), 1);
    assert_eq!(distance("colour", "tools"), 4);
    assert_eq!(distance("", "mcp"), 3);
}

#[test]
fn only_the_first_segment_decides_the_section() {
    for (name, section) in [
        ("skill", "skills"),
        ("skill.extra", "skills"),
        ("skils", "skills"),
        ("Skills", "skills"),
        ("tool", "tools"),
        ("Tools.browser", "tools"),
        ("instal", "install"),
        ("mcpServers", "mcp"),
        ("skills.enbld", "skills"),
        ("clauseo.skills.extra", "skills"),
        ("Clauseo.skills.allow_scripts", "skills"),
        ("x.skils.extra", "skills"),
        ("firm.Tools.browser", "tools"),
        ("clauseo.skills_extra", "skills"),
        ("clauseo.skills-extra", "skills"),
        // Deliberate over-locks, failing closed: a prefixed name holding a section's name.
        ("foo.tools_note", "tools"),
        ("foo.bar.tools_note", "tools"),
    ] {
        assert!(about_section(name, section), "{name}");
    }
    for (name, section) in [
        ("colour", "tools"),
        ("colour", "skills"),
        // A known section is only itself, however close.
        ("plans", "skills"),
        ("ui", "tools"),
    ] {
        assert!(!about_section(name, section), "{name}");
    }
}

// The host's review of #450: the rule counted bytes, so an invisible or look-alike character
// put a name out of reach.
#[test]
fn a_name_with_invisible_or_non_ascii_characters_is_about_every_section_it_is_compared_with() {
    assert_eq!(distance("ѕkіlls", "skills"), 2);
    assert_eq!(distance("é", "e"), 1);
    // A zero-width space, a word joiner, a soft hyphen: dropped before comparing.
    for name in [
        "skil\u{200b}ls",
        "\u{feff}skills.extra",
        "ski\u{2060}l\u{ad}ls",
    ] {
        assert!(about_section(name, "skills"), "{name:?}");
        assert!(!about_section(name, "tools"), "{name:?}");
    }
    // Anything else outside ASCII fails closed, for every section.
    for name in ["ѕkіlls", "x.tооls", "thème"] {
        for section in security_sections() {
            assert!(about_section(name, section), "{name:?} {section}");
        }
    }
}

// The host's review of #450: segments were split only at `.`, so a misspelt section joined to
// its key by another separator was out of reach.
#[test]
fn a_word_of_a_segment_split_at_other_separators_is_about_its_section() {
    for (name, section) in [
        ("skill_extra", "skills"),
        ("skill-extra", "skills"),
        ("skill allow_scripts", "skills"),
        ("tool_browser", "tools"),
        ("firm/tool", "tools"),
        ("x.t-o-o-l-s", "tools"),
        ("MCP-servers", "mcp"),
    ] {
        assert!(about_section(name, section), "{name}");
    }
    assert!(!about_section("colour_scheme", "tools"));
}

// The host's review of #450: two edits from a three-letter section reached ordinary words.
#[test]
fn a_short_section_is_one_edit_away_and_a_long_one_two() {
    assert_eq!(
        (misspelt("mcp"), misspelt("ui"), misspelt("tools")),
        (1, 1, 2)
    );
    assert_eq!(distance("mpc", "mcp"), 1);
    assert_eq!(distance("sklils", "skills"), 1);
    for name in ["app", "tmp", "foo.app", "tmp_dir"] {
        assert!(!about_section(name, "mcp"), "{name}");
    }
    for name in ["mpc", "mc", "mcpp", "firm.mpc_servers"] {
        assert!(about_section(name, "mcp"), "{name}");
    }
    assert!(about_section("tols", "tools") && about_section("tooos", "tools"));
}

// The host's re-check of #450: a section glued to its key, with no separator or another one,
// was out of reach.
#[test]
fn a_glued_name_is_about_the_section_it_starts_with() {
    for (name, section) in [
        ("SkillExtra", "skills"),
        ("skillextra", "skills"),
        ("skill:extra", "skills"),
        ("skill\\extra", "skills"),
        ("ToolBrowser", "tools"),
        ("tool:browser", "tools"),
        ("firmToolBrowser", "tools"),
        ("ｓｋｉｌｌ", "skills"),
    ] {
        assert!(about_section(name, section), "{name}");
    }
    for (name, section) in [
        ("colour", "tools"),
        ("telemetry", "tools"),
        ("theme", "skills"),
    ] {
        assert!(!about_section(name, section), "{name}");
    }
}

// The cost of the shape-free rule, on purpose: a word that starts like a section locks it.
#[test]
fn a_word_starting_like_a_section_locks_it_deliberately() {
    assert!(about_section("toolbar", "tools"));
    assert!(about_section("skillful", "skills"));
    // The over-locks firm-it.md §6 names.
    assert!(about_section("clauseo", "plans"));
    for name in ["cpu_limit", "map", "acp"] {
        assert!(about_section(name, "mcp"), "{name}");
    }
    assert!(about_section("instance_id", "install"));
}

// The orchestrator's review of #450: a known first segment hid the rest of the name, so a
// security section inside `ui` locked nothing. What follows a known section is read by the
// same rule as a name with an unknown first segment.
#[test]
fn what_follows_a_known_section_is_read_by_the_rule() {
    for (name, section) in [
        ("ui.tools", "tools"),
        ("ui.tools.browser", "tools"),
        ("ui.skills.extra", "skills"),
        ("ui.tools_note", "tools"),
        ("ui.tool_browser", "tools"),
        ("matters.install_bundle", "install"),
        ("privacy.mcp", "mcp"),
        ("matters.SkillExtra", "skills"),
        ("ui.x.mpc", "mcp"),
    ] {
        assert!(about_section(name, section), "{name}");
    }
    assert!(!about_section("ui.colour", "tools"));
    assert!(!about_section("matters.app", "mcp"));
}

// Every key of the schema misnames no security key, so a firm setting `ui.theme` or
// `privacy.counts` locks nothing beyond it.
#[test]
fn no_known_key_misnames_a_security_key() {
    for (name, _) in crate::leaf::KEYS {
        for key in SECURITY_KEYS {
            assert!(!misnamed_as(name, key, &[]), "{name} {key}");
        }
    }
    // The rule alone would read `ui.language` as near `plans` (two edits from `lang`): only
    // its being a known key keeps it from locking. A misspelt `ui.languages` locks plans.
    assert!(about_section("ui.language", "plans"));
    assert!(misnamed_as("ui.languages", "plans.", &[]));
}

// The orchestrator's review of #450: any character outside ASCII, at any depth, was about every
// section, so a comment key such as `café` or a Hindi word deep in a note locked everything.
// It now counts only at a section's place: the top level, directly under an unknown wrapper, or
// directly under a known section that holds no security key.
#[test]
fn a_non_ascii_name_is_about_every_section_only_at_a_section_place() {
    for name in [
        "टिप्पणी",
        "ѕkіlls",
        "x.thème",
        "x.thème.note",
        "ui.ѕkіlls",
        "matters.café",
    ] {
        for section in security_sections() {
            assert!(about_section(name, section), "{name:?} {section}");
        }
    }
    for name in [
        "_comment.note.café",
        "_comment.note.टिप्पणी",
        "ui.note.café",
        "tools.café",
        "privacy.thème",
    ] {
        for section in security_sections() {
            if !name.starts_with(section) {
                assert!(!about_section(name, section), "{name:?} {section}");
            }
        }
    }
    // Deeper, the ASCII rules still hold: two Cyrillic letters are two edits from `skills`.
    assert!(about_section("_comment.note.ѕkіlls", "skills"));
    assert!(about_section("_comment.note.café_tools", "tools"));
}
