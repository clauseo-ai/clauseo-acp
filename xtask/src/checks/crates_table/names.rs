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

/// What follows a crates.io package name in a Never cell to mark it as one: `rmcp (crates.io)`.
pub(super) const CRATES_IO: &str = "(crates.io)";

/// The name of an entry written `<name> (crates.io)`, else `None`. Such an entry is the
/// crates.io package `<name>`, whatever the look-alike rules would say of it.
pub(super) fn marked_crates_io(entry: &str) -> Option<&str> {
    entry.strip_suffix(CRATES_IO).map(str::trim_end)
}

/// The entry `entry`, a package name that is no row name, given every row name in `rows`.
/// Names are compared ignoring case, with `-` read as `_`.
pub(super) fn crate_entry(entry: &str, rows: &[&str]) -> Result<Never, Rejected> {
    let lower = entry.to_ascii_lowercase();
    let folded = fold(&lower);
    if lower.starts_with("clauseo-") {
        return clauseo_entry(entry, &folded, rows);
    }
    if let Some(row) = rows.iter().find(|row| within_one_edit(&folded, row)) {
        let how = if folded == *row {
            let ways = match (entry != lower, lower != folded) {
                (true, true) => "other letter case and `-` for `_`",
                (true, false) => "other letter case",
                _ => "`-` for `_`",
            };
            format!("is the row `{row}` written with {ways}")
        } else {
            format!("is one edit from the row `{row}`, so it reads as a typo of it")
        };
        return Err(Rejected {
            what: format!(
                "{how}, and as written it would ban a crates.io package `{entry}` instead of our `{package}`",
                package = package_of(row)
            ),
            fix: format!(
                "write `{row}` to ban our crate `{package}`, or `{entry} {CRATES_IO}` if you mean the crates.io package: xtask/src/checks/crates_table.rs rejects a name within one edit of a row name (ignoring case, reading `-` as `_`, a swap of two letters counting as one edit)",
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

/// The entry `entry`, which starts with `clauseo-` in any case (`folded` is it in lowercase with
/// `-` read as `_`): our crate when it is exactly the package of a row, else rejected.
fn clauseo_entry(entry: &str, folded: &str, rows: &[&str]) -> Result<Never, Rejected> {
    let rest = folded.strip_prefix("clauseo_").unwrap_or(folded);
    let own = rows
        .iter()
        .find(|row| package_of(row) != **row && fold(&package_of(row)) == folded);
    match own {
        Some(row) if entry.starts_with("clauseo-") && entry == entry.to_ascii_lowercase() => {
            Ok(Never::Workspace((*row).to_owned()))
        }
        Some(row) => Err(Rejected {
            what: format!(
                "is the package `{package}` written with other letter case",
                package = package_of(row)
            ),
            fix: format!(
                "write `{package}` (or the row name `{row}`) to ban our crate, or `{entry} {CRATES_IO}` if you mean the crates.io package",
                package = package_of(row)
            ),
        }),
        None => match rows.iter().find(|row| **row == rest) {
            Some(row) => Err(Rejected {
                what: format!(
                    "is not our package: the row `{row}`'s package is `{package}`",
                    package = package_of(row)
                ),
                fix: format!(
                    "write `{row}` to ban our crate `{package}`, or `{entry} {CRATES_IO}` if you mean the crates.io package",
                    package = package_of(row)
                ),
            }),
            None => Err(Rejected {
                what: format!(
                    "starts with `clauseo-`, but `{rest}` is no row of the Crates table, so it names none of our crates"
                ),
                fix: format!(
                    "write the row name of the crate you mean ({}), add a row `{rest}` for the crate, or write `{entry} {CRATES_IO}` if you mean the crates.io package",
                    near_rows_hint(rest, rows)
                ),
            }),
        },
    }
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

/// True when `a` and `b` are equal or differ by one inserted, deleted or replaced character,
/// or by a swap of two adjacent characters.
pub(super) fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if long.len() - short.len() > 1 {
        return false;
    }
    let same_start = short.iter().zip(&long).take_while(|(x, y)| x == y).count();
    let skip = usize::from(short.len() == long.len());
    let swapped = short.len() == long.len()
        && short.get(same_start) == long.get(same_start + 1)
        && short.get(same_start + 1) == long.get(same_start)
        && short.get(same_start + 2..) == long.get(same_start + 2..);
    swapped || short.get(same_start + skip..) == long.get(same_start + 1..)
}
