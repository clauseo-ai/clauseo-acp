//! The word budgets of everything each agent is given as standing text, as the app launches it:
//! a working stetdoc, a long macOS data folder with spaces, the host's folders filled, and the
//! longest connectors and browser parts that agent can get. Decision record 0031 set the budgets
//! and 0058 amended them; #1123 measures each agent's whole text too. A cap is the count that day
//! rounded up to the next 50, never below a cap that was already there.

use std::path::Path;

use clauseo_profile::instructions::{
    BrowserPart, BrowserRoute, CLAUDE_SHELL_MACOS, CODEX_REFUSAL, CODEX_SHELL_OFFLINE, LEGAL,
    SCALE_CLAUDE, STETDOC_CLI, TOOLS, browser_part, browser_part_for, codex_shell, connectors_part,
    word_count,
};
use clauseo_profile::placeholders::{
    BROWSER, CONNECTORS, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER,
};
use clauseo_profile::subagents::{PAPER_READER, ResearcherArm, researcher_prompt, subagents};
use clauseo_profile::{ClauseoProfile, Instructions};
use clauseo_protocol::AgentKind;

/// The legal part (`legal.md`): 0058's cap, kept.
const LEGAL_MAX: usize = 1360;
/// The text both agents share (the tools part with the stetdoc line, and the legal part), filled:
/// 0058's cap, kept.
const SHARED_MAX: usize = 1800;
/// Claude's whole `systemPrompt.append` on macOS: the tools part with the stetdoc line, the
/// legal part, the shell line and "Working at scale".
const CLAUDE_MAX: usize = 2250;
/// Codex's whole `developer_instructions` on macOS: the tools part with the stetdoc line, the
/// legal part, the offline line, the refusal line, the shell line and the built-in skills index.
/// A launch's pack lines come on top, capped by their own budget (decision record 0040).
/// 2000 until the browser part's certificate-error rule (20 words) took the count from 1996 to
/// 2016.
const CODEX_MAX: usize = 2040;
/// The paper reader's prompt, its folder filled.
const PAPER_READER_MAX: usize = 200;
/// The researcher's prompt, either arm.
const RESEARCHER_MAX: usize = 650;

/// A macOS data folder whose path holds spaces.
const DATA: &str = "/Users/Ann Mary/Library/Application Support/Clauseo";

/// `text` with the host's tokens filled as for a session under [`DATA`] with the case-law
/// entry, `{browser}` given `browser`.
#[cfg(test)]
fn filled(text: &str, browser: &str) -> String {
    let work = format!("{DATA}/run/0b7e3c1a-5d2f-4e8a-9c61-2f4d8e7a1b30/work");
    let notes = format!("{DATA}/agent-state/matter-4821/notes");
    clauseo_profile::placeholders::fill(
        text,
        &IN_INSTRUCTIONS,
        &[
            (WORK_FOLDER, &work),
            (MATTER_NOTES, &notes),
            (CONNECTORS, connectors_part(true)),
            (BROWSER, browser),
        ],
    )
    .unwrap()
}

/// The profile as the app launches it, with a working stetdoc.
#[cfg(test)]
fn profile() -> ClauseoProfile {
    super::launch_profile(Path::new(DATA)).with_stetdoc_cli(true)
}

#[test]
fn legal_word_budget() {
    let legal = word_count(LEGAL);
    assert!(legal <= LEGAL_MAX, "the legal part has {legal} words");
    let whole = ClauseoProfile::new(super::paths_under(Path::new("/data")), None).instructions();
    assert_eq!(
        whole,
        Instructions::tools()
            .with_connectors()
            .with_part(LEGAL)
            .compose("")
    );
    assert!(whole.ends_with(&format!("\n\n{}", LEGAL.trim())));
    assert_eq!(
        word_count(&whole),
        word_count(TOOLS) + word_count(LEGAL),
        "only {{author}} is filled, with one word"
    );
    // With a working stetdoc, as the app launches: the stetdoc line joins the tools part.
    let stetdoc = ClauseoProfile::new(super::paths_under(Path::new("/data")), None)
        .with_stetdoc_cli(true)
        .instructions();
    assert_eq!(
        word_count(&stetdoc),
        word_count(&whole) + word_count(STETDOC_CLI)
    );
    let shared = filled(
        &profile().instructions_for(AgentKind::Claude),
        browser_part(BrowserPart::NotInstalled),
    );
    let count = word_count(&shared);
    assert!(count <= SHARED_MAX, "the shared text has {count} words");
}

/// Claude's standing text: everything in `systemPrompt.append`. Claude has no browser at launch,
/// so its longest browser part is the no-browser sentence.
#[test]
fn claude_standing_text_budget() {
    let profile = profile();
    let text = filled(
        &profile.claude_instructions_for(true),
        browser_part(BrowserPart::NotInstalled),
    );
    for part in [LEGAL, STETDOC_CLI, CLAUDE_SHELL_MACOS, SCALE_CLAUDE] {
        let head = part.trim().lines().next().unwrap();
        assert!(text.contains(head.split('{').next().unwrap()), "{head}");
    }
    let count = word_count(&text);
    assert!(
        count <= CLAUDE_MAX,
        "Claude's standing text has {count} words"
    );
}

/// Codex's standing text: everything in `developer_instructions` with the built-in index.
#[test]
fn codex_standing_text_budget() {
    let profile = profile();
    let index = profile.skills_index();
    assert!(word_count(&index) <= 150, "{} words", word_count(&index));
    let text = filled(
        &format!("{}\n\n{index}", profile.codex_instructions_for(true)),
        browser_part_for(BrowserPart::On, BrowserRoute::CodexEntry),
    );
    for line in [
        CODEX_SHELL_OFFLINE,
        CODEX_REFUSAL,
        codex_shell(true).unwrap(),
    ] {
        assert!(text.contains(line.trim()), "{line}");
    }
    assert!(text.ends_with(index.trim_end()));
    let count = word_count(&text);
    assert!(
        count <= CODEX_MAX,
        "Codex's standing text has {count} words"
    );
}

/// Each helper's prompt is all the standing text it gets (a helper never gets
/// `systemPrompt.append`).
#[test]
fn each_helper_prompt_is_within_its_budget() {
    let reader = filled(subagents()[PAPER_READER]["prompt"].as_str().unwrap(), "");
    let count = word_count(&reader);
    assert!(
        count <= PAPER_READER_MAX,
        "the paper reader's prompt has {count} words"
    );
    for arm in [ResearcherArm::Rh2, ResearcherArm::Rh3] {
        let count = word_count(&researcher_prompt(arm));
        assert!(
            count <= RESEARCHER_MAX,
            "{arm:?}'s prompt has {count} words"
        );
    }
}
