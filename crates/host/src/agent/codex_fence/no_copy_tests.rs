//! Codex's data fence and the guard's no-copy list: denied to Codex's shell and patches.

use super::tests::{access, data, escalated_access, fence, fence_with, listed};

/// The guard's no-copy list sits beside its record, so the same rules deny it: no shell or
/// patch of Codex's may write it (or plant a link at its place), ordinary or escalated.
#[test]
fn the_guard_no_copy_list_is_denied_to_the_shell_and_patches() {
    let list = data()
        .join("browser")
        .join("guard")
        .join("s1")
        .join(crate::pre_write_guard::no_copy::FILE_NAME);
    for keys in [fence(), fence_with(&listed())] {
        assert_eq!(access(&keys, &list), "deny");
        assert_eq!(escalated_access(&keys, &list), "deny");
        assert_eq!(escalated_access(&keys, list.parent().unwrap()), "deny");
    }
}
