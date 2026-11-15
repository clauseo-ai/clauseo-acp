//! The relay's peer check: a connection from outside agent-browser's tree is closed before a
//! byte is read; one from agent-browser itself or a child of it gets as far as the request.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::peer::{stat_parent, tcp_line_inode};
use super::tests::{relay, request};

/// What the relay answers this process's `GET /json`, all of it, until it closes.
fn reply_to_json(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // The relay may close before reading: a failed write is part of the answer.
    drop(stream.write_all(b"GET /json HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"));
    let mut reply = Vec::new();
    drop(stream.read_to_end(&mut reply));
    String::from_utf8_lossy(&reply).into_owned()
}

#[test]
fn agent_browser_itself_gets_as_far_as_the_request() {
    let (devtools, _reads, _writes) = relay();
    let (status, _) = request(&devtools, "GET /json HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 404 Not Found");
}

#[test]
fn a_process_outside_agent_browsers_tree_is_closed_unread() {
    let (devtools, _reads, _writes) = relay();
    // No such process: this test process is nobody's descendant of it.
    devtools.allow_descendants_of(4_000_000_000);
    assert_eq!(reply_to_json(devtools.port()), "", "not even a 404");
}

#[test]
fn nobody_gets_in_before_agent_browser_is_named() {
    let (browser_reads, guard_writes) = std::io::pipe().unwrap();
    let (guard_reads, browser_writes) = std::io::pipe().unwrap();
    let devtools = super::Devtools::start(Box::new(guard_writes), Box::new(guard_reads)).unwrap();
    assert_eq!(reply_to_json(devtools.port()), "");
    drop((browser_reads, browser_writes));
}

/// A real child of agent-browser's stand-in (this process) connects, and a sibling tree does
/// not: `bash`'s `/dev/tcp` makes the connection from the child.
#[cfg(unix)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test's own child, a client of the relay from another process"
)]
fn a_child_of_agent_browser_is_let_in_and_another_process_is_not() {
    let (devtools, _reads, _writes) = relay();
    let script = format!(
        "exec 3<>/dev/tcp/127.0.0.1/{}; printf 'GET /json HTTP/1.1\\r\\n\\r\\n' >&3; \
         head -c 22 <&3",
        devtools.port()
    );
    let child = std::process::Command::new("bash")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&child.stdout),
        "HTTP/1.1 404 Not Found"
    );
    // Now agent-browser is a process this one did not start: this one's child is outside it.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    devtools.allow_descendants_of(other.id());
    let child = std::process::Command::new("bash")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&child.stdout), "", "closed unread");
    drop(other.kill());
    drop(other.wait());
}

#[test]
fn proc_net_tcp_lines_name_the_client_socket() {
    let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  \
                  timeout inode";
    // 127.0.0.1:50000 -> 127.0.0.1:61000, inode 98765 (little-endian hosts print 0100007F).
    let local = format!("{:08X}", u32::from_ne_bytes([127, 0, 0, 1]));
    let line = format!(
        "   3: {local}:C350 {local}:EE48 01 00000000:00000000 00:00000000 00000000  1000        0 \
         98765 1 0000000000000000 20 4 30 10 -1"
    );
    assert_eq!(tcp_line_inode(&line, 50_000, 61_000), Some(98_765));
    assert_eq!(
        tcp_line_inode(&line, 61_000, 50_000),
        None,
        "the relay's own side"
    );
    assert_eq!(tcp_line_inode(&line, 50_001, 61_000), None);
    assert_eq!(tcp_line_inode(header, 50_000, 61_000), None);
    let elsewhere = line.replacen(&local, "0101A8C0", 1);
    assert_eq!(
        tcp_line_inode(&elsewhere, 50_000, 61_000),
        None,
        "not loopback"
    );
}

#[test]
fn proc_stat_lines_name_the_parent_even_with_odd_names() {
    assert_eq!(
        stat_parent("4242 (agent-browser) S 4100 4242 4100 0 -1"),
        Some(4100)
    );
    assert_eq!(stat_parent("77 (a) b (c) R 12 77 77 0 -1"), Some(12));
    assert_eq!(stat_parent("garbage"), None);
}
