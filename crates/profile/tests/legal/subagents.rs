//! Claude's subagents and its "Working at scale" part, and Codex's refusal line: what each agent
//! gets, as shipped, whichever measured setting `subagents::PAPER_READER_MODEL`,
//! `subagents::RESEARCHER_ARM` and `instructions::SHIP_WORKING_AT_SCALE` hold.

use clauseo_profile::claude::OPTION_KEYS;
use clauseo_profile::instructions::{
    CODEX_REFUSAL, LEGAL, SCALE_CLAUDE, SCALE_RESEARCH, SHIP_RESEARCH_TEXT, SHIP_WORKING_AT_SCALE,
    claude_scale,
};
use clauseo_profile::placeholders::WORK_FOLDER;
use clauseo_profile::subagents::{
    PAPER_READER, PAPER_READER_EFFORT, PAPER_READER_MODEL, RESEARCHER, RESEARCHER_ARM,
    ResearcherArm, legal_evidence_and_citations, researcher_prompt,
};
use clauseo_protocol::AgentKind;
use serde_json::{Value, json};

/// What Claude's instructions end with: a blank line and the "Working at scale" part, or nothing
/// when it does not ship.
pub(super) fn claude_scale_suffix() -> String {
    claude_scale(research()).map_or_else(String::new, |scale| format!("\n\n{scale}"))
}

/// Whether Claude's part holds the research bullets: the text ships and so does a researcher.
fn research() -> bool {
    SHIP_RESEARCH_TEXT && RESEARCHER_ARM.is_some()
}

/// Every `{…}` token in `text`, by its opening brace up to the closing one.
fn tokens(text: &str) -> Vec<&str> {
    text.match_indices('{')
        .filter_map(|(at, _)| text[at..].find('}').map(|end| &text[at..=at + end]))
        .collect()
}

/// The words that would tell an agent about subagents or batching.
const SCALE_WORDS: [&str; 4] = [
    "Working at scale",
    "paper-reader",
    "`researcher`",
    "subagent",
];

/// Claude's instructions end with the part, after everything else (as the timing harness
/// appended it), with the research bullets only when it has the researcher. Codex's never hold
/// it, nor any word about subagents: measured, it was slower and cost more.
#[test]
fn only_claude_gets_working_at_scale_and_it_comes_last() {
    let profile = super::launch_profile(&std::env::temp_dir().join("subagents-scale"));
    assert!(SCALE_CLAUDE.starts_with("## Working at scale\n\n"));
    assert!(SCALE_CLAUDE.contains("`paper-reader`") && !SCALE_CLAUDE.contains("`researcher`"));
    assert!(SCALE_RESEARCH.starts_with("- ") && SCALE_RESEARCH.contains("`researcher`"));
    assert!(!SCALE_RESEARCH.contains("##"));
    for part in [SCALE_CLAUDE, SCALE_RESEARCH] {
        assert!(tokens(part).iter().all(|token| *token == WORK_FOLDER));
    }
    assert!(SCALE_CLAUDE.contains(WORK_FOLDER));
    if SHIP_WORKING_AT_SCALE {
        assert_eq!(claude_scale(false).unwrap(), SCALE_CLAUDE.trim());
        let both = format!("{}\n{}", SCALE_CLAUDE.trim(), SCALE_RESEARCH.trim());
        assert_eq!(claude_scale(true).unwrap(), both);
    } else {
        assert_eq!(claude_scale(true), None);
    }
    let claude = profile.claude_instructions();
    assert!(claude.ends_with(&claude_scale_suffix()));
    assert_eq!(
        claude.contains("## Working at scale"),
        SHIP_WORKING_AT_SCALE
    );
    assert_eq!(
        claude.contains(SCALE_RESEARCH.trim()),
        SHIP_WORKING_AT_SCALE && research()
    );
    assert!(!profile.instructions().contains("Working at scale"));
    let meta = profile.session_meta(Some(AgentKind::Claude)).unwrap();
    assert_eq!(meta["systemPrompt"]["append"], claude.as_str());
    for macos in [true, false] {
        let codex = profile.codex_instructions_for(macos);
        for word in SCALE_WORDS {
            assert!(!codex.contains(word), "{word}");
        }
    }
    let keys = profile.codex_keys_without_index(Some(AgentKind::Codex));
    let text = keys["developer_instructions"].as_str().unwrap();
    assert!(SCALE_WORDS.iter().all(|word| !text.contains(word)));
}

/// Claude's `options.agents` holds the paper reader, and the researcher while one is chosen,
/// each as measured; Codex gets no agents at all.
#[test]
fn claude_ships_the_measured_paper_reader_and_researcher() {
    assert!(OPTION_KEYS.contains(&"agents"));
    assert!(["haiku", "sonnet"].contains(&PAPER_READER_MODEL));
    let profile = super::launch_profile(&std::env::temp_dir().join("subagents-agents"));
    let meta = profile.session_meta(Some(AgentKind::Claude)).unwrap();
    let agents = meta["claudeCode"]["options"]["agents"].as_object().unwrap();
    let names: Vec<&str> = agents.keys().map(String::as_str).collect();
    match RESEARCHER_ARM {
        Some(_) => assert_eq!(names, [PAPER_READER, RESEARCHER]),
        None => assert_eq!(names, [PAPER_READER]),
    }
    for agent in agents.values() {
        assert!(agent["description"].as_str().unwrap().len() > 40);
        let prompt = agent["prompt"].as_str().unwrap();
        assert!(prompt.contains(WORK_FOLDER));
        assert!(tokens(prompt).iter().all(|token| *token == WORK_FOLDER));
    }
    let reader = &agents[PAPER_READER];
    assert_eq!(reader["model"], PAPER_READER_MODEL);
    assert_eq!(reader["effort"], PAPER_READER_EFFORT);
    assert_eq!(reader["maxTurns"], 25);
    assert_eq!(reader["tools"], json!(["Read", "Grep", "Glob", "Write"]));
    let digest = reader["prompt"].as_str().unwrap();
    assert!(digest.contains("{work_folder}/digests/<batch name>.md"));
    assert!(digest.ends_with("Your final reply is only the digest path."));
    if let Some(arm) = RESEARCHER_ARM {
        let researcher = &agents[RESEARCHER];
        assert_eq!(researcher["model"], arm.model());
        assert_eq!(researcher["effort"], arm.effort());
        assert_eq!(researcher["maxTurns"], arm.max_turns());
        assert_eq!(researcher["prompt"], researcher_prompt(arm).as_str());
        let tools: Vec<&str> = researcher["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            tools,
            ["WebSearch", "WebFetch", "Read", "Grep", "Glob", "Write"]
        );
    }
    assert!(profile.session_meta(Some(AgentKind::Codex)).is_none());
    let codex =
        serde_json::to_string(&profile.codex_keys_without_index(Some(AgentKind::Codex))).unwrap();
    assert!(!codex.contains("\"agents\""));
}

/// The researcher's prompt starts with the legal part's Evidence and Citations sections,
/// verbatim, so it holds the lead's rules; RH3 alone adds a budget, and both end alike.
#[test]
fn the_researcher_prompt_carries_the_legal_evidence_rules() {
    let sections = legal_evidence_and_citations();
    assert!(sections.starts_with("## Evidence\n"));
    assert!(sections.contains("\n## Citations\n"));
    assert!(!sections.contains("## Working notes"));
    assert!(LEGAL.contains(sections));
    for arm in [ResearcherArm::Rh2, ResearcherArm::Rh3] {
        let prompt = researcher_prompt(arm);
        let rules = "- Search for authority against the position as hard as for authority";
        assert!(prompt.starts_with(&format!("{sections}\n\n{rules}")));
        assert!(prompt.contains("negates it.\n\nNever put client names"));
        for rule in [
            "later: appeal, review",
            "Attribute each proposition",
            "provisos",
        ] {
            assert!(prompt.contains(rule), "{rule}");
        }
        assert!(prompt.contains("{work_folder}/research/<topic>.md"));
        assert!(prompt.ends_with("not one after another."));
        assert_eq!(prompt.contains(" Budget: "), arm == ResearcherArm::Rh3);
    }
    assert_eq!(
        (ResearcherArm::Rh2.effort(), ResearcherArm::Rh2.max_turns()),
        ("medium", 30)
    );
    assert_eq!(
        (ResearcherArm::Rh3.effort(), ResearcherArm::Rh3.max_turns()),
        ("low", 20)
    );
}

/// The refusal line (UC-710) reaches every Codex session and no Claude one, and its words are
/// the reviewed copy: verbatim in `ui/app/src/review/registry-uc-agents.tsx` and in the branch's
/// approvals file, so what reviewers approve is what Codex is told.
#[test]
fn codex_is_told_to_report_every_refusal_in_the_reviewed_words() {
    let line = CODEX_REFUSAL.trim();
    assert!(!line.contains('\n'));
    assert!(line.ends_with("never try another way round it."));
    let profile = super::launch_profile(&std::env::temp_dir().join("subagents-refusal"));
    for macos in [true, false] {
        assert!(profile.codex_instructions_for(macos).contains(line));
        assert!(!profile.claude_instructions_for(macos).contains(line));
    }
    let keys = profile.codex_keys_without_index(Some(AgentKind::Codex));
    assert!(
        keys["developer_instructions"]
            .as_str()
            .unwrap()
            .contains(line)
    );
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/app");
    let registry = std::fs::read_to_string(ui.join("src/review/registry-uc-agents.tsx")).unwrap();
    assert!(registry.contains("\"UC-710\""));
    assert!(registry.contains(&serde_json::to_string(line).unwrap()));
    let approvals =
        std::fs::read_to_string(ui.join("copy-approvals/UC-agents-subagents-scale.json")).unwrap();
    let approvals: Value = serde_json::from_str(&approvals).unwrap();
    assert_eq!(approvals["UC-710"]["en"], line);
    assert_eq!(approvals["UC-710"]["status"], "provisional");
}
