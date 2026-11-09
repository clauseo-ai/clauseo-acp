//! [`SpawnOptions::pass_fd`] and [`SpawnOptions::close_other_fds`] on Unix: the child reads its
//! fd 3 and writes its fd 4, as Chrome's `--remote-debugging-pipe` does, and gets no other
//! descriptor of this process, whichever order the two are passed in; a descriptor this process
//! left open across exec reaches a child only without `close_other_fds`.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::path::PathBuf;

use async_io::block_on;
use clauseo_process::{LaunchSpec, SpawnOptions, spawn_in_tree};

/// Reads one line from fd 3, writes it back on fd 4 with the child's open descriptors.
const ECHO: &str =
    r#"read -r line <&3; printf 'got=%s fds=%s\n' "$line" "$(ls /dev/fd | tr '\n' ' ')" >&4"#;

/// Starts the echo with the two pipes' child ends at 3 and 4, passed in either order, and
/// checks what came back on 4.
#[test]
fn the_child_reads_fd_3_and_writes_fd_4() {
    for swap in [false, true] {
        let (to_child_r, mut to_child_w) = std::io::pipe().unwrap();
        let (mut from_child_r, from_child_w) = std::io::pipe().unwrap();
        let (fd3, fd4) = (OwnedFd::from(to_child_r), OwnedFd::from(from_child_w));
        // The sources' own numbers, to see that none reaches the child as itself.
        let sources = format!("{} {}", fd3.as_raw_fd(), fd4.as_raw_fd());
        let opts = if swap {
            SpawnOptions::new().pass_fd(4, fd4).pass_fd(3, fd3)
        } else {
            SpawnOptions::new().pass_fd(3, fd3).pass_fd(4, fd4)
        };
        let spec = LaunchSpec::new(
            PathBuf::from("/bin/sh"),
            vec!["-c".into(), ECHO.into()],
            BTreeMap::from([("SOURCES".into(), sources.into())]),
            PathBuf::from("/"),
        )
        .unwrap();
        let mut launched = spawn_in_tree(&spec, &opts).unwrap();
        // This process's copies of the child ends close here, so the read below ends at EOF.
        drop(opts);
        to_child_w.write_all(b"hello\n").unwrap();
        drop(to_child_w);
        let mut back = String::new();
        from_child_r.read_to_string(&mut back).unwrap();
        let report = block_on(launched.handle.exited());
        assert_eq!(report.code(), Some(0), "{back}");
        // A source numbered 3 or 4 is that target, open by design; any other is a leak.
        let open: Vec<&str> = back
            .trim_end()
            .split_once(" open=")
            .map_or("", |(_, open)| open)
            .split_whitespace()
            .filter(|n| !["3", "4"].contains(n))
            .collect();
        assert!(back.starts_with("got=hello "), "{back}");
        assert!(
            open.is_empty(),
            "the child got this process's {open:?}: {back}"
        );
    }
}

#[test]
fn a_descriptor_open_across_exec_reaches_a_child_only_without_close_other_fds() {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    let (leaked, _writer) = std::io::pipe().unwrap();
    fcntl(&leaked, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let fd = leaked.as_raw_fd();
    // Whether descriptor `fd` is open in a child started with `opts`.
    let open_in_child = |fd: i32, opts: &SpawnOptions| -> bool {
        let script = r#"if { : <&"$FD"; } 2>/dev/null; then echo open; else echo closed; fi"#;
        let spec = LaunchSpec::new(
            PathBuf::from("/bin/sh"),
            vec!["-c".into(), script.into()],
            BTreeMap::from([("FD".into(), fd.to_string().into())]),
            PathBuf::from("/"),
        )
        .unwrap();
        let mut launched = spawn_in_tree(&spec, opts).unwrap();
        let mut out = String::new();
        block_on(futures::AsyncReadExt::read_to_string(
            &mut launched.stdout,
            &mut out,
        ))
        .unwrap();
        block_on(launched.handle.exited());
        match out.trim() {
            "open" => true,
            "closed" => false,
            other => panic!("{other}"),
        }
    };
    assert!(open_in_child(fd, &SpawnOptions::new()), "the check sees it");
    assert!(!open_in_child(fd, &SpawnOptions::new().close_other_fds()));
    // A target other than the leaked number, which may itself be 3.
    let target = if fd == 3 { 4 } else { 3 };
    let (passed, _w) = std::io::pipe().unwrap();
    let opts = SpawnOptions::new()
        .close_other_fds()
        .pass_fd(target, OwnedFd::from(passed));
    assert!(open_in_child(target, &opts), "a passed descriptor stays");
    assert!(!open_in_child(fd, &opts));
}
