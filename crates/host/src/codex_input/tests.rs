//! Codex's count, at the limit and one over, on the shared case table (Hindi, emoji, CRLF),
//! and the link text codex-acp adds for each file.

use clauseo_protocol::ContentBlock;

use super::{CODEX_MAX_INPUT_CHARS, codex_input_chars, over_codex_cap};

fn text(text: &str) -> ContentBlock {
    ContentBlock::Text {
        text: text.to_owned(),
    }
}

fn link(name: &str, uri: &str) -> ContentBlock {
    ContentBlock::ResourceLink {
        uri: uri.to_owned(),
        name: name.to_owned(),
        mime_type: None,
        title: None,
        description: None,
        size: None,
    }
}

/// Every case of the shared table: a text block counts its scalar values.
#[test]
fn a_text_block_counts_scalar_values_as_the_shared_table_says() {
    let table: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/tests/support/escaped-len-cases.json"
    ))
    .unwrap();
    let cases = table["cases"].as_array().unwrap();
    for name in ["Hindi", "emoji", "CRLF", "astral only", "Hindi conjuncts"] {
        assert!(cases.iter().any(|case| case["name"] == name), "{name}");
    }
    for case in cases {
        let expected = usize::try_from(case["scalar_values"].as_u64().unwrap()).unwrap();
        let blocks = [text(case["text"].as_str().unwrap())];
        assert_eq!(codex_input_chars(&blocks), expected, "{}", case["name"]);
    }
}

/// At the limit passes, one over is refused, summed over the blocks; a 4-byte emoji and a
/// Devanagari letter count one each, `\r\n` two.
#[test]
fn the_cap_is_scalar_values_summed_over_the_turn() {
    let max = CODEX_MAX_INPUT_CHARS;
    assert!(!over_codex_cap(&[text(&"a".repeat(max))]));
    assert!(over_codex_cap(&[text(&"a".repeat(max + 1))]));
    let half = "b".repeat(max / 2);
    assert!(!over_codex_cap(&[text(&half), text(&half)]));
    assert!(over_codex_cap(&[text(&half), text(&(half.clone() + "x"))]));
    for (unit, per) in [("😀", 1), ("क", 1), ("\r\n", 2)] {
        let at = unit.repeat(max / per);
        assert_eq!(codex_input_chars(&[text(&at)]), max, "{unit:?}");
        assert!(!over_codex_cap(&[text(&at)]), "{unit:?} at the limit");
        assert!(over_codex_cap(&[text(&(at + "a"))]), "{unit:?} one over");
    }
    // Bytes would refuse this: 4 MiB of emoji is 1 Mi characters.
    assert_eq!("😀".repeat(max).len(), 4 * max);
}

/// codex-acp writes a resource link as `[@name](uri)`, or for a nameless `file://` URI as
/// `[@last part](uri)`, else as the URI; that text counts (the shared table's `codex_links`,
/// the first measured live).
#[test]
fn a_file_link_counts_as_codex_acp_writes_it() {
    let table: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/tests/support/escaped-len-cases.json"
    ))
    .unwrap();
    let links = table["codex_links"].as_array().unwrap();
    assert!(links.len() >= 5);
    for case in links {
        let (name, uri) = (
            case["name"].as_str().unwrap(),
            case["uri"].as_str().unwrap(),
        );
        let expected = usize::try_from(case["codex_chars"].as_u64().unwrap()).unwrap();
        assert_eq!(codex_input_chars(&[link(name, uri)]), expected, "{case}");
    }
    // At the cap with a link passes; one more character over it is refused.
    let max = CODEX_MAX_INPUT_CHARS;
    let paper = link("paper.txt", "file:///tmp/vm2/cxbound/scratch/r3/paper.txt");
    let at = "a".repeat(max - 58);
    assert!(!over_codex_cap(&[text(&at), paper.clone()]));
    assert!(over_codex_cap(&[text(&(at + "a")), paper]));
}

/// The cap and the count were read for one codex-acp and its Codex. Clauseo installs the
/// versions `crates/limits/tests/live/package-lock.json` pins (the `agents` crate embeds it);
/// a bump fails here until Codex's limit is checked again.
#[test]
fn the_cap_names_the_codex_acp_clauseo_installs() {
    let lock: serde_json::Value =
        serde_json::from_str(include_str!("../../../limits/tests/live/package-lock.json")).unwrap();
    let pinned = |package: &str| {
        lock["packages"][format!("node_modules/{package}")]["version"]
            .as_str()
            .unwrap_or_else(|| panic!("{package} is not in the pins"))
            .to_owned()
    };
    let recheck = "re-check Codex's input limit (MAX_USER_INPUT_TEXT_CHARS and how codex-acp \
        builds a turn's input; crates/host/src/codex_input.rs says where) before bumping, then \
        update CODEX_ACP_VERSION and CODEX_VERSION";
    assert_eq!(
        pinned("@agentclientprotocol/codex-acp"),
        super::CODEX_ACP_VERSION,
        "codex-acp moved: {recheck}"
    );
    assert_eq!(
        pinned("@openai/codex"),
        super::CODEX_VERSION,
        "Codex moved: {recheck}"
    );
    // codex-acp asks for ^0.160.1: Clauseo's own override is what holds Codex at this version.
    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../../limits/tests/live/package.json")).unwrap();
    assert_eq!(
        package["overrides"]["@openai/codex"],
        super::CODEX_VERSION,
        "the @openai/codex override moved: {recheck}"
    );
}
