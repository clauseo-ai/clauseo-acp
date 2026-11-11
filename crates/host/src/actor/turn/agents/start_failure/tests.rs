use std::path::{Path, PathBuf};

use super::*;

/// claude-agent-acp 0.89.1's stderr up to a refused `session/new`, as the process crate's tail
/// gives it (the home folder already `~`).
const TAIL: &str = "\
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=validate-cwd durationMs=1 totalMs=1
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=settings durationMs=4 totalMs=5
(node:1354020) [CLAUDE_SDK_CAN_USE_TOOL_SHADOWED] Warning: canUseTool will not be invoked for: WebSearch.
[session/query] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 resume=none apiType=native baseUrl=native
[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=prepare-query durationMs=11 totalMs=16
Error: Claude Code process exited with code 1
";

#[test]
fn the_summary_keeps_the_phases_and_the_last_line() {
    assert_eq!(
        start_summary(TAIL, &[]),
        "[session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=validate-cwd \
         durationMs=1 totalMs=1 | [session/create] sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 \
         phase=settings durationMs=4 totalMs=5 | [session/create] \
         sessionId=7da90d5d-1c5c-4b16-92f4-7ee338481cd8 phase=prepare-query durationMs=11 \
         totalMs=16 | Error: Claude Code process exited with code 1"
    );
}

#[test]
fn a_phase_last_line_goes_once_and_an_empty_tail_is_none() {
    let tail = "noise\n[session/create] sessionId=s phase=settings durationMs=4 totalMs=5\n\n";
    assert_eq!(
        start_summary(tail, &[]),
        "[session/create] sessionId=s phase=settings durationMs=4 totalMs=5"
    );
    assert_eq!(start_summary("", &[]), "none");
    assert_eq!(start_summary("\n  \n", &[]), "none");
}

#[test]
fn the_summary_is_cut_on_a_character_boundary() {
    let long = format!("{}é", "x".repeat(START_TAIL_BYTES - 1));
    let summary = start_summary(&long, &[]);
    assert_eq!(summary.len(), START_TAIL_BYTES - 1);
    assert!(summary.chars().all(|c| c == 'x'));
}

/// The spellings of `/Users/lawyer/Matters/Acme Holdings v Beta` on macOS with home
/// `/Users/lawyer`.
fn acme() -> Vec<String> {
    let folder = Path::new("/Users/lawyer/Matters/Acme Holdings v Beta/");
    matter_spellings(&[folder, folder], &[PathBuf::from("/Users/lawyer")], true)
}

#[test]
fn the_matter_folder_and_a_path_below_it_are_matter_in_the_summary() {
    let matter = acme();
    let tail = "\
[session/create] sessionId=s phase=validate-cwd cwd=/Users/lawyer/Matters/Acme Holdings v Beta durationMs=1 totalMs=1
Error: ENOENT: no such file or directory, open '~/Matters/Acme Holdings v Beta/Board minutes 2026.docx'
";
    let summary = start_summary(tail, &matter);
    assert_eq!(
        summary,
        "[session/create] sessionId=s phase=validate-cwd cwd=<matter> durationMs=1 totalMs=1 | \
         Error: ENOENT: no such file or directory, open '<matter>'"
    );
    assert!(!summary.contains("Acme"), "{summary}");
    assert!(!summary.contains("Board"), "{summary}");
}

#[test]
fn an_unquoted_sub_path_goes_to_its_end_and_the_rest_stays() {
    let matter = acme();
    assert_eq!(
        without_matter(
            "stat /Users/lawyer/Matters/Acme Holdings v Beta/pleadings/draft.md: denied",
            &matter
        ),
        "stat <matter>: denied"
    );
    assert_eq!(
        without_matter("a ~/Matters/Acme Holdings v Beta b", &matter),
        "a <matter> b"
    );
    assert_eq!(
        without_matter(
            "cwd \"~/Matters/Acme Holdings v Beta/x y.md\" gone",
            &matter
        ),
        "cwd \"<matter>\" gone"
    );
    assert_eq!(without_matter("~/Other/file", &matter), "~/Other/file");
}

#[test]
fn macos_private_spellings_and_the_canonical_folder_match() {
    let given = Path::new("/var/folders/ab/T/Kunal Desai Estate");
    let canonical = Path::new("/private/var/folders/ab/T/Kunal Desai Estate");
    let matter = matter_spellings(&[given, canonical], &[], true);
    for line in [
        "open /private/var/folders/ab/T/Kunal Desai Estate/notes.txt",
        "open /var/folders/ab/T/Kunal Desai Estate/notes.txt",
    ] {
        assert_eq!(without_matter(line, &matter), "open <matter>", "{line}");
    }
    let only_given = matter_spellings(&[given], &[], true);
    assert!(only_given.contains(&canonical.display().to_string()));
    // Off macOS no `/private` spelling is added.
    let linux = matter_spellings(&[given], &[], false);
    assert_eq!(linux, vec![given.display().to_string()]);
}

#[test]
fn a_windows_folder_matches_as_printed_and_json_escaped_and_a_root_is_never_a_spelling() {
    let matter = matter_spellings(&[Path::new(r"C:\Matters\Tata v Mistry")], &[], false);
    assert_eq!(
        without_matter(r"cwd C:\Matters\Tata v Mistry\a.txt", &matter),
        "cwd <matter>"
    );
    assert_eq!(
        without_matter(r#"{"cwd":"C:\\Matters\\Tata v Mistry"}"#, &matter),
        r#"{"cwd":"<matter>"}"#
    );
    assert_eq!(
        matter_spellings(&[Path::new("/")], &[], true),
        Vec::<String>::new()
    );
}

#[test]
fn the_summary_is_redacted_before_it_is_cut() {
    let matter = acme();
    let line = format!(
        "{} /Users/lawyer/Matters/Acme Holdings v Beta/x",
        "y".repeat(START_TAIL_BYTES - 10)
    );
    let summary = start_summary(&line, &matter);
    assert!(!summary.contains("/Users"), "{summary}");
    assert!(summary.len() <= START_TAIL_BYTES);
}
