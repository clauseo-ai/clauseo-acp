//! The rules the legal part gives both agents (decision record 0058), read in each agent's
//! rendered text: the tokens left for the host, who the assistant says it is, how it names a file
//! it saved, where its drafts and answers go, and how it makes a new document.

use std::path::Path;

use clauseo_profile::ClauseoProfile;
use clauseo_profile::instructions::LEGAL;
use clauseo_profile::placeholders::{
    BROWSER, CONNECTORS, IN_INSTRUCTIONS, MATTER_HISTORY, MATTER_NOTES, WORK_FOLDER,
};

use super::{launch_profile, paths_under};

#[test]
fn the_launch_instructions_hold_only_tokens_the_host_fills() {
    use clauseo_profile::instructions::PLAN;
    use clauseo_protocol::AgentKind;
    let profile = ClauseoProfile::new(paths_under(Path::new("/data")), None);
    assert_eq!(profile.instructions().matches(PLAN).count(), 2);
    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let text = profile.instructions_for(kind);
        assert!(text.contains(WORK_FOLDER) && text.contains(MATTER_NOTES));
        assert_eq!(text.matches(MATTER_HISTORY).count(), 2, "the history line");
        assert!(!text.contains("{author}") && !text.contains(PLAN));
        let filled = clauseo_profile::placeholders::fill(
            &text,
            &IN_INSTRUCTIONS,
            &[
                (WORK_FOLDER, "W"),
                (MATTER_NOTES, "M"),
                (MATTER_HISTORY, "H"),
                (CONNECTORS, ""),
                (BROWSER, ""),
            ],
        )
        .unwrap();
        assert!(!filled.contains("{work_folder}") && !filled.contains("{matter_notes}"));
        assert!(!filled.contains(CONNECTORS));
    }
}

/// The identity rule (decision record 0058): the assistant is Clauseo and, only when asked,
/// names the account the session works through as the app names the plan: `ChatGPT` in what
/// Codex gets, `Claude` in what Claude gets. The senior-lawyer sentence still opens the section.
#[test]
fn each_agent_is_told_it_is_clauseo_on_its_own_account() {
    use clauseo_profile::instructions::{PLAN, plan_name};
    use clauseo_protocol::AgentKind;
    const SENIOR: &str = "## Identity and audience\n\nYou think like a senior lawyer in Indian \
        practice: identify the controlling legal question first, then which statutes govern, \
        which courts have spoken and where binding authority lies.";
    let rule = |plan: &str| {
        format!(
            "You are Clauseo, working through the lawyer's {plan} account. Only when asked who \
             you are or what AI you run on, say so plainly: \"I'm Clauseo. I'm using your \
             {plan} account to work on this matter.\" Never claim to be another product or \
             hide which account you use."
        )
    };
    assert!(LEGAL.starts_with(SENIOR));
    assert_eq!(LEGAL.matches(&rule(PLAN)).count(), 1);
    assert_eq!(plan_name(AgentKind::Claude), "Claude");
    assert_eq!(plan_name(AgentKind::Codex), "ChatGPT");
    let profile = launch_profile(&std::env::temp_dir().join("identity-rule"));
    let claude = profile.claude_options().system_prompt_append().to_owned();
    let codex = profile.codex_instructions_for(false);
    assert!(claude.contains(&rule("Claude")), "{claude}");
    assert!(codex.contains(&rule("ChatGPT")), "{codex}");
    assert!(!claude.contains("ChatGPT"));
    assert!(!codex.contains("your Claude account"));
    for text in [&claude, &codex] {
        assert!(text.contains(SENIOR));
        assert!(!text.contains(PLAN));
    }
    assert_eq!(
        profile.codex_instructions_for(true).matches(PLAN).count(),
        0
    );
}

/// The saved-file rule (decision record 0058): both agents name a file they saved in the matter
/// in plain words, with no download, `file://` or `sandbox:` link and no full path, in the
/// Conduct bullet on what never goes in text for the lawyer.
#[test]
fn each_agent_names_a_saved_file_with_no_download_link() {
    const RULE: &str = "Name a file you saved or changed plainly (\"I saved it as Reply to legal \
        notice.docx in the matter.\"), with no download link, `file://` or `sandbox:` link or \
        full path: the lawyer opens it from the papers list.";
    let bullet = LEGAL
        .lines()
        .find(|line| line.starts_with("- Never put tool names"))
        .unwrap();
    assert!(bullet.ends_with(RULE), "{bullet}");
    assert_eq!(LEGAL.matches(RULE).count(), 1);
    let profile = launch_profile(&std::env::temp_dir().join("saved-file-rule"));
    let claude = profile.claude_options().system_prompt_append().to_owned();
    for text in [claude, profile.codex_instructions_for(false)] {
        assert!(text.contains(RULE), "{text}");
    }
}

/// The deliverable rule (decision record 0058): both agents put a memo, letter, reply or other
/// draft in a clean `.docx` in the matter and name it in the chat with a short summary, answer
/// everything else in the chat, and leave no Markdown or scratch file in the matter. It is in the
/// legal part, so it holds with or without stetdoc.
#[test]
fn each_agent_puts_a_draft_in_word_and_answers_in_the_chat() {
    const RULE: &str = "The text after your last tool call is the reply. Every caveat and \
        limitation goes there. A memo, letter, reply or other draft is a clean `.docx` in the \
        matter, unless the lawyer names another format: name it in the chat with a short summary. \
        Answers, explanations and analysis go in the chat, however long. Never leave Markdown or \
        scratch files in the matter.";
    assert_eq!(LEGAL.matches(RULE).count(), 1);
    for stetdoc in [false, true] {
        let profile =
            launch_profile(&std::env::temp_dir().join("chat-rule")).with_stetdoc_cli(stetdoc);
        let claude = profile.claude_options().system_prompt_append().to_owned();
        for text in [claude, profile.codex_instructions_for(false)] {
            assert!(text.contains(RULE), "{text}");
        }
    }
}

/// The new-document rule (decision record 0058): with a working stetdoc, both agents read that a
/// new document is made clean with pandoc from a Markdown file. The saved-file rule then names it.
#[test]
fn each_agent_makes_a_new_document_clean_with_pandoc() {
    let profile =
        launch_profile(&std::env::temp_dir().join("new-document-rule")).with_stetdoc_cli(true);
    let claude = profile.claude_options().system_prompt_append().to_owned();
    for text in [claude, profile.codex_instructions_for(false)] {
        assert!(text.contains("Make a new document clean"), "{text}");
        assert!(
            text.contains("`pandoc \"{work_folder}/<file>.md\" -o \"<Name>.docx\"`"),
            "{text}"
        );
        assert!(text.contains("Never pipe into pandoc."), "{text}");
        assert!(
            text.contains("Name a file you saved or changed plainly"),
            "{text}"
        );
    }
    assert!(
        !launch_profile(&std::env::temp_dir().join("new-document-off"))
            .instructions()
            .contains("pandoc")
    );
}

/// The sentence both agents are told about a paper the lawyer mentions with "@" (UC-711): once in
/// each agent's text on every OS, with or without a working stetdoc, and in the reviewed words,
/// verbatim in `ui/app/src/review/registry-uc-agents.tsx` and provisional in its approvals file.
#[test]
fn both_agents_read_a_mentioned_paper_in_the_reviewed_words() {
    use clauseo_profile::instructions::TOOLS;
    const LINE: &str = "When the lawyer mentions a paper with @, read it before you answer; for a \
                        Word document, use stetdoc.";
    assert_eq!(TOOLS.matches(LINE).count(), 1);
    let root = std::env::temp_dir().join("rules-mention-read");
    for stetdoc in [false, true] {
        let profile = launch_profile(&root).with_stetdoc_cli(stetdoc);
        for macos in [false, true] {
            let claude = profile.claude_instructions_for(macos);
            let codex = profile.codex_instructions_for(macos);
            assert_eq!(claude.matches(LINE).count(), 1, "{claude}");
            assert_eq!(codex.matches(LINE).count(), 1, "{codex}");
        }
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/app");
    let registry = std::fs::read_to_string(ui.join("src/review/registry-uc-agents.tsx")).unwrap();
    assert!(registry.contains("\"UC-711\""));
    assert!(registry.contains(&serde_json::to_string(LINE).unwrap()));
    let approvals =
        std::fs::read_to_string(ui.join("copy-approvals/UC-agents-mention-read.json")).unwrap();
    let approvals: serde_json::Value = serde_json::from_str(&approvals).unwrap();
    assert_eq!(approvals["UC-711"]["en"], LINE);
    assert_eq!(approvals["UC-711"]["status"], "approved");
    assert!(approvals["UC-711"]["hi"].is_null());
}

/// The clean-edit sentence (UC-985, Rohan 23 Nov): both agents edit as tracked changes unless the
/// lawyer asks for no tracked changes, then make a clean edit with `stetdoc edit --no-track`, and
/// use `--manual` only when asked for it by name. Once in each agent's text on every OS, with or
/// without a working stetdoc, verbatim in `registry-uc-agents.tsx` and in its approvals file.
#[test]
fn both_agents_make_a_clean_edit_when_asked_for_no_tracked_changes() {
    use clauseo_profile::instructions::TOOLS;
    const LINE: &str = "When the lawyer asks for no tracked changes, a clean edit or no markup, use \
                        `stetdoc edit --no-track` and say it is not tracked, so Word has nothing \
                        to accept or reject, and the earlier version is in Versions. Use \
                        `--manual` only when the lawyer asks for manual markup by name.";
    assert_eq!(TOOLS.matches(LINE).count(), 1);
    assert!(!TOOLS.contains("only with the `stetdoc` command in your shell, as tracked changes"));
    let root = std::env::temp_dir().join("rules-clean-edit");
    for stetdoc in [false, true] {
        let profile = launch_profile(&root).with_stetdoc_cli(stetdoc);
        for macos in [false, true] {
            let claude = profile.claude_instructions_for(macos);
            let codex = profile.codex_instructions_for(macos);
            assert_eq!(claude.matches(LINE).count(), 1, "{claude}");
            assert_eq!(codex.matches(LINE).count(), 1, "{codex}");
        }
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/app");
    let registry = std::fs::read_to_string(ui.join("src/review/registry-uc-agents.tsx")).unwrap();
    assert!(registry.contains("\"UC-985\""));
    assert!(registry.contains(&serde_json::to_string(LINE).unwrap()));
    let approvals =
        std::fs::read_to_string(ui.join("copy-approvals/UC-agents-clean-edit.json")).unwrap();
    let approvals: serde_json::Value = serde_json::from_str(&approvals).unwrap();
    assert_eq!(approvals["UC-985"]["en"], LINE);
    assert!(approvals["UC-985"]["hi"].is_null());
}

/// Claude lists, searches and reads files with `Glob`, `Grep` and `Read`, never through a shell:
/// once in Claude's text on every OS, with or without a working stetdoc, never in Codex's (its
/// tools are others); "Working at scale" no longer reads the text copies with `cat` or `sed -n`;
/// verbatim in `registry-uc-agents.tsx` and in its approvals file.
#[test]
fn claude_lists_and_reads_files_with_its_own_tools() {
    use clauseo_profile::instructions::{CLAUDE_FILES, SCALE_CLAUDE};
    const LINE: &str = "To list, search or read files in the matter folder or your notes folder, \
                        use the Glob, Grep and Read tools. Do not use PowerShell or Bash commands \
                        for listing, searching or reading files.";
    assert_eq!(CLAUDE_FILES.trim(), LINE);
    for shell_read in ["`cat`", "`sed -n`", "with your shell", "one grep or script"] {
        assert!(!SCALE_CLAUDE.contains(shell_read), "{shell_read}");
    }
    let root = std::env::temp_dir().join("rules-claude-files");
    for stetdoc in [false, true] {
        let profile = launch_profile(&root).with_stetdoc_cli(stetdoc);
        for macos in [false, true] {
            let claude = profile.claude_instructions_for(macos);
            let codex = profile.codex_instructions_for(macos);
            assert_eq!(claude.matches(LINE).count(), 1, "{claude}");
            assert!(!codex.contains("Glob, Grep and Read"), "{codex}");
        }
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/app");
    let registry = std::fs::read_to_string(ui.join("src/review/registry-uc-agents.tsx")).unwrap();
    assert!(registry.contains("\"UC-1200\""));
    assert!(registry.contains(&serde_json::to_string(LINE).unwrap()));
    let approvals =
        std::fs::read_to_string(ui.join("copy-approvals/UC-claude-files.json")).unwrap();
    let approvals: serde_json::Value = serde_json::from_str(&approvals).unwrap();
    assert_eq!(approvals["UC-1200"]["en"], LINE);
}

/// Helpers on both agents, in the same one case (more than eight documents or about 300 pages,
/// long papers only) or when the lawyer asks for them, at most six for one answer: Claude's
/// "Working at scale" says so, and so does Codex's `AGENTS.md`, which is verbatim in
/// `registry-uc-agents.tsx` and in its approvals file and in neither agent's instructions.
#[test]
fn both_agents_start_helpers_in_the_same_cases() {
    use clauseo_profile::instructions::{SCALE_CLAUDE, codex_agents_md};
    for part in [
        "more than eight documents or about 300 pages read, or the lawyer asks for helpers:",
        "Launch at most six helpers for one answer.",
    ] {
        assert_eq!(SCALE_CLAUDE.matches(part).count(), 1, "{part}");
    }
    let codex = codex_agents_md();
    for part in [
        "more than eight documents or about 300 pages read",
        "longer than about 15,000 characters into 4 to 6 batches",
        "Start each sub-agent fresh, without your conversation history (fork_turns=\"none\").",
        "Start at most six sub-agents for one answer.",
        "unless the lawyer asks for helpers in their message.",
    ] {
        assert_eq!(codex.matches(part).count(), 1, "{part}");
    }
    let root = std::env::temp_dir().join("rules-codex-agents");
    let profile = launch_profile(&root);
    for macos in [false, true] {
        assert!(
            !profile
                .codex_instructions_for(macos)
                .contains("sub-agents in one case")
        );
        assert!(
            !profile
                .claude_instructions_for(macos)
                .contains("sub-agents in one case")
        );
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/app");
    let registry = std::fs::read_to_string(ui.join("src/review/registry-uc-agents.tsx")).unwrap();
    assert!(registry.contains("\"UC-1201\""));
    assert!(registry.contains(&serde_json::to_string(codex).unwrap()));
    let approvals =
        std::fs::read_to_string(ui.join("copy-approvals/UC-claude-files.json")).unwrap();
    let approvals: serde_json::Value = serde_json::from_str(&approvals).unwrap();
    assert_eq!(approvals["UC-1201"]["en"], codex);
}

/// Codex's `AGENTS.md`: within its word cap, and the text that asks Codex for helpers on a long
/// matter only, each started fresh, none starting helpers of its own: no config key caps a
/// helper's depth for the second multi-agent version (`agents.max_depth` is "V1 only; ignored by
/// V2" in codex 0.171.0), so the sentence does.
#[test]
fn codex_agents_md_asks_for_helpers_on_long_matters_only() {
    use clauseo_profile::instructions::{CODEX_AGENTS_MD, codex_agents_md, word_count};
    let words = word_count(codex_agents_md());
    assert!(words <= 150, "{words} words");
    assert_eq!(codex_agents_md(), CODEX_AGENTS_MD.trim());
    for pinned in [
        "In this workspace the user explicitly asks for sub-agents in one case: when a task needs \
         more than eight documents or about 300 pages read.",
        "Start each sub-agent fresh, without your conversation history (fork_turns=\"none\").",
        "Start at most six sub-agents for one answer. Sub-agents never start sub-agents of their \
         own.",
        "In every other case do the work yourself, unless the lawyer asks for helpers in their \
         message.",
    ] {
        assert!(CODEX_AGENTS_MD.contains(pinned), "{pinned}");
    }
}

/// Both agents, every OS: unfinished work is reported as done and left, with an offer to go on
/// (Codex at Low effort stopped part-way with "a partial list" and no offer, Windows run of 24
/// Nov), and file repairs and format fixes count as mechanics the lawyer is not told about.
#[test]
fn both_agents_say_what_is_left_and_offer_to_continue() {
    use clauseo_profile::instructions::LEGAL;
    const LEFT: &str =
        "If you stop short, say what is done and what is left, and offer to continue.";
    assert_eq!(LEGAL.matches(LEFT).count(), 1);
    assert_eq!(
        LEGAL
            .matches("mechanics (file repairs, format fixes)")
            .count(),
        1
    );
    let profile = launch_profile(&std::env::temp_dir().join("rules-left"));
    for macos in [false, true] {
        assert_eq!(
            profile.claude_instructions_for(macos).matches(LEFT).count(),
            1
        );
        assert_eq!(
            profile.codex_instructions_for(macos).matches(LEFT).count(),
            1
        );
    }
}
