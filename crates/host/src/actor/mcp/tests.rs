//! The keychain half of a change to the lawyer's MCP servers, over the in-memory store: a save,
//! a rename that moves the items, a removal, a failed write put back, and a put-back after a
//! settings write that failed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clauseo_connectors::{MemoryStore, SecretStore, StoreError};
use clauseo_protocol::{
    McpSecretEdit, McpSecretValue, McpServerEdit, McpServerKind, RedactedString,
};
use clauseo_settings::{KeychainRef, McpServerLaunch, McpValue};
use zeroize::Zeroizing;

use super::keychain::{self, Step};

const TOKEN: &str = "secret-value-1";
const OTHER: &str = "secret-value-2";

fn set(name: &str, value: &str) -> McpSecretEdit {
    McpSecretEdit {
        name: name.to_owned(),
        value: McpSecretValue::Set(RedactedString::new(Zeroizing::new(value.to_owned())).unwrap()),
    }
}

fn keep(name: &str) -> McpSecretEdit {
    McpSecretEdit {
        name: name.to_owned(),
        value: McpSecretValue::Keep {},
    }
}

fn edit(name: &str, env: Vec<McpSecretEdit>) -> McpServerEdit {
    McpServerEdit {
        name: name.to_owned(),
        kind: McpServerKind::Local,
        command: Some("/opt/homebrew/bin/npx".to_owned()),
        args: Vec::new(),
        url: None,
        env,
        headers: Vec::new(),
        enabled: true,
    }
}

fn saved() -> McpValue {
    McpValue::Keychain(KeychainRef::default())
}

fn held(env: &[(&str, McpValue)]) -> McpServerLaunch {
    McpServerLaunch::local(
        "/opt/homebrew/bin/npx".to_owned(),
        Vec::new(),
        env.iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
        true,
    )
}

fn text(store: &MemoryStore, account: &str) -> Option<String> {
    store
        .peek(account)
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

#[test]
fn an_add_writes_each_set_value_and_stores_refs() {
    let store = MemoryStore::new();
    let (server, plan) = keychain::plan(None, &edit("github", vec![set("TOKEN", TOKEN)])).unwrap();
    assert_eq!(server, held(&[("TOKEN", saved())]));
    assert_eq!(plan.after, Vec::<String>::new());
    keychain::run(&store, plan.steps).unwrap();
    assert_eq!(text(&store, "github/env/TOKEN").as_deref(), Some(TOKEN));
}

#[test]
fn a_rename_copies_kept_items_and_deletes_the_old_ones_after() {
    let store = MemoryStore::new();
    store.put("gh/env/TOKEN", TOKEN.as_bytes());
    store.put("gh/env/DROPPED", b"old");
    let old = held(&[
        ("TOKEN", saved()),
        ("DROPPED", saved()),
        ("PLAIN", McpValue::Plain("p".to_owned())),
    ]);
    let (server, plan) = keychain::plan(
        Some(("gh", &old)),
        &edit(
            "github",
            vec![keep("TOKEN"), keep("PLAIN"), set("NEW", OTHER)],
        ),
    )
    .unwrap();
    assert_eq!(
        server,
        held(&[
            ("TOKEN", saved()),
            ("PLAIN", McpValue::Plain("p".to_owned())),
            ("NEW", saved()),
        ])
    );
    assert_eq!(plan.after, ["gh/env/DROPPED", "gh/env/TOKEN"]);
    keychain::run(&store, plan.steps).unwrap();
    assert_eq!(text(&store, "github/env/TOKEN").as_deref(), Some(TOKEN));
    assert_eq!(text(&store, "github/env/NEW").as_deref(), Some(OTHER));
    keychain::delete(&store, &plan.after);
    assert_eq!(store.peek("gh/env/TOKEN"), None);
    assert_eq!(store.peek("gh/env/DROPPED"), None);
}

#[test]
fn a_case_only_rename_never_deletes_the_item_it_kept() {
    let old = held(&[("TOKEN", saved())]);
    let (_, plan) =
        keychain::plan(Some(("github", &old)), &edit("GitHub", vec![keep("TOKEN")])).unwrap();
    assert!(plan.after.is_empty(), "{:?}", plan.after);
}

#[test]
fn a_removal_names_every_item_of_the_server() {
    let server = held(&[
        ("A", saved()),
        ("B", McpValue::Plain("b".to_owned())),
        ("C", saved()),
    ]);
    assert_eq!(
        keychain::accounts("github", &server),
        ["github/env/A", "github/env/C"]
    );
}

#[test]
fn a_kept_value_never_saved_and_a_name_twice_are_refused() {
    let error = keychain::plan(None, &edit("github", vec![keep("TOKEN")])).unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::CommandInvalid);
    let error = keychain::plan(
        None,
        &edit("github", vec![set("TOKEN", TOKEN), set("TOKEN", OTHER)]),
    )
    .unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::McpEnvNameInvalid);
    assert!(!format!("{error:?} {error}").contains(TOKEN));
}

#[test]
fn a_failed_write_puts_back_what_was_written() {
    let store = Arc::new(MemoryStore::new());
    store.put("github/env/A", b"old-a");
    let steps = vec![
        Step::Set {
            account: "github/env/A".to_owned(),
            value: Zeroizing::new("new-a".to_owned()),
        },
        Step::Set {
            account: "github/env/B".to_owned(),
            value: Zeroizing::new("new-b".to_owned()),
        },
    ];
    // The first write lands; the second fails.
    let failing = FailSecond::new(Arc::clone(&store));
    let error = keychain::run(&failing, steps).unwrap_err();
    assert!(!error.to_string().contains("new-"));
    assert_eq!(text(&store, "github/env/A").as_deref(), Some("old-a"));
    assert_eq!(store.peek("github/env/B"), None);
}

#[test]
fn a_put_back_after_a_failed_settings_write_restores_or_deletes() {
    let store = MemoryStore::new();
    store.put("github/env/A", b"old-a");
    let (_, plan) = keychain::plan(
        Some(("github", &held(&[("A", saved())]))),
        &edit("github", vec![set("A", "new-a"), set("B", "new-b")]),
    )
    .unwrap();
    let undo = keychain::run(&store, plan.steps).unwrap();
    assert_eq!(text(&store, "github/env/A").as_deref(), Some("new-a"));
    keychain::put_back(&store, undo);
    assert_eq!(text(&store, "github/env/A").as_deref(), Some("old-a"));
    assert_eq!(store.peek("github/env/B"), None);
}

#[test]
fn every_write_fails_with_the_store_and_nothing_stays() {
    let store = MemoryStore::new();
    store.fail_next_writes(1);
    let (_, plan) = keychain::plan(None, &edit("github", vec![set("A", TOKEN)])).unwrap();
    assert!(keychain::run(&store, plan.steps).is_err());
    assert_eq!(store.peek("github/env/A"), None);
}

/// A store whose second `set` fails; every other call goes to `inner`.
struct FailSecond {
    inner: Arc<MemoryStore>,
    sets: AtomicUsize,
}

impl FailSecond {
    fn new(inner: Arc<MemoryStore>) -> Self {
        Self {
            inner,
            sets: AtomicUsize::new(0),
        }
    }
}

impl SecretStore for FailSecond {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        if self.sets.fetch_add(1, Ordering::SeqCst) == 1 {
            return Err(StoreError::Platform("refused".to_owned()));
        }
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        self.inner.delete(account)
    }
}
