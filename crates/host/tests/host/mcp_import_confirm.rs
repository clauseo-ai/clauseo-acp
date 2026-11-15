//! The one native confirm before an import (A1's `McpLaunchConfirm`): Allow imports every
//! listed server turned on; Cancel imports nothing and answers `mcp.save_declined`; an import
//! with nothing to import never asks; one too long to show whole in one confirm keeps the servers
//! that fit, whole and in order, and names the rest, which the next import adds. Split from
//! `mcp_import.rs` for its length.

use std::sync::Mutex;

use clauseo_host::{McpLaunchAsk, McpLaunchConfirm, mcp_shown};

use super::*;

/// A confirm that answers every import `allow` and records what each import listed.
pub(super) struct Answers {
    allow: bool,
    imports: Mutex<Vec<Vec<McpLaunchAsk>>>,
    left_out: Mutex<Vec<Vec<String>>>,
}

impl Answers {
    pub(super) fn allowing(allow: bool) -> Self {
        Self {
            allow,
            imports: Mutex::new(Vec::new()),
            left_out: Mutex::new(Vec::new()),
        }
    }

    /// The servers each import's confirm said were left out, in order.
    fn left_out(&self) -> Vec<Vec<String>> {
        self.left_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The lists each import showed, in order.
    fn imports(&self) -> Vec<Vec<McpLaunchAsk>> {
        self.imports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl McpLaunchConfirm for Answers {
    fn confirm(&self, _: &McpLaunchAsk) -> bool {
        panic!("an import asked the save's confirm");
    }

    fn confirm_import(&self, asks: &[McpLaunchAsk], left_out: &[String]) -> bool {
        self.imports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(asks.to_vec());
        self.left_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(left_out.to_vec());
        self.allow
    }
}

/// A host over the apps' config with `answers` as the confirm.
fn start(dir: &Dir, store: &Arc<MemoryStore>, answers: &Arc<Answers>) -> crate::support::Running {
    let answers = Arc::clone(answers);
    dir.start_with(0, |config| {
        wired(config, store, apps()).with_mcp_confirm(answers)
    })
}

/// Allow: one ask listing each server with what it starts, then both saved turned on.
#[test]
fn mcp_import_allow_asks_once_with_every_server_and_imports_them_on() {
    let dir = Dir::new("mcp_import_allow", &[]);
    let (store, answers) = (
        Arc::new(MemoryStore::new()),
        Arc::new(Answers::allowing(true)),
    );
    let host = start(&dir, &store, &answers);
    let mut sub = subscribe(&host);
    let (view, last) = imported(&host, &mut sub, ClaudeDesktop);
    assert_eq!(last.imported, ["github", "notes"]);
    let github = McpLaunchAsk {
        server: "github".to_owned(),
        program: vec![
            npx_name().to_owned(),
            "-y".to_owned(),
            "server-github".to_owned(),
        ],
        url: None,
        names: vec!["GITHUB_TOKEN".to_owned()],
    };
    let notes = McpLaunchAsk {
        server: "notes".to_owned(),
        program: Vec::new(),
        url: Some("https://notes.example.test/mcp".to_owned()),
        names: vec!["Authorization".to_owned()],
    };
    assert_eq!(answers.imports(), [vec![github, notes]]);
    assert!(view.servers.iter().all(|server| server.enabled), "{view:?}");
    let asked = format!("{:?}", answers.imports());
    assert!(!asked.contains(ENV_VALUE) && !asked.contains(HEADER_VALUE));
    host.shutdown();
}

/// Cancel: nothing saved, no keychain write, `mcp.save_declined`.
#[test]
fn mcp_import_cancel_imports_nothing() {
    let dir = Dir::new("mcp_import_cancel", &[]);
    let (store, answers) = (
        Arc::new(MemoryStore::new()),
        Arc::new(Answers::allowing(false)),
    );
    let host = start(&dir, &store, &answers);
    let before = dir.settings_json();
    host.refused(import(Codex), ErrorCode::McpSaveDeclined);
    assert_eq!(answers.imports().len(), 1);
    assert_eq!(store.writes(), 0, "a declined import wrote to the keychain");
    assert_eq!(dir.settings_json(), before);
    host.shutdown();
}

/// Nothing to import (every server held, or blocked): no ask, and the import answers `Done`.
#[test]
fn mcp_import_with_nothing_to_import_never_asks() {
    let dir = Dir::new("mcp_import_nothing", &[]);
    let held = json!({"command": program()});
    dir.set_settings(&[], json!({"mcp": {"servers": {"drafts": held}}}));
    let (store, answers) = (
        Arc::new(MemoryStore::new()),
        Arc::new(Answers::allowing(true)),
    );
    let host = start(&dir, &store, &answers);
    let mut sub = subscribe(&host);
    // Codex: `drafts` held, `billing` blocked, `paused` off.
    let (_, last) = imported(&host, &mut sub, Codex);
    assert_eq!(last.imported, Vec::<String>::new());
    assert_eq!(answers.imports(), Vec::<Vec<McpLaunchAsk>>::new());
    host.shutdown();
}

/// An import whose list would pass A1's 2,000 characters, as shown, keeps whole servers in name
/// order while they fit and leaves out the rest whole: the confirm and the result name them
/// (`too_many`), and the next import adds the next ones. A server too long to show even alone is
/// skipped as `unreadable`, and never blocks the others.
#[test]
fn mcp_import_too_long_for_one_confirm_keeps_a_whole_prefix_and_names_the_rest() {
    let dir = Dir::new("mcp_import_too_large", &[]);
    let long = "x".repeat(150);
    let fs = too_many_apps(&long);
    let (store, answers) = (
        Arc::new(MemoryStore::new()),
        Arc::new(Answers::allowing(true)),
    );
    let answered = Arc::clone(&answers);
    let host = dir.start_with(0, |config| {
        wired(config, &store, fs).with_mcp_confirm(answered)
    });
    let mut sub = subscribe(&host);
    let all: Vec<String> = (0..20).map(|n| format!("tool{n:02}")).collect();
    let (_, first) = imported(&host, &mut sub, ClaudeDesktop);
    let shown = &answers.imports()[0];
    let kept = shown.len();
    assert!(kept > 0 && kept < 20, "{kept} kept");
    // Whole servers, in order, each with every argument.
    let names: Vec<&str> = shown.iter().map(|ask| ask.server.as_str()).collect();
    assert_eq!(names, all[..kept]);
    assert!(shown.iter().all(|ask| ask.program[1] == long));
    // Within the cap as shown; one more server would pass it.
    assert!(mcp_shown::import_chars(shown) <= mcp_shown::MAX_ASK_CHARS);
    let mut one_more = shown.clone();
    one_more.push(McpLaunchAsk {
        server: all[kept].clone(),
        ..shown[0].clone()
    });
    assert!(mcp_shown::import_chars(&one_more) > mcp_shown::MAX_ASK_CHARS);
    // The confirm named the rest, and the result names them and the one too long alone.
    assert_eq!(answers.left_out()[0], all[kept..]);
    assert_eq!(first.imported, all[..kept]);
    let mut skipped: Vec<McpSkipped> = all[kept..]
        .iter()
        .map(|name| skip(name, McpSkipReason::TooMany, None))
        .collect();
    skipped.insert(0, skip("huge", McpSkipReason::Unreadable, None));
    assert_eq!(first.skipped, skipped);
    assert_eq!(first.others, 0);
    // Only the kept servers' values reached the keychain; a left-out one wrote nothing.
    for name in &all[..kept] {
        assert!(
            store.peek(&format!("{name}/env/TOOL_TOKEN")).is_some(),
            "{name}"
        );
    }
    for name in all[kept..].iter().chain([&"huge".to_owned()]) {
        assert_eq!(
            store.peek(&format!("{name}/env/TOOL_TOKEN")),
            None,
            "{name}"
        );
    }
    assert_eq!(store.writes(), kept);
    // The next import takes the next whole servers.
    host.ok(import(ClaudeDesktop));
    let view = view_until(&mut sub, |view| {
        view.last_import
            .as_ref()
            .is_some_and(|last| last.imported != first.imported)
    });
    let second = view.last_import.unwrap();
    let next = answers.imports()[1].len();
    assert!(next > 0);
    assert_eq!(second.imported, all[kept..kept + next]);
    host.shutdown();
}

/// Twenty servers with a 150-character argument and a value each, and `huge`, too long to show.
fn too_many_apps(long: &str) -> FakeFs {
    let mut servers: serde_json::Map<String, Value> = (0..20)
        .map(|n| {
            let server = json!({"command": program(), "args": [long],
                "env": {"TOOL_TOKEN": ENV_VALUE}});
            (format!("tool{n:02}"), server)
        })
        .collect();
    servers.insert(
        "huge".to_owned(),
        json!({"command": program(), "args": ["y".repeat(2_100)], "env": {"TOOL_TOKEN": ENV_VALUE}}),
    );
    FakeFs::default().file(&program(), "").file(
        &desktop_config(),
        &json!({ "mcpServers": servers }).to_string(),
    )
}

/// Cancel on a partial import: nothing at all is imported, kept or left out, and the keychain and
/// settings are untouched; the confirm still named the ones left out.
#[test]
fn mcp_import_declined_partial_import_writes_nothing() {
    let dir = Dir::new("mcp_import_partial_declined", &[]);
    let fs = too_many_apps(&"x".repeat(150));
    let (store, answers) = (
        Arc::new(MemoryStore::new()),
        Arc::new(Answers::allowing(false)),
    );
    let answered = Arc::clone(&answers);
    let host = dir.start_with(0, |config| {
        wired(config, &store, fs).with_mcp_confirm(answered)
    });
    let before = dir.settings_json();
    host.refused(import(ClaudeDesktop), ErrorCode::McpSaveDeclined);
    assert_eq!(answers.imports().len(), 1);
    assert!(!answers.left_out()[0].is_empty(), "nothing was left out");
    assert_eq!(store.writes(), 0);
    assert_eq!(dir.settings_json(), before);
    host.shutdown();
}
