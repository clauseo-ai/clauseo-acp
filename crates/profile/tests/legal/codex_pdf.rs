//! The line Codex gets about scanned and long PDFs.

/// Codex has no PDF input, so where Clauseo's `pdftoppm` is on its `PATH` the desktop tells it
/// to render pages into its work folder and view them (`setup/hook.rs`). The profile's own
/// instructions never say so, for either agent: the tools are not everywhere.
#[test]
fn the_pdf_line_is_one_short_sentence_the_profile_never_adds_itself() {
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
    for macos in [true, false] {
        assert!(!profile.codex_instructions_for(macos).contains("pdftoppm"));
        assert!(!profile.claude_instructions_for(macos).contains("pdftoppm"));
    }
    let codex = profile.codex_keys_without_index(Some(AgentKind::Codex));
    let text = codex["developer_instructions"].as_str().unwrap();
    assert!(!text.contains("pdftoppm"));
    assert!(!profile.instructions().contains("pdftoppm"));
}
