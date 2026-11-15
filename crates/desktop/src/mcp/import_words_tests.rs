//! The import confirm's words: the list in both languages, the cap, and the reviewed copy.

use clauseo_host::McpLaunchAsk;
use clauseo_protocol::Language;

use super::{EN, HI, import_words};

fn local(server: &str, program: &[&str]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: program.iter().map(|part| (*part).to_owned()).collect(),
        url: None,
        names: Vec::new(),
    }
}

fn remote(server: &str, url: &str) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: Vec::new(),
        url: Some(url.to_owned()),
        names: Vec::new(),
    }
}

#[test]
fn mcp_import_words_list_each_server_with_what_it_starts() {
    let asks = [
        local("github", &["npx", "-y", "server-github"]),
        local("files", &["uvx", "/Users/ana/Case Files"]),
        remote("notes", "https://notes.example/mcp"),
    ];
    let words = import_words(&asks, &[], Language::En);
    assert_eq!(words.title, "Let Clauseo run these programs?");
    assert_eq!(
        words.message,
        "These will start when you work in a matter: github (npx -y server-github), \
         files (uvx '/Users/ana/Case Files'), notes (https://notes.example/mcp). Only allow \
         programs you trust."
    );
    assert_eq!(
        (words.ok.as_str(), words.cancel.as_str()),
        ("Allow", "Cancel")
    );
    let hindi = import_words(&asks, &[], Language::Hi);
    assert_eq!(hindi.title, HI.title);
    assert!(hindi.message.contains("github (npx -y server-github)"));
    assert_eq!(hindi.ok, "अनुमति दें");
}

/// Every server it is given is listed, whole: the host gives only those that fit.
#[test]
fn mcp_import_words_list_every_server_and_their_names() {
    let mut asks: Vec<McpLaunchAsk> = (0..12).map(|n| local(&format!("s{n}"), &["npx"])).collect();
    asks[0].names = vec!["GITHUB_TOKEN".to_owned()];
    asks[11].names = vec!["NODE_OPTIONS".to_owned()];
    let words = import_words(&asks, &[], Language::En);
    assert!(
        words.message.contains("s11 (npx). Only allow"),
        "{}",
        words.message
    );
    assert!(
        words
            .message
            .ends_with("\n\nNew or changed settings: GITHUB_TOKEN (s0), NODE_OPTIONS (s11)."),
        "{}",
        words.message
    );
}

#[test]
fn mcp_import_words_fill_each_slot_once() {
    let asks = [local("odd", &["tool", "{count}", "{list}"])];
    let words = import_words(&asks, &[], Language::En);
    assert!(
        words.message.contains("odd (tool {count} {list})"),
        "{}",
        words.message
    );
}

/// Every word here, in both languages, is in its review home under its id (UC-231 on).
#[test]
fn mcp_import_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-uc-mcp-import.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    let column = |table: &super::ImportTable| {
        [
            table.title,
            table.message,
            table.left_named,
            table.left_one_named,
            table.left,
            table.left_one,
        ]
    };
    let ids = [231, 232, 252, 253, 254, 255];
    for (n, (en, hi)) in ids
        .into_iter()
        .zip(column(&EN).into_iter().zip(column(&HI)))
    {
        let id = format!("\"UC-{n}\"");
        assert!(registry.contains(&id), "no {id}");
        for words in [en, hi] {
            let quoted = format!("\"{words}\"");
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}

/// The servers that did not fit get a line of their own after the list: how many, and their
/// names when they fit; none of them is in the list.
#[test]
fn mcp_import_words_say_how_many_did_not_fit_and_which() {
    let asks = [local("github", &["npx"])];
    let later = ["notes".to_owned(), "drafts".to_owned()];
    let words = import_words(&asks, &later, Language::En);
    assert!(
        words.message.ends_with(
            "programs you trust.\n\n2 more tools didn't fit here and won't be imported now: \
             notes, drafts. Import again to add them."
        ),
        "{}",
        words.message
    );
    let one = import_words(&asks, &later[..1], Language::En);
    assert!(
        one.message.ends_with(
            "1 more tool didn't fit here and won't be imported now: notes. Import again to add it."
        ),
        "{}",
        one.message
    );
    // Too many names to show: the count alone.
    let many: Vec<String> = (0..150).map(|n| format!("tool{n:03}")).collect();
    let counted = import_words(&asks, &many, Language::En);
    assert!(
        counted.message.ends_with(
            "150 more tools didn't fit here and won't be imported now. Import again to add them."
        ),
        "{}",
        counted.message
    );
    assert!(!counted.message.contains("tool000"));
    let hindi = import_words(&asks, &later, Language::Hi);
    assert!(hindi.message.contains("2 और टूल"), "{}", hindi.message);
    assert!(hindi.message.contains("notes, drafts"), "{}", hindi.message);
    let none = import_words(&asks, &[], Language::En);
    assert!(none.message.ends_with("programs you trust."));
}
