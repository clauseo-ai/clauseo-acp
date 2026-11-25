//! The word budgets of everything each agent is given as standing text, as the app launches it:
//! a working stetdoc, a long macOS data folder with spaces, the host's folders filled, and the
//! longest connectors and browser parts that agent can get. Decision record 0031 set the budgets
//! and 0058 amended them; #1123 measures each agent's whole text too. A cap is the count that day
//! rounded up to the next 50, never below a cap that was already there.

use std::path::Path;

use clauseo_profile::instructions::{
    BrowserPart, BrowserRoute, CLAUDE_FILES, CLAUDE_SHELL_MACOS, CODEX_REFUSAL, LEGAL,
    SCALE_CLAUDE, STETDOC_CLI, TOOLS, browser_part, browser_part_for, codex_agents_md, codex_shell,
    codex_shell_net, codex_shell_net_on, connectors_part, word_count,
};
use clauseo_profile::placeholders::{
    BROWSER, CONNECTORS, IN_INSTRUCTIONS, MATTER_HISTORY, MATTER_NOTES, WORK_FOLDER,
};
use clauseo_profile::subagents::{PAPER_READER, ResearcherArm, researcher_prompt, subagents};
use clauseo_profile::{ClauseoProfile, Instructions};
use clauseo_protocol::AgentKind;

/// The legal part (`legal.md`): 0058's cap, raised to the next 50 for the version history line,
/// and again for the permission-settings line.
const LEGAL_MAX: usize = 1450;
/// The text both agents share (the tools part with the stetdoc line, and the legal part), filled:
/// 0058's cap, raised to the next 50 for the permission-settings line, and again for the
/// unfinished-work line and the mechanics' examples (orchestrator, 24 Nov: caps rise by what a
/// ruled line needs).
const SHARED_MAX: usize = 1900;
/// Claude's whole `systemPrompt.append` on macOS: the tools part with the stetdoc line, the
/// legal part, the files line, the shell line and "Working at scale"; raised to the next 50 for
/// the version history line, again for the permission-settings line, and again for the files
/// line (the orchestrator's ruling, 24 Nov: the cap rises by what it needs).
const CLAUDE_MAX: usize = 2400;
/// Codex's whole `developer_instructions` on macOS: the tools part with the stetdoc line, the
/// legal part, the shell-net line (online with web search), the refusal line, the shell line and
/// the built-in skills index.
/// A launch's pack lines come on top, capped by their own budget (decision record 0040).
/// 2000 until the browser part's certificate-error rule (20 words) took the count from 1996 to
/// 2016; 2100 for the version history line; 2150 for the permission-settings line; 2200 for the
/// unfinished-work line and the mechanics' examples (24 Nov).
const CODEX_MAX: usize = 2200;
/// Codex's `AGENTS.md` (when to start sub-agents): its own cap, as it is no part of
/// `developer_instructions`. 24 Nov's text was 143 words; 150 with the sentence that keeps
/// helpers from starting helpers (C-3b), since no config key caps their depth for the second
/// multi-agent version.
const CODEX_AGENTS_MAX: usize = 150;
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
    let history = format!("{DATA}/matters/matter-4821/history.git");
    clauseo_profile::placeholders::fill(
        text,
        &IN_INSTRUCTIONS,
        &[
            (WORK_FOLDER, &work),
            (MATTER_NOTES, &notes),
            (MATTER_HISTORY, &history),
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
    for part in [
        LEGAL,
        STETDOC_CLI,
        CLAUDE_FILES,
        CLAUDE_SHELL_MACOS,
        SCALE_CLAUDE,
    ] {
        let head = part.trim().lines().next().unwrap();
        assert!(text.contains(head.split('{').next().unwrap()), "{head}");
    }
    let count = word_count(&text);
    assert!(
        count <= CLAUDE_MAX,
        "Claude's standing text has {count} words"
    );
}

/// Codex's standing text: everything in `developer_instructions` with the built-in index. Web
/// search on, its shell line is the Windows form on Windows and the other form elsewhere; both
/// forms are counted, and each must fit `CODEX_MAX`.
#[test]
fn codex_standing_text_budget() {
    let profile = profile();
    let index = profile.skills_index();
    assert!(word_count(&index) <= 150, "{} words", word_count(&index));
    let base = profile.codex_instructions_for(true);
    for windows in [false, true] {
        let line = codex_shell_net_on(true, windows);
        let text = filled(
            &format!("{}\n\n{index}", base.replace(codex_shell_net(true), line)),
            browser_part_for(BrowserPart::On, BrowserRoute::CodexEntry),
        );
        for shell in [line, CODEX_REFUSAL, codex_shell(true).unwrap()] {
            assert!(text.contains(shell.trim()), "{shell}");
        }
        assert!(text.ends_with(index.trim_end()));
        let count = word_count(&text);
        assert!(
            count <= CODEX_MAX,
            "Codex's standing text has {count} words (windows: {windows})"
        );
    }
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

/// Codex's `AGENTS.md` fits its own cap.
#[test]
fn codex_agents_md_budget() {
    let count = word_count(codex_agents_md());
    assert!(
        count <= CODEX_AGENTS_MAX,
        "Codex's AGENTS.md has {count} words"
    );
}
