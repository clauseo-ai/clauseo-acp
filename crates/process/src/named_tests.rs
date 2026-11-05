//! [`crate::processes_named`] on the OS that has it, as any user runs it (CI's runners and a
//! lawyer's Mac are not root): a process that is not the one asked for, root's daemons
//! included, never makes the answer unknown.

/// This test process is found by its executable's file name, with a start; a name no process
/// has gives an empty list, not `None`: on macOS `PROC_PIDTBSDINFO` answers EPERM for root's
/// and other users' processes, so a listing that read every process that way could never say
/// "none runs".
#[test]
fn this_process_is_found_by_its_name_and_a_name_none_has_lists_none() {
    let exe = std::env::current_exe().unwrap();
    let name = exe.file_name().unwrap().to_str().unwrap().to_owned();
    let found = crate::processes_named(&name).expect("the listing is readable");
    assert!(!found.is_empty(), "{name} not found");
    assert_eq!(
        crate::processes_named("clauseo-no-such-process"),
        Some(Vec::new())
    );
}
