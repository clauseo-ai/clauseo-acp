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
    // nc prints nothing when a Unix connect fails, so the check is its exit, against the same
    // connect outside the jail succeeding.
    let refused = nc(&elsewhere);
    assert_eq!(refused.code, Some(1), "{refused:#?}");
    run_outside("/usr/bin/nc", &["-U", elsewhere.to_str().unwrap()], &m1).assert_ok();
}
