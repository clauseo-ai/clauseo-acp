//! The one native confirm before an import (A1's `McpLaunchConfirm`): Allow imports every
//! listed server turned on; Cancel imports nothing and answers `mcp.save_declined`; an import
//! with nothing to import never asks. Split from `mcp_import.rs` for its length.

use std::sync::Mutex;

use clauseo_host::{McpLaunchAsk, McpLaunchConfirm};

use super::*;

/// A confirm that answers every import `allow` and records what each import listed.
pub(super) struct Answers {
    allow: bool,
    imports: Mutex<Vec<Vec<McpLaunchAsk>>>,
}

impl Answers {
    pub(super) fn allowing(allow: bool) -> Self {
        Self {
            allow,
            imports: Mutex::new(Vec::new()),
        }
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

    fn confirm_import(&self, asks: &[McpLaunchAsk]) -> bool {
        self.imports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(asks.to_vec());
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
    };
    let notes = McpLaunchAsk {
        server: "notes".to_owned(),
        program: Vec::new(),
        url: Some("https://notes.example.test/mcp".to_owned()),
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
