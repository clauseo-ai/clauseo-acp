//! The native confirm before a lawyer's MCP server is added or what it starts changes (0028's
//! amendment): asked for an add and for a change of the command, arguments, variable names or
//! URL, never for a rename, a turn on or off or new values alone; Cancel answers
//! `mcp.save_declined` and leaves the settings and the keychain as they were.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_connectors::MemoryStore;
use clauseo_host::{McpLaunchAsk, McpLaunchConfirm};
use clauseo_protocol::ErrorCode;

use crate::mcp_tools::{HELD, account, finds_npx, keep, local, npx, save, set, store, wired};
use crate::support::{Dir, Running};

/// A confirm that records each ask and answers `allow`.
#[derive(Default)]
struct Recording {
    asks: Mutex<Vec<McpLaunchAsk>>,
    deny: AtomicBool,
}

impl McpLaunchConfirm for Recording {
    fn confirm(&self, ask: &McpLaunchAsk) -> bool {
        self.asks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ask.clone());
        !self.deny.load(Ordering::SeqCst)
    }
}

impl Recording {
    /// The asks so far, emptied.
    fn take(&self) -> Vec<McpLaunchAsk> {
        std::mem::take(
            &mut *self
                .asks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// A host with `store`, `npx` found, and `confirm` as the MCP confirm.
fn start(dir: &Dir, store: &Arc<MemoryStore>, confirm: &Arc<Recording>) -> Running {
    let npx = npx(dir);
    let confirm = Arc::clone(confirm);
    dir.start_with(0, |config| {
        wired(config, store, finds_npx(&npx)).with_mcp_confirm(confirm)
    })
}

/// An add asks, naming the program and its arguments verbatim; Allow saves it.
#[test]
fn mcp_tools_an_add_asks_and_allow_saves() {
    let dir = Dir::new("mcp_tools_confirm_allow", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    let mut server = local("github", "npx", vec![set("TOKEN", HELD)]);
    server.args = vec!["-y".to_owned(), "@firm/docs tool".to_owned()];
    host.ok(save(None, server));
    assert_eq!(
        confirm.take(),
        [McpLaunchAsk {
            server: "github".to_owned(),
            program: vec![
                "npx".to_owned(),
                "-y".to_owned(),
                "@firm/docs tool".to_owned()
            ],
            url: None,
            names: vec!["TOKEN".to_owned()],
        }]
    );
    assert_eq!(
        store.peek(&account("github", "TOKEN")).as_deref(),
        Some(HELD.as_bytes())
    );
    assert!(dir.settings_json().contains("github"));
    host.shutdown();
}

/// Cancel answers `mcp.save_declined`: nothing in the keychain, the file as it was.
#[test]
fn mcp_tools_cancel_saves_nothing() {
    let dir = Dir::new("mcp_tools_confirm_cancel", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    confirm.deny.store(true, Ordering::SeqCst);
    let before = std::fs::read_to_string(dir.data().join("settings.json")).ok();
    host.refused(
        save(None, local("github", "npx", vec![set("TOKEN", HELD)])),
        ErrorCode::McpSaveDeclined,
    );
    assert_eq!(confirm.take().len(), 1);
    assert_eq!(store.peek(&account("github", "TOKEN")), None);
    assert_eq!((store.writes(), store.deletes()), (0, 0));
    assert_eq!(
        std::fs::read_to_string(dir.data().join("settings.json")).ok(),
        before
    );
    host.shutdown();
}

/// A rename, a turn off and new values alone ask nothing; a change of the arguments alone, of
/// the command, or of the variables' names asks again.
#[test]
fn mcp_tools_only_a_change_of_what_starts_asks() {
    let dir = Dir::new("mcp_tools_confirm_when", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    assert_eq!(confirm.take().len(), 1, "the add");

    // A rename keeping everything else.
    host.ok(save(
        Some("github"),
        local("gh", "npx", vec![keep("TOKEN")]),
    ));
    // A new value only.
    host.ok(save(
        Some("gh"),
        local("gh", "npx", vec![set("TOKEN", "a-newer-value-0042")]),
    ));
    // Off, then on.
    for enabled in [false, true] {
        host.ok(clauseo_protocol::HostCommand::Mcp(
            clauseo_protocol::McpCommand::SetServerEnabled {
                name: "gh".to_owned(),
                enabled,
            },
        ));
    }
    assert_eq!(
        confirm.take(),
        Vec::new(),
        "a rename, a value or a toggle asked"
    );

    // The arguments alone.
    let mut args = local("gh", "npx", vec![keep("TOKEN")]);
    args.args = vec!["--read-only".to_owned()];
    host.ok(save(Some("gh"), args));
    let asks = confirm.take();
    assert_eq!(asks.len(), 1, "an args-only change did not ask");
    assert_eq!(asks[0].program, ["npx", "--read-only"]);

    // A new variable name.
    let mut names = local(
        "gh",
        "npx",
        vec![keep("TOKEN"), set("REGION", "uk-region-0042")],
    );
    names.args = vec!["--read-only".to_owned()];
    host.ok(save(Some("gh"), names));
    let asks = confirm.take();
    assert_eq!(asks.len(), 1, "a new variable name did not ask");
    assert_eq!(asks[0].names, ["REGION"], "the new name is not shown");
    host.shutdown();
}

/// A forged save that adds a loader variable to a trusted server names it in the confirm; a new
/// value for a loader variable asks again, naming it, while a new value for another does not.
#[test]
fn mcp_tools_a_loader_variable_is_named_and_its_value_asks() {
    let dir = Dir::new("mcp_tools_confirm_loader", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    confirm.take();
    host.ok(save(
        Some("github"),
        local(
            "github",
            "npx",
            vec![keep("TOKEN"), set("NODE_OPTIONS", "--require /tmp/evil.js")],
        ),
    ));
    let asks = confirm.take();
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].names, ["NODE_OPTIONS"]);
    assert_eq!(asks[0].program[0], "npx");
    host.ok(save(
        Some("github"),
        local(
            "github",
            "npx",
            vec![
                keep("TOKEN"),
                set("NODE_OPTIONS", "--require /tmp/other.js"),
            ],
        ),
    ));
    let asks = confirm.take();
    assert_eq!(asks.len(), 1, "a loader value change did not ask");
    assert_eq!(asks[0].names, ["NODE_OPTIONS"]);
    host.ok(save(
        Some("github"),
        local(
            "github",
            "npx",
            vec![set("TOKEN", "a-newer-token-0042"), keep("NODE_OPTIONS")],
        ),
    ));
    assert_eq!(confirm.take(), Vec::new(), "an ordinary value change asked");
    host.shutdown();
}

/// A rename and a change of what starts in one save asks.
#[test]
fn mcp_tools_a_rename_with_a_launch_change_asks() {
    let dir = Dir::new("mcp_tools_confirm_rename", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    confirm.take();
    let mut changed = local("gh", "npx", vec![keep("TOKEN")]);
    changed.args = vec!["--write".to_owned()];
    host.ok(save(Some("github"), changed));
    let asks = confirm.take();
    assert_eq!(asks.len(), 1, "a rename with new arguments did not ask");
    assert_eq!(asks[0].server, "gh");
    host.shutdown();
}

/// An ask too long to show whole is refused before any prompt: padding can't push a dangerous
/// argument out of view.
#[test]
fn mcp_tools_an_ask_too_long_to_show_is_refused_unasked() {
    let dir = Dir::new("mcp_tools_confirm_long", &[]);
    let (store, confirm) = (store(&[]), Arc::new(Recording::default()));
    let host = start(&dir, &store, &confirm);
    let mut padded = local("github", "npx", Vec::new());
    padded.args = vec![
        "a".repeat(1_000),
        "b".repeat(1_000),
        "--require=/tmp/evil.js".to_owned(),
    ];
    host.refused(save(None, padded), ErrorCode::McpCommandInvalid);
    assert_eq!(confirm.take(), Vec::new(), "it prompted");
    assert!(!dir.data().join("settings.json").exists() || !dir.settings_json().contains("evil"));
    host.shutdown();
}
