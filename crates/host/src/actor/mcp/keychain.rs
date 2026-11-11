//! The keychain half of a change to the lawyer's MCP servers: the steps a save plans on the
//! actor ([`plan`]), and the blocking keychain calls that run them on a thread of their own
//! ([`run`], [`put_back`], [`delete`]), over `clauseo_connectors::SecretStore`.
//!
//! - A value the lawyer sets is written to `keychain_account(server, kind, name)`; a kept value
//!   of a renamed server is copied from the old name's account to the new one's.
//! - Before each write the account's old item is read, so a failed step, or a settings write
//!   that fails after all of them, puts every account back as it was ([`Undo`]), as far as the
//!   keychain lets it: a put-back that fails is logged, by count.
//! - Items no longer named (a value the server dropped, a rename's old items, a removed server's)
//!   are deleted only once the settings are written ([`Plan::after`]): until then the old
//!   settings still name them. A delete that fails leaves an item nothing names, logged by count.
//! - Accounts that differ only in case are one item in Credential Manager, so an old account
//!   that matches a new one in any case is never deleted.
//! - No value is logged, printed or put in an error: errors are the store's, which name the
//!   keychain's error kind only.

use std::collections::BTreeMap;

use clauseo_connectors::{SecretStore, StoreError};
use clauseo_protocol::{ErrorCode, McpSecretEdit, McpSecretValue, McpServerEdit, McpServerKind};
use clauseo_settings::{
    KeychainRef, McpRefusal, McpServerLaunch, McpTransport, McpValue, McpValueKind,
    keychain_account,
};
use zeroize::Zeroizing;

use crate::error::Error;

/// One keychain step of a save, in order.
pub(crate) enum Step {
    /// Write the value at the account.
    Set {
        account: String,
        value: Zeroizing<String>,
    },
    /// Copy the item at `from` to `to`: a renamed server's kept value. A missing item is
    /// skipped: the server then lacks it, as it did before.
    Copy { from: String, to: String },
}

/// What a save does in the keychain.
#[derive(Default)]
pub(crate) struct Plan {
    /// The writes, before the settings.
    pub(crate) steps: Vec<Step>,
    /// The accounts deleted once the settings are written.
    pub(crate) after: Vec<String>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Counts and accounts only: a step holds a value.
        f.debug_struct("Plan")
            .field("steps", &self.steps.len())
            .field("after", &self.after)
            .finish()
    }
}

impl std::fmt::Debug for Undo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Undo")
            .field("written", &self.0.len())
            .finish()
    }
}

/// Each account a run wrote, with what it held before, in the order written.
#[derive(Default)]
pub(crate) struct Undo(Vec<(String, Option<Zeroizing<Vec<u8>>>)>);

impl Undo {
    /// Whether the run wrote nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The server `edit` makes, replacing `previous` (its name and entry; `None` for an add), and
/// the keychain steps that go with it. A kept value that `previous` does not hold (under the
/// same kind) is `command.invalid`; a name given twice is the kind's name refusal. Every set
/// value is stored as `{"keychain": true}`; a kept plain value stays plain.
pub(crate) fn plan(
    previous: Option<(&str, &McpServerLaunch)>,
    edit: &McpServerEdit,
) -> Result<(McpServerLaunch, Plan), Error> {
    let kind = match edit.kind {
        McpServerKind::Local => McpValueKind::Env,
        McpServerKind::Remote => McpValueKind::Header,
    };
    let edits = match kind {
        McpValueKind::Env => &edit.env,
        McpValueKind::Header => &edit.headers,
    };
    let held = previous
        .filter(|(_, server)| McpValueKind::of(server) == kind)
        .map(|(name, server)| (name, server.values()));
    let mut plan = Plan::default();
    let mut values = BTreeMap::new();
    for McpSecretEdit { name: var, value } in edits {
        if values.contains_key(var) {
            return Err(twice(kind, &edit.name));
        }
        let stored = match value {
            McpSecretValue::Set(text) => {
                plan.steps.push(Step::Set {
                    account: keychain_account(&edit.name, kind, var),
                    value: Zeroizing::new(text.expose().to_owned()),
                });
                McpValue::Keychain(KeychainRef::default())
            }
            McpSecretValue::Keep {} => {
                let Some((old_name, held_value)) =
                    held.and_then(|(old, values)| Some((old, values.get(var)?)))
                else {
                    return Err(Error::refused(
                        ErrorCode::CommandInvalid,
                        "a kept value that the server does not hold",
                    ));
                };
                if matches!(held_value, McpValue::Keychain(_)) && old_name != edit.name {
                    plan.steps.push(Step::Copy {
                        from: keychain_account(old_name, kind, var),
                        to: keychain_account(&edit.name, kind, var),
                    });
                }
                held_value.clone()
            }
        };
        values.insert(var.clone(), stored);
    }
    let transport = match kind {
        McpValueKind::Env => McpTransport::Local {
            command: edit.command.clone().unwrap_or_default(),
            args: edit.args.clone(),
            env: values,
        },
        McpValueKind::Header => McpTransport::Remote {
            url: edit.url.clone().unwrap_or_default(),
            headers: values,
        },
    };
    let server = McpServerLaunch {
        transport,
        enabled: edit.enabled,
    };
    if let Some((old_name, old)) = previous {
        let kept = accounts(&edit.name, &server);
        plan.after = accounts(old_name, old)
            .into_iter()
            .filter(|old| !kept.iter().any(|new| new.eq_ignore_ascii_case(old)))
            .collect();
    }
    Ok((server, plan))
}

/// The keychain accounts `server`, named `name`, names.
pub(crate) fn accounts(name: &str, server: &McpServerLaunch) -> Vec<String> {
    let kind = McpValueKind::of(server);
    server
        .values()
        .iter()
        .filter(|(_, value)| matches!(value, McpValue::Keychain(_)))
        .map(|(var, _)| keychain_account(name, kind, var))
        .collect()
}

fn twice(kind: McpValueKind, server: &str) -> Error {
    let refusal = match kind {
        McpValueKind::Env => McpRefusal::EnvNameInvalid,
        McpValueKind::Header => McpRefusal::HeaderNameInvalid,
    };
    Error::Settings(clauseo_settings::Error::Mcp {
        key: format!("mcp.servers.{server}"),
        refusal,
    })
}

/// Runs `steps` in order. On the first failure every account already written is put back and
/// the failure returned; otherwise what each held before, for [`put_back`].
pub(crate) fn run(store: &dyn SecretStore, steps: Vec<Step>) -> Result<Undo, StoreError> {
    let mut undo = Undo::default();
    for step in steps {
        if let Err(error) = one(store, step, &mut undo) {
            put_back(store, undo);
            return Err(error);
        }
    }
    Ok(undo)
}

/// One step: the account's old item read, then the write; the account joins `undo` once
/// written.
fn one(store: &dyn SecretStore, step: Step, undo: &mut Undo) -> Result<(), StoreError> {
    let (account, value) = match step {
        Step::Set { account, value } => (account, Zeroizing::new(value.as_bytes().to_vec())),
        Step::Copy { from, to } => match store.get(&from)? {
            Some(bytes) => (to, Zeroizing::new(bytes)),
            None => return Ok(()),
        },
    };
    let before = store.get(&account)?.map(Zeroizing::new);
    store.set(&account, &value)?;
    undo.0.push((account, before));
    Ok(())
}

/// Puts each account in `undo` back, last written first: its old item, or none. A failure is
/// logged by count and the rest go on.
pub(crate) fn put_back(store: &dyn SecretStore, undo: Undo) {
    let mut failed = 0_usize;
    for (account, before) in undo.0.into_iter().rev() {
        let result = match before {
            Some(bytes) => store.set(&account, &bytes),
            None => store.delete(&account),
        };
        if result.is_err() {
            failed += 1;
        }
    }
    if failed > 0 {
        tracing::warn!(
            failed,
            "keychain values of an MCP server could not be put back after a failed save"
        );
    }
}

/// Deletes each account; a failure is logged by count and the rest go on.
pub(crate) fn delete(store: &dyn SecretStore, accounts: &[String]) {
    let failed = accounts
        .iter()
        .filter(|account| store.delete(account).is_err())
        .count();
    if failed > 0 {
        tracing::warn!(
            failed,
            "keychain values no MCP server names any more could not be deleted"
        );
    }
}
