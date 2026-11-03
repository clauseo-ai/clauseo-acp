//! The network in the jail: TCP only to the proxy's port; Unix sockets only in the browser
//! socket folder the host made, and none without one.

use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use clauseo_sandbox::BrowserSocketDir;

use crate::support::{World, dir_at, launcher, listen_unix_at, run, run_outside, symlink_at};

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

/// Binds a Unix socket at `$ARGV[0]` in the jail with Perl and prints `BOUND`, or dies naming
/// the error.
const BIND_UNIX: &str = r#"use Socket; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; bind(S, pack_sockaddr_un($ARGV[0])) or die "bind: $!\n"; print "BOUND\n""#;

/// In the folder `$ARGV[0]`: binds a socket, connects to it, passes a byte, removes it, then
/// writes, reads and removes a pid file beside it; prints `OK`, or dies naming the step.
const SOCKET_LIFE: &str = r#"use Socket; my $d = $ARGV[0]; my $p = "$d/d.sock"; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; bind(S, pack_sockaddr_un($p)) or die "bind: $!\n"; listen(S, 1) or die "listen: $!\n"; socket(C, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; connect(C, pack_sockaddr_un($p)) or die "connect: $!\n"; accept(A, S) or die "accept: $!\n"; syswrite(C, "x", 1) == 1 or die "write: $!\n"; sysread(A, my $b, 1) == 1 or die "read: $!\n"; unlink($p) or die "unlink: $!\n"; open(F, ">", "$d/d.pid") or die "open: $!\n"; print F "1\n"; close(F) or die "close: $!\n"; open(G, "<", "$d/d.pid") or die "reopen: $!\n"; my $l = <G>; $l eq "1\n" or die "pid: $l\n"; unlink("$d/d.pid") or die "unlink pid: $!\n"; print "OK\n""#;

/// Does `$ARGV[1]` to the folder `$ARGV[0]`: `rmdir`, `mkdir`, `rename` (to `<folder>.moved`) or
/// `chmod` (to 0777); prints `DONE`, or dies naming the step and the error.
const FOLDER: &str = r#"my ($d, $op) = @ARGV; if ($op eq "rmdir") { rmdir($d) or die "rmdir: $!\n" } elsif ($op eq "mkdir") { mkdir($d) or die "mkdir: $!\n" } elsif ($op eq "rename") { rename($d, "$d.moved") or die "rename: $!\n" } else { chmod(0777, $d) or die "chmod: $!\n" } print "DONE\n""#;

/// Creates the regular file `$ARGV[0]`; prints `DONE`, or dies naming the error.
const TOUCH: &str = r#"open(F, ">", $ARGV[0]) or die "open: $!\n"; print "DONE\n""#;

/// Connects to the Unix socket `$ARGV[0]` and prints `CONNECTED`, or dies naming the error.
const CONNECT_UNIX: &str = r#"use Socket; socket(S, PF_UNIX, SOCK_STREAM, 0) or die "socket: $!\n"; connect(S, pack_sockaddr_un($ARGV[0])) or die "connect: $!\n"; print "CONNECTED\n""#;

#[test]
fn unix_sockets_live_only_in_the_browser_socket_folder() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let other = BrowserSocketDir::create().unwrap();
    let old_run = w.dir("data/agent-state/m1/codex/browser/run");
    let policy = w.builder().browser_socket_dir(&dir).build().unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let perl = |script: &str, args: &[&str]| {
        let mut all = vec!["-e", script];
        all.extend_from_slice(args);
        run(&launcher, "/usr/bin/perl", &all, &m1, &[])
    };
    let dir_text = dir.path().to_str().unwrap();
    assert_eq!(perl(SOCKET_LIFE, &[dir_text]).assert_ok().stdout, "OK\n");
    let refused_bind = |path: &Path| {
        let out = perl(BIND_UNIX, &[path.to_str().unwrap()]);
        out.assert_refused();
        assert!(
            out.stderr.contains("bind: Operation not permitted"),
            "{}: {out:#?}",
            path.display()
        );
        assert!(
            std::fs::symlink_metadata(path).is_err(),
            "{}",
            path.display()
        );
    };
    let stray = format!("clauseo-jail-{}-{}", std::process::id(), dir.id());
    refused_bind(&Path::new("/tmp").join(format!("{stray}.sock")));
    refused_bind(&other.path().join("d.sock"));
    refused_bind(&old_run.join("d.sock"));
    // The folder itself stays as the host made it: not removed, renamed or chmodded.
    for op in ["rmdir", "rename", "chmod"] {
        let out = perl(FOLDER, &[dir_text, op]);
        out.assert_refused();
        assert!(
            out.stderr
                .contains(&format!("{op}: Operation not permitted")),
            "{op}: {out:#?}"
        );
    }
    let meta = std::fs::symlink_metadata(dir.path()).unwrap();
    assert!(meta.is_dir());
    assert_eq!(meta.permissions().mode() & 0o7777, 0o700);
    // Nothing is made in /tmp outside it: no folder, no file.
    let new = Path::new("/tmp").join(&stray);
    let out = perl(FOLDER, &[new.to_str().unwrap(), "mkdir"]);
    out.assert_refused();
    assert!(
        out.stderr.contains("mkdir: Operation not permitted"),
        "{out:#?}"
    );
    assert!(std::fs::symlink_metadata(&new).is_err());
    let file = Path::new("/tmp").join(format!("{stray}.txt"));
    let out = perl(TOUCH, &[file.to_str().unwrap()]);
    out.assert_refused();
    assert!(
        out.stderr.contains("open: Operation not permitted"),
        "{out:#?}"
    );
    assert!(std::fs::symlink_metadata(&file).is_err());
}

#[test]
fn only_regular_files_and_sockets_are_made_in_the_browser_socket_folder() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let (launcher, _) = launcher(w.builder().browser_socket_dir(&dir).build().unwrap());
    let m1 = w.at("m1");
    let at = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
    for (program, args) in [
        ("/bin/mkdir", vec![at("sub")]),
        (
            "/bin/ln",
            vec!["-s".to_owned(), "/etc".to_owned(), at("link")],
        ),
        ("/usr/bin/mkfifo", vec![at("pipe")]),
    ] {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run(&launcher, program, &args, &m1, &[]);
        out.assert_refused();
        assert!(
            out.stderr.contains("Operation not permitted"),
            "{program}: {out:#?}"
        );
    }
    for name in ["sub", "link", "pipe"] {
        assert!(
            std::fs::symlink_metadata(dir.path().join(name)).is_err(),
            "{name}"
        );
    }
    let file = at("d.pid");
    let made = run(&launcher, "/usr/bin/perl", &["-e", TOUCH, &file], &m1, &[]);
    assert_eq!(made.assert_ok().stdout, "DONE\n");
    let socket = at("d.sock");
    let bound = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", BIND_UNIX, &socket],
        &m1,
        &[],
    );
    assert_eq!(bound.assert_ok().stdout, "BOUND\n");
}

/// Swaps `$ARGV[0]` and `$ARGV[1]` with `renameatx_np(AT_FDCWD, .., AT_FDCWD, .., RENAME_SWAP)`
/// (syscall 488, `AT_FDCWD` -2, `RENAME_SWAP` 2); prints `SWAPPED`, or dies naming the error.
const SWAP: &str =
    r#"syscall(488, -2, $ARGV[0], -2, $ARGV[1], 2) == 0 or die "swap: $!\n"; print "SWAPPED\n""#;

#[test]
fn a_folder_swapped_in_from_the_matter_goes_at_drop_without_following_its_links() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let path = dir.path().to_owned();
    let secret = w.at("outside/secret.txt");
    w.dir("m1/swapped");
    symlink_at(&secret, &w.at("m1/swapped/out.txt"));
    let (launcher, _) = launcher(w.builder().browser_socket_dir(&dir).build().unwrap());
    let m1 = w.at("m1");
    let target = path.join("g");
    let target_text = target.to_str().unwrap();
    let made = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", TOUCH, target_text],
        &m1,
        &[],
    );
    assert_eq!(made.assert_ok().stdout, "DONE\n");
    let from = w.at("m1/swapped");
    let swapped = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", SWAP, from.to_str().unwrap(), target_text],
        &m1,
        &[],
    );
    // macOS versions differ here: some jails allow the swap, stricter ones refuse it.
    let held = if swapped.code == Some(0) {
        assert_eq!(swapped.stdout, "SWAPPED\n");
        // The matter's folder, with its link out, now sits in the socket folder.
        target.clone()
    } else {
        swapped.assert_refused();
        assert!(
            swapped.stderr.contains("swap: Operation not permitted"),
            "{swapped:#?}"
        );
        // The same structure, planted by the host, so the drop half still runs.
        let planted = dir_at(&path.join("planted"));
        symlink_at(&secret, &planted.join("out.txt"));
        planted
    };
    let link = held.join("out.txt");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    drop(launcher);
    drop(dir);
    assert!(
        std::fs::symlink_metadata(&path).is_err(),
        "{}",
        path.display()
    );
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "outside secret\n"
    );
}

#[test]
fn a_link_in_the_folder_does_not_reach_a_socket_outside_it() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let outside = w.listen_unix("outside/o.sock");
    let link = symlink_at(&outside, &dir.path().join("l.sock"));
    let (launcher, _) = launcher(w.builder().browser_socket_dir(&dir).build().unwrap());
    let m1 = w.at("m1");
    let out = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", CONNECT_UNIX, link.to_str().unwrap()],
        &m1,
        &[],
    );
    out.assert_refused();
    assert!(
        out.stderr.contains("connect: Operation not permitted"),
        "{out:#?}"
    );
    // The same connect through the link outside the jail succeeds.
    run_outside("/usr/bin/nc", &["-U", link.to_str().unwrap()], &m1).assert_ok();
}

#[test]
fn without_a_browser_socket_folder_no_unix_socket_binds_or_connects() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let old_run = w.listen_unix("data/agent-state/m1/codex/browser/run/d.sock");
    let elsewhere = w.listen_unix("outside/o.sock");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let bind = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", BIND_UNIX, dir.path().join("d.sock").to_str().unwrap()],
        &m1,
        &[],
    );
    bind.assert_refused();
    let connect = |path: &Path| {
        run(
            &launcher,
            "/usr/bin/perl",
            &["-e", CONNECT_UNIX, path.to_str().unwrap()],
            &m1,
            &[],
        )
    };
    for path in [&old_run, &elsewhere] {
        let out = connect(path);
        out.assert_refused();
        assert!(
            out.stderr.contains("connect: Operation not permitted"),
            "{out:#?}"
        );
        // The same connect outside the jail succeeds.
        run_outside("/usr/bin/nc", &["-U", path.to_str().unwrap()], &m1).assert_ok();
    }
}

#[test]
fn a_browser_socket_folder_listener_is_reached_from_the_jail() {
    let w = World::new();
    let dir = BrowserSocketDir::create().unwrap();
    let other = BrowserSocketDir::create().unwrap();
    let inside = listen_unix_at(dir.path().join("d.sock"));
    let beside = listen_unix_at(other.path().join("d.sock"));
    let (launcher, _) = launcher(w.builder().browser_socket_dir(&dir).build().unwrap());
    let m1 = w.at("m1");
    let nc = |path: &Path| {
        run(
            &launcher,
            "/usr/bin/nc",
            &["-U", path.to_str().unwrap()],
            &m1,
            &[],
        )
    };
    nc(&inside).assert_ok();
    let refused = run(
        &launcher,
        "/usr/bin/perl",
        &["-e", CONNECT_UNIX, beside.to_str().unwrap()],
        &m1,
        &[],
    );
    refused.assert_refused();
    assert!(
        refused.stderr.contains("connect: Operation not permitted"),
        "{refused:#?}"
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
