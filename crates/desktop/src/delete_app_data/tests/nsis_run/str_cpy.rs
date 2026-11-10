//! `StrCpy` with a length and an offset, for the 4- and 5-argument forms `Machine::line` runs.

use super::Machine;

/// `StrCpy` with a length and an offset, as NSIS's `EW_ASSIGNVAR` (exec.c) takes them: a
/// negative offset counts from the end (nothing when still negative), the offset is capped at
/// the end, then an empty length takes the rest, a negative one cuts that many characters off
/// the end, and 0 takes nothing.
pub(super) fn str_cpy(machine: &Machine, value: &str, len: &str, offset: &str) -> String {
    let text: Vec<char> = machine.expand(value).chars().collect();
    let total = i64::try_from(text.len()).unwrap();
    let offset = machine.expand(offset);
    let mut start = if offset.is_empty() {
        0
    } else {
        offset.parse::<i64>().unwrap()
    };
    if start < 0 {
        start += total;
    }
    if start < 0 {
        return String::new();
    }
    let rest = &text[usize::try_from(start.min(total)).unwrap()..];
    let left = i64::try_from(rest.len()).unwrap();
    let len = machine.expand(len);
    let take = if len.is_empty() {
        left
    } else {
        let len = len.parse::<i64>().unwrap();
        if len < 0 {
            (left + len).max(0)
        } else {
            len.min(left)
        }
    };
    rest[..usize::try_from(take).unwrap()].iter().collect()
}
