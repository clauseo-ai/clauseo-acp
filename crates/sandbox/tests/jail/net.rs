//! The network in the jail: TCP only to the proxy's port; Unix sockets only under
//! `<state>/browser/run`.

use std::net::TcpListener;

use crate::support::{World, launcher, run, run_outside};

#[test]
fn tcp_reaches_only_the_proxy_port() {
    let w = World::new();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = proxy.local_addr().unwrap().port();
    let other_port = other.local_addr().unwrap().port();
    let policy = w.builder().proxy_port(proxy_port).build().unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let nc = |port: u16| {
        let port = port.to_string();
        run(
            &launcher,
            "/usr/bin/nc",
            &["-v", "-z", "-G", "5", "127.0.0.1", &port],
            &m1,
            &[],
        )
    };
    nc(proxy_port).assert_ok();
    nc(other_port).assert_refused();
}

#[test]
fn no_proxy_port_means_no_network() {
    let w = World::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    run(
        &launcher,
        "/usr/bin/nc",
        &["-v", "-z", "-G", "5", "127.0.0.1", &port],
        &m1,
        &[],
    )
    .assert_refused();
}

#[test]
fn unix_sockets_reach_only_the_browser_run_folder() {
    let w = World::new();
    let allowed = w.listen_unix("data/agent-state/m1/codex/browser/run/d.sock");
    let elsewhere = w.listen_unix("outside/o.sock");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let nc = |path: &std::path::Path| {
        run(
            &launcher,
            "/usr/bin/nc",
            &["-U", path.to_str().unwrap()],
            &m1,
            &[],
        )
    };
    nc(&allowed).assert_ok();
    // nc prints nothing when a Unix connect fails, so its exit is checked against the same
    // connect outside the jail succeeding, and Perl's connect names the refusal (EPERM).
    let refused = nc(&elsewhere);
    assert_eq!(refused.code, Some(1), "{refused:#?}");
    run_outside("/usr/bin/nc", &["-U", elsewhere.to_str().unwrap()], &m1).assert_ok();
    let connect = |path: &std::path::Path| {
        run(
            &launcher,
            "/usr/bin/perl",
            &[
                "-e",
                r#"use Socket; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; connect(S, pack_sockaddr_un($ARGV[0])) or die "connect: $!\n"; print "CONNECTED\n""#,
                path.to_str().unwrap(),
            ],
            &m1,
            &[],
        )
    };
    assert_eq!(connect(&allowed).assert_ok().stdout, "CONNECTED\n");
    let out = connect(&elsewhere);
    out.assert_refused();
    assert!(
        out.stderr.contains("connect: Operation not permitted"),
        "{out:#?}"
    );
}

/// Binds 127.0.0.1:`port` in the jail with Perl and prints `BOUND`, or dies naming the error.
const BIND: &str = r#"use Socket; socket(S, PF_INET, SOCK_STREAM, 0) or die "socket: $!\n"; setsockopt(S, SOL_SOCKET, SO_REUSEADDR, 1); bind(S, pack_sockaddr_in($ARGV[0], inet_aton("127.0.0.1"))) or die "bind: $!\n"; listen(S, 1) or die "listen: $!\n"; print "BOUND\n""#;

#[test]
fn loopback_ports_bind_only_the_listed_ports() {
    let w = World::new();
    // Two free ports, released for the jailed process to take.
    let free = || {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (listed, other) = (free(), free());
    let policy = w
        .builder()
        .loopback(clauseo_sandbox::Loopback::Ports(vec![listed]))
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let bind = |port: u16| {
        let port = port.to_string();
        run(&launcher, "/usr/bin/perl", &["-e", BIND, &port], &m1, &[])
    };
    assert_eq!(bind(listed).assert_ok().stdout, "BOUND\n");
    let out = bind(other);
    out.assert_refused();
    assert!(
        out.stderr.contains("bind: Operation not permitted"),
        "{out:#?}"
    );
    // A listener on the listed port can be reached from the jail; one on the other cannot.
    let open = TcpListener::bind(("127.0.0.1", listed)).unwrap();
    let nc = |port: u16| {
        let port = port.to_string();
        run(
            &launcher,
            "/usr/bin/nc",
            &["-v", "-z", "-G", "5", "127.0.0.1", &port],
            &m1,
            &[],
        )
    };
    nc(listed).assert_ok();
    drop(open);
    let _closed = TcpListener::bind(("127.0.0.1", other)).unwrap();
    nc(other).assert_refused();
}
