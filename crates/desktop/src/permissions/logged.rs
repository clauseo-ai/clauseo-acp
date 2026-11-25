//! What a lifecycle line said last, so a line is written only when it would say something new:
//! the app lists the permissions at every window focus and visibility change, and host.log
//! should show the changes, not the focus count.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

/// The last value logged for each key.
pub(super) struct LastLogged<K, V>(Mutex<Option<HashMap<K, V>>>);

impl<K: Eq + Hash, V: PartialEq> LastLogged<K, V> {
    /// Nothing logged yet.
    pub(super) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Whether `value` differs from the last one kept for `key` (or none was kept); keeps it.
    pub(super) fn changed(&self, key: K, value: V) -> bool {
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let map = last.get_or_insert_with(HashMap::new);
        if map.get(&key) == Some(&value) {
            return false;
        }
        map.insert(key, value);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::LastLogged;

    #[test]
    fn a_line_is_written_only_when_it_says_something_new() {
        let last: LastLogged<&str, i32> = LastLogged::new();
        assert!(last.changed("row", 1), "the first is new");
        assert!(!last.changed("row", 1), "the same again is not");
        assert!(last.changed("row", 2), "a change is");
        assert!(last.changed("row", 1), "and so is the change back");
        assert!(last.changed("other", 1), "each key keeps its own");
        assert!(!last.changed("other", 1));
    }
}
