//! Which `browser` results become a `DownloadOutsideMatter` notice, and which never do.

use std::path::{Path, PathBuf};

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    NoticeKind, NoticeLevel, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus, ToolKind,
};
use serde_json::json;

use super::{Fallbacks, saved_file};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// No notice.
const NONE: Vec<clauseo_protocol::Notice> = Vec::new();

/// `<temp>/agent-state/m1/downloads`, made, with `judgment.pdf` in it.
fn folder(dir: &TempDir) -> PathBuf {
    let folder = dir.path().join("agent-state").join("m1").join("downloads");
    write::create_dir_all(&folder).unwrap();
    write::write_file(&folder.join("judgment.pdf"), b"%PDF").unwrap();
    folder
}

/// The guard's line for `path`, as `saved.rs` writes it.
fn line(name: &str, path: &Path) -> String {
    format!(
        "Saved {name} outside the matter, in Clauseo's own folder for this matter: the matter's \
         drive cannot keep the \"downloaded from the internet\" mark, so the file is not in \
         downloads/. Your user sees where it is in the thread. Its path: {}",
        path.display()
    )
}

fn call(title: &str, name: Option<&str>) -> ToolCall {
    ToolCall {
        acp_id: "tc-1".into(),
        title: title.into(),
        kind: ToolKind::Other,
        tool_name: name.map(Into::into),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    }
}

/// Claude's shape: the result's blocks as text content.
fn claude_done(texts: &[&str]) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        content: Some(
            texts
                .iter()
                .map(|text| ToolCallContent::Text {
                    text: TextOrBlob::Inline((*text).to_owned()),
                })
                .collect(),
        ),
        ..ToolCallPatch::default()
    }
}

/// Codex's shape: no content, the MCP result in `rawOutput` `{ result, error }`.
fn codex_done(texts: &[&str]) -> ToolCallPatch {
    let content: Vec<_> = texts
        .iter()
        .map(|text| json!({"type": "text", "text": text}))
        .collect();
    let output = json!({"result": {"content": content}, "error": null});
    ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        raw_output: Some(TextOrBlob::Inline(output.to_string())),
        ..ToolCallPatch::default()
    }
}

#[test]
fn a_codex_browser_result_naming_a_saved_file_is_one_notice() {
    let dir = TempDir::new("fallback-codex");
    let folder = folder(&dir);
    let mut fallbacks = Fallbacks::default();
    let started = fallbacks.started(&call("mcp.browser.agent_browser_snapshot", None), None);
    assert_eq!(started, NONE);
    let text = line("judgment.pdf", &folder.join("judgment.pdf"));
    let notices = fallbacks.updated("tc-1", &codex_done(&["page text", &text]), Some(&folder));
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].level, NoticeLevel::Info);
    assert_eq!(
        notices[0].kind,
        Some(NoticeKind::DownloadOutsideMatter {
            name: "judgment.pdf".to_owned()
        })
    );
    assert!(notices[0].text.contains("judgment.pdf"));
}

#[test]
fn a_claude_browser_result_that_starts_completed_is_read_once() {
    let dir = TempDir::new("fallback-claude");
    let folder = folder(&dir);
    let text = line("judgment.pdf", &folder.join("judgment.pdf"));
    let mut fallbacks = Fallbacks::default();
    let mut done = call(
        "agent_browser_click",
        Some("mcp__browser__agent_browser_click"),
    );
    done.status = ToolCallStatus::Completed;
    done.content = claude_done(&[&text]).content.unwrap();
    assert_eq!(fallbacks.started(&done, Some(&folder)).len(), 1);
    // The same path again, in a later result: told already.
    done.acp_id = "tc-2".into();
    assert_eq!(fallbacks.started(&done, Some(&folder)), NONE);
}

#[test]
fn a_repeat_in_one_result_is_one_notice() {
    let dir = TempDir::new("fallback-repeat");
    let folder = folder(&dir);
    let text = line("judgment.pdf", &folder.join("judgment.pdf"));
    let mut fallbacks = Fallbacks::default();
    fallbacks.started(&call("mcp.browser.agent_browser_open", None), Some(&folder));
    let notices = fallbacks.updated("tc-1", &claude_done(&[&text, &text]), Some(&folder));
    assert_eq!(notices.len(), 1);
}

#[test]
fn another_servers_tool_and_a_call_not_followed_are_not_read() {
    let dir = TempDir::new("fallback-other");
    let folder = folder(&dir);
    let text = line("judgment.pdf", &folder.join("judgment.pdf"));
    let mut fallbacks = Fallbacks::default();
    fallbacks.started(&call("mcp.case-law.search", None), Some(&folder));
    assert_eq!(
        fallbacks.updated("tc-1", &codex_done(&[&text]), Some(&folder)),
        NONE
    );
    fallbacks.started(
        &call("Read file", Some("mcp__browserx__open")),
        Some(&folder),
    );
    assert_eq!(
        fallbacks.updated("tc-1", &claude_done(&[&text]), Some(&folder)),
        NONE
    );
    // A failed browser call is forgotten; its later completion reads nothing.
    fallbacks.started(&call("mcp.browser.agent_browser_open", None), Some(&folder));
    let failed = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        ..ToolCallPatch::default()
    };
    assert_eq!(fallbacks.updated("tc-1", &failed, Some(&folder)), NONE);
    assert_eq!(
        fallbacks.updated("tc-1", &claude_done(&[&text]), Some(&folder)),
        NONE
    );
}

#[test]
fn a_path_outside_the_folder_a_missing_file_or_a_folder_is_ignored() {
    let dir = TempDir::new("fallback-outside");
    let folder = folder(&dir);
    let outside = dir.path().join("judgment.pdf");
    write::write_file(&outside, b"%PDF").unwrap();
    assert_eq!(saved_file(&line("judgment.pdf", &outside), &folder), None);
    let other = dir.path().join("agent-state").join("m2").join("downloads");
    write::create_dir_all(&other).unwrap();
    write::write_file(&other.join("judgment.pdf"), b"%PDF").unwrap();
    assert_eq!(
        saved_file(&line("judgment.pdf", &other.join("judgment.pdf")), &folder),
        None
    );
    let missing = folder.join("gone.pdf");
    assert_eq!(saved_file(&line("gone.pdf", &missing), &folder), None);
    write::create_dir_all(&folder.join("sub")).unwrap();
    assert_eq!(saved_file(&line("sub", &folder.join("sub")), &folder), None);
    // `..`, `.`, a deeper path and a relative one never pass.
    let up = format!("{}{}..", folder.display(), std::path::MAIN_SEPARATOR);
    assert_eq!(
        saved_file(&format!("Saved x. Its path: {up}"), &folder),
        None
    );
    let dot = format!(
        "{}{}.",
        folder.join("judgment.pdf").display(),
        std::path::MAIN_SEPARATOR
    );
    assert_eq!(
        saved_file(&format!("Saved x. Its path: {dot}"), &folder),
        None
    );
    let deeper = folder.join("sub").join("judgment.pdf");
    write::write_file(&deeper, b"%PDF").unwrap();
    assert_eq!(saved_file(&line("judgment.pdf", &deeper), &folder), None);
    assert_eq!(
        saved_file("Saved judgment.pdf. Its path: judgment.pdf", &folder),
        None
    );
}

#[test]
fn only_the_guards_line_and_the_text_after_its_last_marker_count() {
    let dir = TempDir::new("fallback-marker");
    let folder = folder(&dir);
    let path = folder.join("judgment.pdf");
    let good = line("judgment.pdf", &path);
    assert_eq!(
        saved_file(&good, &folder),
        Some((path, "judgment.pdf".to_owned()))
    );
    // Page text that does not begin as the guard's line.
    assert_eq!(saved_file(&format!("A page said: {good}"), &folder), None);
    // A marker after the real path: no text after any marker is a file in the folder.
    assert_eq!(saved_file(&format!("{good} Its path: x"), &folder), None);
    // No marker at all.
    assert_eq!(
        saved_file("Saved judgment.pdf to downloads/ in the matter.", &folder),
        None
    );
}

/// A name that holds the marker itself puts it in the path too: the marker before the path
/// still reads, and the notice names the whole name. Unix only: Windows names hold no `:`.
#[cfg(unix)]
#[test]
fn a_name_holding_the_marker_still_reads() {
    let dir = TempDir::new("fallback-marker-name");
    let folder = folder(&dir);
    for name in ["a Its path: b.pdf", "Its path: .pdf"] {
        let path = folder.join(name);
        write::write_file(&path, b"x").unwrap();
        assert_eq!(
            saved_file(&line(name, &path), &folder),
            Some((path.clone(), name.to_owned())),
            "{name:?}"
        );
    }
}

#[test]
fn a_name_with_a_control_or_bidi_character_is_ignored() {
    let dir = TempDir::new("fallback-bidi");
    let folder = folder(&dir);
    let name = "invoice\u{202e}fdp.exe";
    write::write_file(&folder.join(name), b"x").unwrap();
    assert_eq!(saved_file(&line(name, &folder.join(name)), &folder), None);
}

#[cfg(unix)]
#[test]
fn a_link_in_the_folder_is_ignored() {
    let dir = TempDir::new("fallback-link");
    let folder = folder(&dir);
    let link = folder.join("link.pdf");
    write::symlink(&folder.join("judgment.pdf"), &link).unwrap();
    assert_eq!(saved_file(&line("link.pdf", &link), &folder), None);
}

#[test]
fn no_session_folder_reads_nothing() {
    let dir = TempDir::new("fallback-none");
    let folder = folder(&dir);
    let text = line("judgment.pdf", &folder.join("judgment.pdf"));
    let mut fallbacks = Fallbacks::default();
    fallbacks.started(&call("mcp.browser.agent_browser_open", None), None);
    assert_eq!(fallbacks.updated("tc-1", &codex_done(&[&text]), None), NONE);
}
