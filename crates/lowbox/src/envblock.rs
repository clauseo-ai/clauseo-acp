//! The child's environment block: exactly the variables the command sets, and nothing else. No
//! variable comes from the guard or from the token's profile: an empty set is an empty block.
//! Windows names are compared without letter case, and the block is sorted that way, as
//! `CreateProcess` expects. Plain Rust over UTF-16.

use crate::error::Error;

/// `=`.
const EQUALS: u16 = b'=' as u16;

/// The block of `vars` (name, value), in order, a later name replacing an earlier one in any
/// letter case: sorted by name without letter case, each entry ending in a NUL and the block in
/// a second one. No variables give two NULs.
pub(crate) fn block(vars: &[(Vec<u16>, Vec<u16>)]) -> Result<Vec<u16>, Error> {
    let mut entries: Vec<(&[u16], &[u16])> = Vec::with_capacity(vars.len());
    for (name, value) in vars {
        check_name(name)?;
        if value.contains(&0) {
            return Err(Error::Environment {
                reason: "a value holds a NUL",
            });
        }
        entries.retain(|(have, _)| !same_name(have, name));
        entries.push((name, value));
    }
    entries.sort_by(|a, b| fold(a.0).cmp(fold(b.0)));
    let mut out: Vec<u16> = Vec::new();
    for (name, value) in &entries {
        out.extend_from_slice(name);
        out.push(EQUALS);
        out.extend_from_slice(value);
        out.push(0);
    }
    if entries.is_empty() {
        out.push(0);
    }
    out.push(0);
    Ok(out)
}

/// A name may not be empty, hold `=` after its first unit (`=C:` is a name), or hold a NUL.
fn check_name(name: &[u16]) -> Result<(), Error> {
    if name.is_empty() || name.contains(&0) || name.iter().skip(1).any(|u| *u == EQUALS) {
        return Err(Error::Environment {
            reason: "a name is empty or holds = or a NUL",
        });
    }
    Ok(())
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

    fn var(name: &str, value: &str) -> (Vec<u16>, Vec<u16>) {
        (w(name), w(value))
    }

    fn text(block: &[u16]) -> String {
        String::from_utf16(block).unwrap().replace('\0', "|")
    }

    #[test]
    fn no_variables_is_an_empty_block() {
        assert_eq!(block(&[]).unwrap(), [0, 0]);
    }

    #[test]
    fn the_block_holds_exactly_the_variables_set_sorted_without_letter_case() {
        let vars = [
            var("windir", r"C:\W"),
            var("TEMP", "old"),
            var("=C:", r"C:\x"),
            var("temp", "new"),
            var("SystemRoot", r"C:\W"),
        ];
        assert_eq!(
            text(&block(&vars).unwrap()),
            r"=C:=C:\x|SystemRoot=C:\W|temp=new|windir=C:\W||"
        );
    }

    #[test]
    fn a_bad_name_or_value_is_refused() {
        for bad in [
            var("", "x"),
            var("A=B", "x"),
            var("A\0", "x"),
            var("A", "x\0y"),
        ] {
            assert!(block(std::slice::from_ref(&bad)).is_err(), "{bad:?}");
        }
    }
}
