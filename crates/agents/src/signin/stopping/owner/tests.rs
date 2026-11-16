//! The owner rule, the table parsers, and on Linux the real `/proc` tables.

use std::collections::BTreeSet;

use super::owned;

#[test]
fn owned_needs_a_listener_and_none_outside_the_tree() {
    let set = |pids: &[u32]| pids.iter().copied().collect::<BTreeSet<u32>>();
    assert!(owned(&set(&[10]), &[10, 11]));
    assert!(owned(&set(&[10, 11]), &[10, 11]));
    assert!(!owned(&set(&[]), &[10]), "no listener");
    assert!(
        !owned(&set(&[10, 99]), &[10]),
        "another process listens too"
    );
    assert!(
        !owned(&set(&[0]), &[10]),
        "a listener nobody readable holds"
    );
}

#[cfg(unix)]
#[test]
fn proc_tables_give_the_listen_rows_on_the_port() {
    // Two rows on port 1455 (0x05AF): one LISTEN, one ESTABLISHED; one LISTEN on another port.
    let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:05AF 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1 0 100 0 0 10 0
   1: 0100007F:05AF 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 4343 1 0 20 4 30 10 -1
   2: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4444 1 0 100 0 0 10 0
";
    assert_eq!(super::proc_net::parse_listen(text, 1455), [4242]);
    assert_eq!(super::proc_net::parse_listen(text, 8080), [4444]);
    assert_eq!(super::proc_net::parse_listen(text, 1), Vec::<u64>::new());
}

#[cfg(unix)]
#[test]
fn lsof_lines_give_pids() {
    let text = "p123\nf7\np456\nnoise\n";
    let pids: Vec<u32> = super::lsof::parse_pids(text).into_iter().collect();
    assert_eq!(pids, [123, 456]);
}

/// This test's own listener is found, owned by this process and by no other pid.
#[cfg(target_os = "linux")]
#[test]
fn this_processes_listener_is_its_own() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let inodes = super::proc_net::listen_inodes(port).unwrap();
    assert_eq!(inodes.len(), 1);
    let owners = super::proc_net::owners(&inodes);
    let me = std::process::id();
    assert!(owned(&owners, &[me]), "{owners:?}");
    assert!(!owned(&owners, &[1]), "{owners:?}");
    drop(listener);
}
