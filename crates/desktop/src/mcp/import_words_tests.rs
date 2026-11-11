//! The import confirm's words: the list in both languages, the cap, and the reviewed copy.

use clauseo_host::McpLaunchAsk;
use clauseo_protocol::Language;

use super::{EN, HI, MAX_LISTED, import_words};

fn local(server: &str, program: &[&str]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: program.iter().map(|part| (*part).to_owned()).collect(),
        url: None,
    }
}

fn remote(server: &str, url: &str) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: Vec::new(),
        url: Some(url.to_owned()),
    }
}

#[test]
fn mcp_import_words_list_each_server_with_what_it_starts() {
    let asks = [
        local("github", &["npx", "-y", "server-github"]),
        local("files", &["uvx", "/Users/ana/Case Files"]),
        remote("notes", "https://notes.example/mcp"),
    ];
    let words = import_words(&asks, Language::En);
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
    let hindi = import_words(&asks, Language::Hi);
    assert_eq!(hindi.title, HI.title);
    assert!(hindi.message.contains("github (npx -y server-github)"));
    assert_eq!(hindi.ok, "अनुमति दें");
}

#[test]
fn mcp_import_words_count_the_servers_past_the_cap() {
    let asks: Vec<McpLaunchAsk> = (0..MAX_LISTED + 3)
        .map(|n| local(&format!("s{n}"), &["npx"]))
        .collect();
    let words = import_words(&asks, Language::En);
    assert!(
        words.message.contains("s7 (npx), and 3 more."),
        "{}",
        words.message
    );
    assert!(!words.message.contains("s8 "), "{}", words.message);
}

#[test]
fn mcp_import_words_fill_each_slot_once() {
    let asks = [local("odd", &["tool", "{count}", "{list}"])];
    let words = import_words(&asks, Language::En);
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
    let column = |table: &super::ImportTable| [table.title, table.message, table.more];
    for (n, (en, hi)) in (231..).zip(column(&EN).into_iter().zip(column(&HI))) {
        let id = format!("\"UC-{n}\"");
        assert!(registry.contains(&id), "no {id}");
        for words in [en, hi] {
            let quoted = format!("\"{words}\"");
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}
