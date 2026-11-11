//! The rules the legal part gives both agents (decision record 0058), read in each agent's
//! rendered text: the tokens left for the host, who the assistant says it is, how it names a file
//! it saved, that it answers in the chat, and how it makes a new document.

use std::path::Path;

use clauseo_profile::ClauseoProfile;
use clauseo_profile::instructions::LEGAL;
use clauseo_profile::placeholders::{
    BROWSER, CONNECTORS, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER,
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
        assert!(!text.contains("{author}") && !text.contains(PLAN));
        let filled = clauseo_profile::placeholders::fill(
            &text,
            &IN_INSTRUCTIONS,
            &[
                (WORK_FOLDER, "W"),
                (MATTER_NOTES, "M"),
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

/// The chat rule (decision record 0058): both agents answer in the chat, make a file only when
/// the lawyer asks for one, and leave no Markdown or scratch file in the matter. It is in the
/// legal part, so it holds with or without stetdoc.
#[test]
fn each_agent_answers_in_the_chat_and_leaves_no_scratch_file() {
    const RULE: &str = "The text after your last tool call is the reply. Every caveat and \
        limitation goes there. Answer in the chat, however long; make a file only when the lawyer \
        asks for one, as a `.docx` unless another format is named, and never leave Markdown or \
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

/// The new-document rule (decision record 0058): with a working stetdoc, both agents read that
/// tracked changes only change a paper that already exists, and that a new document is made
/// clean with pandoc from a Markdown file. The saved-file rule then names it.
#[test]
fn each_agent_makes_a_new_document_clean_with_pandoc() {
    let profile =
        launch_profile(&std::env::temp_dir().join("new-document-rule")).with_stetdoc_cli(true);
    let claude = profile.claude_options().system_prompt_append().to_owned();
    for text in [claude, profile.codex_instructions_for(false)] {
        assert!(
            text.contains("Tracked changes are only for changing a paper that already exists."),
            "{text}"
        );
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
