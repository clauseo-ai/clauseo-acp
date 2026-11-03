use super::{Arch, Libc, Os, Platform};

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn npm_list_rule() {
    let mac = Platform::new(Os::Darwin, Arch::Arm64);
    assert!(mac.allows(&[], &[], &[]));
    assert!(mac.allows(&list(&["darwin"]), &list(&["arm64"]), &[]));
    assert!(!mac.allows(&list(&["darwin"]), &list(&["x64"]), &[]));
    assert!(!mac.allows(&list(&["linux", "win32"]), &[], &[]));
    assert!(!mac.allows(&list(&["!darwin"]), &[], &[]));
    assert!(mac.allows(&list(&["!win32"]), &[], &[]));
    // libc is Linux's alone: a glibc-only package still passes the macOS check of libc.
    assert!(mac.allows(&[], &[], &list(&["glibc"])));
}

#[test]
fn linux_libc_is_checked() {
    let glibc = Platform::new(Os::Linux, Arch::X64);
    let musl = Platform {
        libc: Some(Libc::Musl),
        ..glibc
    };
    assert!(glibc.allows(&list(&["linux"]), &list(&["x64"]), &list(&["glibc"])));
    assert!(!glibc.allows(&list(&["linux"]), &list(&["x64"]), &list(&["musl"])));
    assert!(musl.allows(&list(&["linux"]), &list(&["x64"]), &list(&["musl"])));
}

#[test]
fn names() {
    let win = Platform::new(Os::Win32, Arch::X64);
    assert_eq!(win.node_name(), "win-x64");
    assert_eq!(win.to_string(), "win32-x64");
    let linux = Platform::new(Os::Linux, Arch::Arm64);
    assert_eq!(linux.node_name(), "linux-arm64");
    assert_eq!(linux.to_string(), "linux-arm64-glibc");
}

#[test]
fn this_machine_is_a_platform() {
    // Every CI and developer machine is one of the six.
    let current = Platform::current().expect("a supported platform");
    assert!(
        Platform::ALL
            .iter()
            .any(|platform| { platform.os == current.os && platform.arch == current.arch })
    );
}
