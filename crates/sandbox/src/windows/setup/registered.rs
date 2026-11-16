//! The registered matters a set-up fences (step 8), as the setup exe finds them (S15 `PR4h`):
//! the exe takes no matter on its command line, so it reads them from the owner's files. Each
//! matter `matters.json` lists as holding a pool user, with the folders its own ledger entries
//! name: its matter, state and notes folders (the newest entry of each kind, as a moved folder
//! leaves older ones). A matter whose ledger names no matter or no state folder never launched
//! on tier B; it is left out, and its first launch fences it (`RegisteredMatter::fenced`).
//!
//! Both files are the owner's and the owner can write them, so the exe reads them, and fences
//! what they name, only while acting as the owner (K1): a false path gets no more than the
//! owner could write there anyway. Pure: tested on every OS.

use std::path::PathBuf;

use crate::windows::admit::PathKind;
use crate::windows::fence::{MatterFolders, RegisteredMatter};
use crate::windows::ledger::{Entry, Ledger};
use crate::windows::matters::Rows;

/// Every matter of `rows` holding a pool user whose folders `ledger` names, as the module says,
/// in matter order.
pub fn registered(rows: &Rows, ledger: &Ledger) -> Vec<RegisteredMatter> {
    let mut found = Vec::new();
    for (id, row) in rows {
        let Some(pool_user) = row.holds() else {
            continue;
        };
        let newest = |kind: PathKind| -> Option<PathBuf> {
            ledger
                .entries()
                .filter(|entry| entry.matter == *id && entry.kind == kind)
                .max_by_key(|entry: &&Entry| entry.id)
                .map(|entry| entry.path.clone())
        };
        let (Some(root), Some(state)) = (newest(PathKind::Matter), newest(PathKind::State)) else {
            tracing::debug!(%id, "a matter with a pool user but no folders in the ledger");
            continue;
        };
        found.push(RegisteredMatter {
            id: *id,
            folders: MatterFolders {
                root,
                state,
                notes: newest(PathKind::Notes),
            },
            pool_user: pool_user.clone(),
            fenced: false,
        });
    }
    found
}
