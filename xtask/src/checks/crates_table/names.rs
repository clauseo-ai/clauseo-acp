//! A "Never depends on" entry that is a package name but not exactly a row name: our crate
//! written `clauseo-<row>`, a crates.io package, or a name that looks like ours and is rejected
//! (see the module doc of `crates_table.rs`).

use super::{Never, fold, package_of};

/// Why an entry was rejected: the end of the message after "`<entry>` in the Never depends on
/// column of `<row>`", and the fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rejected {
    pub what: String,
    pub fix: String,
}

/// The entry `entry`, a package name that is no row name, given every row name in `rows`.
pub(super) fn crate_entry(entry: &str, rows: &[&str]) -> Result<Never, Rejected> {
    if let Some(rest) = entry.strip_prefix("clauseo-") {
        let name = fold(rest);
        return match rows.iter().find(|row| **row == name) {
            Some(row) => Ok(Never::Workspace((*row).to_owned())),
            None => Err(Rejected {
                what: format!(
                    "starts with `clauseo-`, but `{rest}` is no row of the Crates table, so it names none of our crates"
                ),
                fix: format!(
                    "write the row name of the crate you mean ({}), or add a row `{name}` for the crate",
                    near_rows_hint(&name, rows)
                ),
            }),
        };
    }
    let folded = fold(entry);
    if let Some(row) = rows.iter().find(|row| within_one_edit(&folded, row)) {
        let how = if folded == *row {
            format!("is the row `{row}` written with `-` for `_`")
        } else {
            format!("is one edit from the row `{row}`, so it reads as a typo of it")
        };
        return Err(Rejected {
            what: format!(
                "{how}, and as written it would ban a crates.io package `{entry}` instead of our `{package}`",
                package = package_of(row)
            ),
            fix: format!(
                "write `{row}` to ban our crate `{package}`: xtask/src/checks/crates_table.rs rejects a name within one edit of a row name once `-` is read as `_`",
                package = package_of(row)
            ),
        });
    }
    if entry.contains('_') {
        let dashed = entry.replace('_', "-");
        return Err(Rejected {
            what: "holds `_` but is no row of the Crates table, and only our row names are written with `_`".to_owned(),
            fix: format!(
                "write `{dashed}` for the crates.io package (cargo reads `-` and `_` in a package name the same), or the row name of the crate you mean if it is ours"
            ),
        });
    }
    Ok(Never::External(entry.to_owned()))
}

/// The part of the fix that says which row names to pick from: those one edit from `name`, else
/// the Crate column.
fn near_rows_hint(name: &str, rows: &[&str]) -> String {
    let near: Vec<String> = rows
        .iter()
        .filter(|row| within_one_edit(name, row))
        .map(|row| format!("`{row}`"))
        .collect();
    if near.is_empty() {
        "a name in the Crate column".to_owned()
    } else {
        format!("perhaps {}", near.join(" or "))
    }
}

/// True when `a` and `b` are equal or differ by one inserted, deleted or replaced character.
pub(super) fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if long.len() - short.len() > 1 {
        return false;
    }
    let same_start = short.iter().zip(&long).take_while(|(x, y)| x == y).count();
    let skip = usize::from(short.len() == long.len());
    short.get(same_start + skip..) == long.get(same_start + 1..)
}
