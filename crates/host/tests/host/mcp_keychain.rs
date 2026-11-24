//! The lawyer's MCP values when the keychain refuses a read (an error, not "no item", as macOS
//! answers before the lawyer presses Allow): the server is left out and its value shows unsaved,
//! as a missing one does; `KeychainReadAgain` reads again only the refused values, and the
//! view and the skills and tools list go out once one reads.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_connectors::{MemoryStore, SecretStore, StoreError};
use clauseo_protocol::{CommandOutcome, HostCommand, McpSecretName, McpToolsView};
use serde_json::json;

use crate::addons::addons_changed;
use crate::mcp_tools::{
    account, finds_npx, npx, one_turn, remove, scenario, settings, view_until, wired,
};
use crate::protection_rig::subscribe;
use crate::support::Dir;

/// A keychain that refuses reads of `refused` accounts while `now`, counting each
/// account's reads; everything else is the memory store's.
struct Refusing {
    inner: Arc<MemoryStore>,
    refused: Vec<String>,
    now: AtomicBool,
    reads: Mutex<BTreeMap<String, usize>>,
}

impl Refusing {
    fn reads(&self, account: &str) -> usize {
        self.reads
            .lock()
            .unwrap()
            .get(account)
            .copied()
            .unwrap_or(0)
    }
}

impl SecretStore for Refusing {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        *self
            .reads
            .lock()
            .unwrap()
            .entry(account.to_owned())
            .or_default() += 1;
        if self.now.load(Ordering::SeqCst) && self.refused.iter().any(|a| a == account) {
            return Err(StoreError::Platform("a platform failure".to_owned()));
        }
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        self.inner.delete(account)
    }
}

/// The env names of `server` in `view`, with whether each shows saved.
fn env(view: &McpToolsView, server: &str) -> Vec<McpSecretName> {
    view.servers
        .iter()
        .find(|row| row.name == server)
        .map(|row| row.env.clone())
        .unwrap_or_default()
}

fn token(saved: bool) -> Vec<McpSecretName> {
    vec![McpSecretName {
        name: "TOKEN".to_owned(),
        saved,
    }]
}

/// `github`'s TOKEN is stored but its read refused; `notes`' TOKEN is not stored at all. A
/// launch leaves both out, each unsaved. Allowed, `KeychainReadAgain` reads github's again only:
/// it shows saved, with `McpToolsChanged` and `AddonsChanged`; notes' stays unsaved, unread.
#[test]
fn keychain_read_again_reads_only_the_refused_values() {
    let dir = Dir::new("mcp_keychain_read_again", &[]);
    let npx = npx(&dir);
    let scenario = scenario(&dir, "mcp-keychain-read-again", &json!([]));
    settings(
        &dir,
        scenario,
        &json!({
            "github": {"command": "npx", "env": {"TOKEN": {"keychain": true}}, "enabled": true},
            "notes": {"command": "npx", "env": {"TOKEN": {"keychain": true}}, "enabled": true},
        }),
    );
    let inner = Arc::new(MemoryStore::new());
    inner.put(&account("github", "TOKEN"), b"stored-but-refused-value");
    let keychain = Arc::new(Refusing {
        inner: Arc::clone(&inner),
        refused: vec![account("github", "TOKEN")],
        now: AtomicBool::new(true),
        reads: Mutex::default(),
    });
    let store: Arc<dyn SecretStore> = Arc::<Refusing>::clone(&keychain);
    let host = dir.start_with(0, |config| {
        wired(config, &inner, finds_npx(&npx)).with_mcp_secrets(store)
    });
    let mut sub = subscribe(&host);
    one_turn(&dir, &host);
    // As today: both show unsaved, the refused one as the missing one.
    let after = view_until(&mut sub, |view| {
        env(view, "github") == token(false) && env(view, "notes") == token(false)
    });
    assert_eq!(env(&after, "notes"), token(false));
    let (github, notes) = (account("github", "TOKEN"), account("notes", "TOKEN"));
    let (github_reads, notes_reads) = (keychain.reads(&github), keychain.reads(&notes));

    keychain.now.store(false, Ordering::SeqCst);
    assert_eq!(
        host.dispatch(HostCommand::KeychainReadAgain {}),
        Ok(CommandOutcome::Done)
    );
    let view = view_until(&mut sub, |view| env(view, "github") == token(true));
    assert_eq!(env(&view, "notes"), token(false));
    addons_changed(&mut sub);
    assert_eq!(keychain.reads(&github), github_reads + 1);
    assert_eq!(
        keychain.reads(&notes),
        notes_reads,
        "a missing value is not read again"
    );
    host.shutdown();
}

/// Removing a server whose value's last read was refused deletes the settings entry but keeps
/// that keychain item (what it holds is not known); a value read fine is deleted as before.
#[test]
fn removing_a_server_keeps_a_value_whose_read_was_refused() {
    let dir = Dir::new("mcp_keychain_remove", &[]);
    let npx = npx(&dir);
    let scenario = scenario(&dir, "mcp-keychain-remove", &json!([]));
    settings(
        &dir,
        scenario,
        &json!({
            "github": {"command": "npx", "env": {"TOKEN": {"keychain": true}}, "enabled": true},
        }),
    );
    let inner = Arc::new(MemoryStore::new());
    let github = account("github", "TOKEN");
    inner.put(&github, b"stored-but-refused-value");
    let keychain = Arc::new(Refusing {
        inner: Arc::clone(&inner),
        refused: vec![github.clone()],
        now: AtomicBool::new(true),
        reads: Mutex::default(),
    });
    let store: Arc<dyn SecretStore> = Arc::<Refusing>::clone(&keychain);
    let host = dir.start_with(0, |config| {
        wired(config, &inner, finds_npx(&npx)).with_mcp_secrets(store)
    });
    let mut sub = subscribe(&host);
    one_turn(&dir, &host);
    view_until(&mut sub, |view| env(view, "github") == token(false));
    host.ok(remove("github"));
    view_until(&mut sub, |view| {
        view.servers.iter().all(|row| row.name != "github")
    });
    host.settle();
    assert!(inner.peek(&github).is_some(), "a refused value was deleted");
    assert_eq!(inner.deletes(), 0);
    host.shutdown();
}
