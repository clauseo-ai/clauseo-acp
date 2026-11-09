//! The child's environment block: the token's own environment (`CreateEnvironmentBlock`), or
//! none, with the command's changes on top. Windows names are compared without letter case, and
//! the block is sorted that way, as `CreateProcess` expects. Plain Rust over UTF-16.

use crate::error::Error;

/// `=`.
const EQUALS: u16 = b'=' as u16;

/// One change to the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    /// Set `name` to `value`.
    Set(Vec<u16>, Vec<u16>),
    /// Remove `name`.
    Remove(Vec<u16>),
}

/// The `name=value` entries of a NUL-separated block ending in two NULs. Reading stops at the
/// first empty entry.
pub(crate) fn entries(block: &[u16]) -> Vec<Vec<u16>> {
    block
        .split(|u| *u == 0)
        .take_while(|entry| !entry.is_empty())
        .map(<[u16]>::to_vec)
        .collect()
}

/// The block from `base` entries with `changes` applied in order: sorted by name without letter
/// case, each entry ending in a NUL and the block in a second one. An empty environment is two
/// NULs.
pub(crate) fn block(base: Vec<Vec<u16>>, changes: &[Change]) -> Result<Vec<u16>, Error> {
    let mut entries = base;
    for change in changes {
        let name = match change {
            Change::Set(name, _) | Change::Remove(name) => name,
        };
        check_name(name)?;
        entries.retain(|entry| !same_name(name_of(entry), name));
        if let Change::Set(name, value) = change {
            if value.contains(&0) {
                return Err(Error::Environment {
                    reason: "a value holds a NUL",
                });
            }
            let mut entry = name.clone();
            entry.push(EQUALS);
            entry.extend_from_slice(value);
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| fold(name_of(a)).cmp(fold(name_of(b))));
    let mut out: Vec<u16> = Vec::new();
    for entry in &entries {
        out.extend_from_slice(entry);
        out.push(0);
    }
    if entries.is_empty() {
        out.push(0);
    }
    out.push(0);
    Ok(out)
}

/// A name may not be empty, hold `=` after its first unit, or hold a NUL.
fn check_name(name: &[u16]) -> Result<(), Error> {
    if name.is_empty() || name.contains(&0) || name.iter().skip(1).any(|u| *u == EQUALS) {
        return Err(Error::Environment {
            reason: "a name is empty or holds = or a NUL",
        });
    }
    Ok(())
}

/// The name part of `name=value`. A name may start with `=` (`=C:=C:\dir`), so the search for
/// the separator starts at the second unit.
fn name_of(entry: &[u16]) -> &[u16] {
    let end = entry
        .iter()
        .skip(1)
        .position(|u| *u == EQUALS)
        .map_or(entry.len(), |i| i + 1);
    &entry[..end]
}

/// Whether two names are the same without letter case.
fn same_name(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && fold(a).eq(fold(b))
}

/// The name with ASCII letters uppercased, for comparing.
fn fold(name: &[u16]) -> impl Iterator<Item = u16> + '_ {
    name.iter().map(|u| {
        if (u16::from(b'a')..=u16::from(b'z')).contains(u) {
            u - 32
        } else {
            *u
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn text(block: &[u16]) -> String {
        String::from_utf16(block).unwrap().replace('\0', "|")
    }

    #[test]
    fn a_block_reads_back_into_entries() {
        let block = w("=C:=C:\\x\0Path=a;b\0TEMP=t\0\0junk");
        assert_eq!(
            entries(&block),
            [w("=C:=C:\\x"), w("Path=a;b"), w("TEMP=t")]
        );
    }

    #[test]
    fn changes_replace_without_letter_case_and_sort() {
        let base = vec![
            w("Path=a"),
            w("TEMP=old"),
            w("=C:=C:\\x"),
            w("windir=C:\\W"),
        ];
        let changes = [
            Change::Set(w("temp"), w("new")),
            Change::Set(w("TMP"), w("new")),
            Change::Remove(w("PATH")),
        ];
        assert_eq!(
            text(&block(base, &changes).unwrap()),
            "=C:=C:\\x|temp=new|TMP=new|windir=C:\\W||"
        );
    }

    #[test]
    fn an_empty_environment_is_two_nuls() {
        assert_eq!(block(Vec::new(), &[]).unwrap(), [0, 0]);
    }

    #[test]
    fn a_bad_name_or_value_is_refused() {
        for change in [
            Change::Set(w(""), w("x")),
            Change::Set(w("A=B"), w("x")),
            Change::Set(w("A\0"), w("x")),
            Change::Set(w("A"), w("x\0y")),
            Change::Remove(w("")),
        ] {
            assert!(
                block(Vec::new(), std::slice::from_ref(&change)).is_err(),
                "{change:?}"
            );
        }
    }
}
