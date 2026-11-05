//! A `sandbox` that is itself a link, with no `--sandbox-removed` (the VM's row 6): unlinked by
//! handle and reported, its target untouched, nothing kept; and the decision the handle makes. On
//! Windows the link here is a junction (`make.rs`); `windows.rs` adds a directory symbolic link
//! and a reparse point that is no link.

use super::{World, exists, make};
use crate::delete_app_data::ops::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK, LinkVerdict, NAME_SURROGATE_BIT,
    link_verdict, unlink_name_surrogate,
};
use crate::delete_app_data::{FAILED, KEPT_SANDBOX};

#[test]
fn the_handle_unlinks_only_a_junction_or_a_symbolic_link() {
    // Both are name surrogates (`IsReparseTagNameSurrogate`): deleting one removes a name only.
    for tag in [IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK] {
        assert_ne!(tag & NAME_SURROGATE_BIT, 0, "{tag:#x}");
        assert_eq!(link_verdict(true, tag), LinkVerdict::Unlinked, "{tag:#x}");
    }
    for tag in [
        0x9000_001A, // IO_REPARSE_TAG_CLOUD (OneDrive's placeholders)
        0x9000_601A, // IO_REPARSE_TAG_CLOUD_6
        0x8000_0013, // IO_REPARSE_TAG_DEDUP
        0x8000_001B, // IO_REPARSE_TAG_APPEXECLINK
        0x8000_0017, // IO_REPARSE_TAG_WOF
        0xA000_001D, // IO_REPARSE_TAG_LX_SYMLINK: a name surrogate, but no junction or link
        0xA000_0027, // IO_REPARSE_TAG_WCI_LINK_1: the same
        0x0000_7E57, // anyone's own tag
        0,
    ] {
        assert_eq!(link_verdict(true, tag), LinkVerdict::Kept(tag), "{tag:#x}");
    }
    // No reparse point on the handle (a real folder swapped in): never deleted, whatever tag.
    for tag in [0, IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK] {
        assert_eq!(link_verdict(false, tag), LinkVerdict::NotALink, "{tag:#x}");
    }
}

#[test]
fn a_real_folder_given_to_the_handle_unlink_is_left_whole() {
    // What a swap between the listing and the open leaves: a real folder, never deleted here.
    let world = World::new("dad-unlink-real");
    let sandbox = world.data_dir("sandbox");
    world.data_file("sandbox/acl-ledger.json");
    assert_eq!(
        unlink_name_surrogate(&sandbox).unwrap(),
        LinkVerdict::NotALink
    );
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
}

#[test]
fn a_linked_sandbox_without_sandbox_removed_is_unlinked_and_reported() {
    let world = World::new("dad-sandbox-link-kept");
    world.scratch.dir("outside/canary/set-aside");
    world
        .scratch
        .file("outside/canary/acl-ledger.json", b"not ours");
    world.data_file("store.sqlite");
    make::folder_link(&world.canary(), &world.data("sandbox"));
    let (code, stderr) = world.delete(false);
    // A failure, not a keep: nothing behind the link is S15's, so nothing is named as kept.
    assert_eq!(code, FAILED, "{stderr}");
    assert!(stderr.contains("removed as a link"), "{stderr}");
    let canary = world.canary();
    assert!(
        !stderr.contains(canary.to_str().unwrap()),
        "the line names the link's target: {stderr}"
    );
    world.assert_both_gone();
    world.assert_canary_whole();
    assert!(exists(&world.at("outside/canary/acl-ledger.json")));
    assert!(exists(&world.at("outside/canary/set-aside")));
}

#[test]
fn a_real_sandbox_is_still_kept_whole() {
    // The link case changed nothing for the folder S15 made.
    let world = World::new("dad-sandbox-real-kept");
    world.data_dir("sandbox");
    world.data_file("sandbox/acl-ledger.json");
    assert_eq!(world.delete(false), (KEPT_SANDBOX, String::new()));
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
}
