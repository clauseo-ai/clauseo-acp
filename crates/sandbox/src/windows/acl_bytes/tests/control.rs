//! The control the sandbox folder's protected DACL is written with (`private_dir`), which
//! `SetKernelObjectSecurity` keeps exactly as given.

use super::*;

/// Protected, auto-inherited and asking to stay so, by their documented values; nothing
/// `SetSecurityDescriptorControl` refuses to set (only `0x0100` to `0x2000` may be set).
#[test]
fn a_protected_dacl_is_written_protected_and_auto_inherited() {
    assert_eq!(PROTECTED_DACL_CONTROL, 0x1000 | 0x0400 | 0x0100);
    assert_eq!(PROTECTED_DACL_CONTROL & !0x3F00, 0);
}

/// A descriptor with that control reads back protected and auto-inherited.
#[test]
fn a_descriptor_with_that_control_reads_back_so() {
    let dacl = render(2, &[&spec(POOL, Rights::Modify).bytes()]);
    let bytes = build_descriptor(PROTECTED_DACL_CONTROL, Some(&dacl), None);
    let read = Descriptor::parse(&bytes).unwrap();
    assert!(read.dacl_protected());
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);
}
