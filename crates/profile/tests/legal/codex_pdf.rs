//! The line Codex gets about scanned and long PDFs.

/// Codex has no PDF input, so every Codex session, on every OS, is told to render pages with
/// Clauseo's `pdftoppm` into its work folder and view them; Claude, whose Read runs `pdftoppm`
/// itself, never is.
#[test]
fn only_codex_is_told_to_render_pdf_pages_into_its_work_folder() {
    use clauseo_profile::instructions::{codex_pdf, word_count};
    use clauseo_protocol::AgentKind;
    let line = codex_pdf();
    assert_eq!(line.matches(". ").count(), 0, "one sentence: {line}");
    assert!(word_count(line) <= 40, "{} words", word_count(line));
    assert!(
        line.contains("`pdftoppm -png -r 100 -f <first> -l <last> "),
        "{line}"
    );
    assert!(line.contains("\"{work_folder}/<name>\""), "{line}");
    let profile = super::launch_profile(&std::env::temp_dir().join("codex-pdf-line"));
    let offline = clauseo_profile::instructions::CODEX_SHELL_OFFLINE.trim();
    let shared = format!(
        "{}\n\n{offline}",
        profile.instructions_for(AgentKind::Codex)
    );
    assert_eq!(
        profile.codex_instructions_for(false),
        format!("{shared}\n\n{line}")
    );
    assert!(
        profile
            .codex_instructions_for(true)
            .starts_with(&format!("{shared}\n\n{line}"))
    );
    let codex = profile.codex_keys_without_index(Some(AgentKind::Codex));
    assert!(
        codex["developer_instructions"]
            .as_str()
            .unwrap()
            .contains(line)
    );
    assert!(!profile.instructions().contains("pdftoppm"));
    for macos in [true, false] {
        assert!(!profile.claude_instructions_for(macos).contains("pdftoppm"));
    }
}
