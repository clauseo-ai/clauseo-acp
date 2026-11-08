//! The task file's read (`Object::read_child_secured`) shares reads alone: a file someone holds
//! open for writing isn't read, so a write handle opened before its DACL changed can't change
//! it under the read. The plain read (`Object::read_child`) still shares writes.

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

use super::*;
use crate::windows::acl::object::Object;

/// `ERROR_SHARING_VIOLATION`.
const SHARING_VIOLATION: i32 = 32;

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test holds the file open for writing, as a pool user's earlier handle would"
)]
fn a_secured_read_refuses_a_file_held_open_for_writing() {
    let place = Place::new("acl-held");
    let file = dunce::canonicalize(place.dir.file("held.xml", "x")).unwrap();
    let folder = Object::open_for(file.parent().unwrap(), 0).unwrap();
    let name: Vec<u16> = "held.xml".encode_utf16().collect();
    let info = OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0;
    let (bytes, _) = folder.read_child_secured(&name, 16, info).unwrap().unwrap();
    assert_eq!(bytes, b"x");

    let writer = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    let error = folder.read_child_secured(&name, 16, info).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(SHARING_VIOLATION), "{error}");
    assert_eq!(folder.read_child(&name, 16).unwrap(), Some(b"x".to_vec()));
    drop(writer);
    assert!(
        folder
            .read_child_secured(&name, 16, info)
            .unwrap()
            .is_some()
    );
}
