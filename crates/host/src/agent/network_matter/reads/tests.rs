//! Which reads inside a matter stored by its network address the host answers itself
//! (`reads.rs`), with Windows' rules on every system.

use std::path::Path;

use serde_json::{Value, json};

use super::{links_followed, own_share_read};

const MATTER: &str = r"\\srv\share\Matters\A";

/// Whether `tool` with `input` is answered, the agent's working folder being the matter.
fn allowed(tool: &str, input: &Value) -> bool {
    own_share_read(
        Some(tool),
        Some(input),
        Path::new(MATTER),
        Path::new(MATTER),
    )
}

fn read(path: &str) -> bool {
    allowed("Read", &json!({ "file_path": path }))
}

/// The own matter, by each tool, has no card.
#[test]
fn reads_inside_the_own_matter_are_answered() {
    for path in [
        r"\\srv\share\Matters\A\brief.docx",
        r"\\srv\share\Matters\A\Exhibits\B 2.pdf",
        r"\\srv\share\Matters\A",
        r"\\srv\share\Matters\A\",
        "//srv/share/Matters/A/brief.docx",
        r"\\srv\share\Matters\A\LONGNA~1.DOC",
        r"\\srv\share\Matters\A\~$brief.docx",
    ] {
        assert!(read(path), "{path}");
    }
    let page = json!({ "file_path": r"\\srv\share\Matters\A\x.pdf", "pages": "1-3", "limit": 9 });
    assert!(allowed("Read", &page));
    for grep in [
        json!({ "pattern": "indemnity", "path": MATTER }),
        json!({ "pattern": "indemnity", "path": format!(r"{MATTER}\Drafts"), "-i": true }),
        json!({ "pattern": "x", "glob": "*.docx", "output_mode": "content", "-C": 2, "-o": true }),
        json!({ "pattern": "x" }),
    ] {
        assert!(allowed("Grep", &grep), "{grep}");
    }
    for glob in [
        json!({ "pattern": "**/*.pdf" }),
        json!({ "pattern": "Drafts/*.docx", "path": MATTER }),
        json!({ "pattern": "{a,b}/*", "path": format!(r"{MATTER}\Drafts") }),
    ] {
        assert!(allowed("Glob", &glob), "{glob}");
    }
}

/// Another share on the same server, another server, and every other spelling of the own
/// share or matter stay as they are (the data fence refuses them unasked).
#[test]
fn other_shares_servers_and_spellings_are_not_answered() {
    for path in [
        r"\\srv\other\Matters\A\x.docx",
        r"\\srv\share\Matters\B\x.docx",
        r"\\srv\share\Matters\x.docx",
        r"\\evil\share\Matters\A\x.docx",
        r"\\10.0.0.5\share\Matters\A\x.docx",
        r"\\srv.corp.example\share\Matters\A\x.docx",
        r"\\srv@SSL\share\Matters\A\x.docx",
        r"\\srv@SSL@443\share\Matters\A\x.docx",
        r"\\srv@8443\share\Matters\A\x.docx",
        r"\\srv.\share\Matters\A\x.docx",
        r"\\?\UNC\srv\share\Matters\A\x.docx",
        "//?/UNC/srv/share/Matters/A/x.docx",
        r"\\SRV\SHARE\MATTERS\A\x.docx",
        r"\\srv\share\Matters\a\x.docx",
        r"\\srv\share\MATTER~1\A\x.docx",
        r"\\srv\share\Matters\A\..\B\x.docx",
        r"\\srv\share\Matters\A\.\x.docx",
        r"\\srv\share\Matters\A\..",
        r"\\srv\share\Matters\A\x.",
        r"\\srv\share\Matters\A\x ",
        r"\\srv\share\Matters\A\...\x",
        r"\\srv\share\Matters\A\\x.docx",
        r"\\srv\share\Matters\A\x.docx:hidden",
        r"\\srv\share\Matters\A\x.docx::$DATA",
        r"\\srv\share\Matters\A\CON",
        r"\\srv\share\Matters\A\nul.txt",
        r"\\srv\share\Matters\A\*.docx",
        "\\\\srv\\share\\Matters\\A\\x\u{1}y",
        r"Z:\Matters\A\x.docx",
        r"x.docx",
        r"Matters\A\x.docx",
        r"~\x.docx",
        "",
    ] {
        assert!(!read(path), "{path}");
    }
}

/// A Grep or Glob whose folder, pattern or filter leaves the matter is not answered.
#[test]
fn searches_that_escape_are_not_answered() {
    for glob in [
        json!({ "pattern": r"\\evil\s\*" }),
        json!({ "pattern": "//evil/s/*" }),
        json!({ "pattern": r"C:\*" }),
        json!({ "pattern": "C:*" }),
        json!({ "pattern": "../*" }),
        json!({ "pattern": "a/../../*" }),
        json!({ "pattern": "{..,a}/*" }),
        json!({ "pattern": "~/*" }),
        json!({ "pattern": "/etc/*" }),
        json!({ "pattern": "" }),
        json!({ "pattern": "*", "path": r"\\srv\other" }),
        json!({ "pattern": "*", "path": r"\\srv\share\Matters" }),
        json!({ "pattern": "*", "path": "" }),
        json!({ "pattern": "*", "path": null }),
        json!({ "path": MATTER }),
    ] {
        assert!(!allowed("Glob", &glob), "{glob}");
    }
    for grep in [
        json!({ "pattern": "x", "glob": "../**" }),
        json!({ "pattern": "x", "glob": r"\\evil\s\*" }),
        json!({ "pattern": "x", "glob": "D:/**" }),
        json!({ "pattern": "x", "path": r"\\evil\share\Matters\A" }),
        json!({ "pattern": "x", "path": r"\\srv\share\Matters\A\.." }),
        json!({ "pattern": "x", "glob": 3 }),
        json!({ "pattern": 3 }),
        json!({ "path": MATTER }),
    ] {
        assert!(!allowed("Grep", &grep), "{grep}");
    }
    // No `path`: the agent's working folder decides.
    let elsewhere = |tool: &str, input: &Value, cwd: &str| {
        own_share_read(Some(tool), Some(input), Path::new(MATTER), Path::new(cwd))
    };
    let bare = json!({ "pattern": "*" });
    assert!(!elsewhere("Glob", &bare, r"\\srv\share\Matters"));
    assert!(!elsewhere("Grep", &bare, r"C:\Users\a"));
    assert!(elsewhere("Grep", &bare, r"\\srv\share\Matters\A\Drafts"));
}

/// Changes, other tools, an unknown key or shape, and no tool name still ask.
#[test]
fn changes_and_anything_unknown_still_ask() {
    let file = json!({ "file_path": r"\\srv\share\Matters\A\x.docx" });
    for tool in [
        "Edit",
        "Write",
        "MultiEdit",
        "NotebookEdit",
        "Bash",
        "PowerShell",
        "WebFetch",
        "mcp__files__Read",
        "read",
        "READ",
        "NotebookRead",
        "LS",
    ] {
        assert!(!allowed(tool, &file), "{tool}");
    }
    let edit = json!({
        "file_path": r"\\srv\share\Matters\A\x.docx",
        "old_string": "a",
        "new_string": "b",
    });
    assert!(!allowed("Read", &edit));
    assert!(!allowed(
        "Read",
        &json!({ "file_path": r"\\srv\share\Matters\A\x", "x": 1 })
    ));
    assert!(!allowed(
        "Grep",
        &json!({ "pattern": "x", "paths": [MATTER] })
    ));
    assert!(!allowed("Read", &json!([r"\\srv\share\Matters\A\x.docx"])));
    assert!(!allowed("Read", &json!({ "file_path": 7 })));
    assert!(!allowed("Read", &json!({})));
    let matter = Path::new(MATTER);
    assert!(!own_share_read(None, Some(&file), matter, matter));
    assert!(!own_share_read(Some("Read"), None, matter, matter));
}

/// Windows' default link policy (nothing set, or 0) follows no link on a share; anything else,
/// or a value that can't be read, may.
#[test]
fn the_link_policy_allows_answers_only_at_its_default() {
    assert!(!links_followed(&[Ok(None), Ok(None), Ok(None), Ok(None)]));
    assert!(!links_followed(&[
        Ok(Some(0)),
        Ok(None),
        Ok(Some(0)),
        Ok(None)
    ]));
    assert!(links_followed(&[Ok(Some(1)), Ok(None), Ok(None), Ok(None)]));
    assert!(links_followed(&[Ok(None), Ok(None), Ok(None), Ok(Some(1))]));
    assert!(links_followed(&[Ok(None), Err(()), Ok(None), Ok(None)]));
}
