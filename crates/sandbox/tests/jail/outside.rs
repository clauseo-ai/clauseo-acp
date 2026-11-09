//! What lies outside the jail besides files, tried as ordinary actions: another process's
//! arguments and path, the network past the proxy (IPv6, UDP, a listener on every address), the
//! lawyer's SSH agent, Apple Events, and the login Keychain under the Claude profile.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};
use futures::io::AsyncReadExt;

use crate::deny::{Jail, assert_refused_with, unique};
use crate::support::{World, launcher, run, run_outside, sh};

/// Network actions for `perl -e`, the first argument naming one. Prints `DONE`, or dies naming
/// the action and the error.
const NET: &str = r#"use Socket qw(:all); my $op = shift;
    my $v6 = inet_pton(AF_INET6, "2606:4700:4700::1111");
    if ($op eq "udp") { socket(S, PF_INET, SOCK_DGRAM, 0) or die "socket: $!\n";
        send(S, "x", 0, pack_sockaddr_in(53, inet_aton("1.1.1.1"))) or die "udp: $!\n"; }
    if ($op eq "tcp6") { socket(S, PF_INET6, SOCK_STREAM, 0) or die "socket: $!\n";
        connect(S, pack_sockaddr_in6(443, $v6)) or die "tcp6: $!\n"; }
    if ($op eq "udp6") { socket(S, PF_INET6, SOCK_DGRAM, 0) or die "socket: $!\n";
        send(S, "x", 0, pack_sockaddr_in6(53, $v6)) or die "udp6: $!\n"; }
    if ($op eq "bind") { socket(S, PF_INET, SOCK_STREAM, 0) or die "socket: $!\n";
        bind(S, pack_sockaddr_in(0, INADDR_ANY)) or die "bind: $!\n"; }
    if ($op eq "bind6") { socket(S, PF_INET6, SOCK_STREAM, 0) or die "socket: $!\n";
        bind(S, pack_sockaddr_in6(0, inet_pton(AF_INET6, "::"))) or die "bind6: $!\n"; }
    print "DONE\n""#;

/// Item 1: a process the user started outside the jail. `ps` is set-user-ID and the jail does
/// not run it; the arguments (`KERN_PROCARGS2`) and path (`proc_pidpath`) are refused, which
/// `(deny default)` alone did not do on macOS 27.2. Listing pids still works, so the jail finds
/// its own children, and their arguments and path read.
#[test]
fn another_process_cannot_be_inspected() {
    let j = Jail::new();
    let spec = LaunchSpec::new(
        "/bin/sleep".into(),
        vec!["60".into()],
        BTreeMap::new(),
        j.m1(),
    );
    let mut sleeper = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec.unwrap())
        .unwrap();
    let pid = sleeper.handle.pid().to_string();
    j.probe_outside(&["procargs", &pid]).assert_ok();
    j.probe_outside(&["pidpath", &pid]).assert_ok();
    let refused = "Operation not permitted";
    assert_refused_with(&j.probe(&["procargs", &pid]), refused);
    assert_refused_with(&j.probe(&["pidpath", &pid]), refused);
    // Its name, owner, parent and start time (`KERN_PROC_PID`): the base allows no
    // `kern.proc.pid.` sysctl.
    j.probe_outside(&["kinfo", &pid]).assert_ok();
    assert_refused_with(&j.probe(&["kinfo", &pid]), refused);
    let ps = run(&j.launcher, "/bin/ps", &["eww", &pid], &j.m1(), &[]);
    assert_ne!(ps.code, Some(0), "{ps:#?}");
    assert!(!ps.stdout.contains("sleep"), "{ps:#?}");
    let own = r#"/bin/sleep 5 & child=$!
        "$1" listall && "$1" children $child && "$1" procargs $child && "$1" pidpath $child &&
        "$1" kinfo $child"#;
    let out = j.sh(own, &[j.w.at("tools/probe").to_str().unwrap()]);
    for step in ["listall", "children", "procargs", "pidpath", "kinfo"] {
        assert!(out.stdout.contains(&format!("ALLOWED {step}")), "{out:#?}");
    }
    run_outside("/bin/kill", &["-TERM", &pid], &j.m1()).assert_ok();
    async_io::block_on(sleeper.handle.exited());
}

/// The process-listing sysctls (`KERN_PROC`): outside the jail they name every process with its
/// owner and parent. In the jail each is refused, for root's processes, the user's and a
/// `sleep` the test starts outside (by pid and by its own process group), while the jail's own
/// child still reads. The sandbox checks them as `sysctl-read`, not `process-info-listpids`.
#[test]
fn process_listing_sysctls_are_refused() {
    let j = Jail::new();
    let spec = LaunchSpec::new(
        "/bin/sleep".into(),
        vec!["60".into()],
        BTreeMap::new(),
        j.m1(),
    );
    let mut sleeper = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec.unwrap())
        .unwrap();
    let pid = sleeper.handle.pid().to_string();
    let pgid = run_outside("/bin/ps", &["-o", "pgid=", "-p", &pid], &j.m1());
    let pgid = pgid.assert_ok().stdout.trim().to_owned();
    let uid = run_outside("/usr/bin/id", &["-u"], &j.m1());
    let uid = uid.assert_ok().stdout.trim().to_owned();
    let pgid_of_1 = run_outside("/bin/ps", &["-o", "pgid=", "-p", "1"], &j.m1());
    let pgid_of_1 = pgid_of_1.assert_ok().stdout.trim().to_owned();
    for (kind, arg) in [
        ("ALL", "0"),
        ("UID", "0"),
        ("UID", uid.as_str()),
        ("RUID", "0"),
        ("PGRP", pgid_of_1.as_str()),
        ("PGRP", pgid.as_str()),
        ("PID", "1"),
        ("PID", pid.as_str()),
    ] {
        j.probe_outside(&["kproc", kind, arg]).assert_ok();
        let inside = j.probe(&["kproc", kind, arg]);
        assert_refused_with(&inside, "Operation not permitted");
    }
    let own = r#"/bin/sleep 5 & "$1" kproc PID $!"#;
    let out = j.sh(own, &[j.w.at("tools/probe").to_str().unwrap()]);
    assert!(out.stdout.contains("ALLOWED kproc"), "{out:#?}");
    run_outside("/bin/kill", &["-TERM", &pid], &j.m1()).assert_ok();
    async_io::block_on(sleeper.handle.exited());
}

/// Item 2: IPv6 and UDP to outside addresses, and a listener on every address, with a proxy
/// port in the policy. Each works outside the jail but for the network itself, which fails
/// otherwise than EPERM.
#[test]
fn ipv6_udp_and_a_listener_on_every_address_are_refused() {
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = proxy.local_addr().unwrap().port();
    let j = Jail::with(|b| b.proxy_port(port));
    for op in ["udp", "tcp6", "udp6", "bind", "bind6"] {
        let out = run(&j.launcher, "/usr/bin/perl", &["-e", NET, op], &j.m1(), &[]);
        assert_refused_with(&out, &format!("{op}: Operation not permitted"));
    }
    let bind = run_outside("/usr/bin/perl", &["-e", NET, "bind"], &j.m1());
    assert!(bind.assert_ok().stdout.contains("DONE"), "{bind:#?}");
}

/// A throwaway `ssh-agent` in the foreground, ended when dropped.
struct Agent(clauseo_process::Launched);

impl Drop for Agent {
    fn drop(&mut self) {
        clauseo_process::kill(self.0.handle.pid());
        async_io::block_on(self.0.handle.exited());
    }
}

/// Item 3: the lawyer's SSH agent, its socket handed in as `SSH_AUTH_SOCK`.
#[test]
fn the_ssh_agent_cannot_be_reached() {
    let j = Jail::new();
    let socket = j.w.at("outside/agent.sock");
    let args = vec!["-D".into(), "-a".into(), socket.clone().into()];
    let spec = LaunchSpec::new("/usr/bin/ssh-agent".into(), args, BTreeMap::new(), j.m1());
    let mut launched = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec.unwrap())
        .unwrap();
    // The agent prints its socket's name once it listens there.
    let mut said = [0_u8; 256];
    let n = async_io::block_on(launched.stdout.read(&mut said)).unwrap();
    assert!(String::from_utf8_lossy(&said[..n]).contains("SSH_AUTH_SOCK"));
    let _agent = Agent(launched);
    let socket = socket.to_str().unwrap();
    // Outside it answers (no identities: exit 1); in the jail the connection is refused.
    let env = [("SSH_AUTH_SOCK", socket)];
    let outside = PassthroughLauncher::new(SpawnOptions::new().clear_env());
    let listed = run(&outside, "/usr/bin/ssh-add", &["-l"], &j.m1(), &env);
    assert!(listed.stdout.contains("no identities"), "{listed:#?}");
    let out = run(&j.launcher, "/usr/bin/ssh-add", &["-l"], &j.m1(), &env);
    out.assert_refused();
    assert!(out.stderr.contains("Error connecting to agent"), "{out:#?}");
}

/// Chrome's port rendezvous: Chrome runs outside the agent's jail in a profile of its own, so
/// the jail, browser socket folder and all, neither looks up nor registers its names. Refused
/// by the sandbox (`BOOTSTRAP_NOT_PRIVILEGED`), not unknown to launchd.
#[test]
fn chromes_rendezvous_names_are_refused() {
    let sockets = clauseo_sandbox::BrowserSocketDir::create().unwrap();
    let j = Jail::with(|b| b.browser_socket_dir(&sockets));
    for prefix in [
        "org.chromium.Chromium.MachPortRendezvousServer",
        "com.google.Chrome.MachPortRendezvousServer",
    ] {
        let name = format!("{prefix}.{}", std::process::id());
        let refused = "BOOTSTRAP_NOT_PRIVILEGED";
        assert_refused_with(&j.probe(&["lookup", &name]), refused);
        assert_refused_with(&j.probe(&["checkin", &name]), refused);
    }
}

/// Item 4: an Apple Event to Finder. Not tried outside: it would ask the user for Automation.
#[test]
fn apple_events_are_refused() {
    let j = Jail::new();
    let script = r#"tell application "Finder" to get name of startup disk"#;
    let out = run(
        &j.launcher,
        "/usr/bin/osascript",
        &["-e", script],
        &j.m1(),
        &[],
    );
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert!(out.stdout.trim().is_empty(), "{out:#?}");
}

/// The Command Line Tools' or Xcode's `git-credential-osxkeychain`.
fn credential_helper(cwd: &std::path::Path) -> PathBuf {
    let out = run_outside("/usr/bin/xcode-select", &["-p"], cwd);
    PathBuf::from(out.assert_ok().stdout.trim())
        .join("usr/libexec/git-core/git-credential-osxkeychain")
}

/// The throwaway Keychain item and git credential, removed when dropped (and before the test
/// makes them, should a failed run have left them).
struct Throwaway {
    service: String,
    helper: PathBuf,
    cwd: PathBuf,
}

const HOST: &str = "protocol=https\nhost=clauseo-jail-test.invalid\nusername=clauseo\n";

impl Throwaway {
    fn erase(&self) {
        let args = ["delete-generic-password", "-s", &self.service];
        drop(run_outside("/usr/bin/security", &args, &self.cwd));
        let erase = r#"printf '%s\n' "$1" | "$2" erase"#;
        let helper = self.helper.to_str().unwrap();
        drop(run_outside(
            "/bin/sh",
            &["-c", erase, "sh", HOST, helper],
            &self.cwd,
        ));
    }
}

impl Drop for Throwaway {
    fn drop(&mut self) {
        self.erase();
    }
}

/// Item 5: under the Claude profile (Keychain allowed, the real home), neither a login-Keychain
/// item nor a git credential made outside the jail can be read. Fails today: the Keychain
/// rules let `security` and the git helper read both.
#[test]
#[ignore = "keychain exposure: awaiting the orchestrator's fix"]
fn the_claude_profile_cannot_read_the_keychain() {
    let w = World::new();
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let m1 = w.at("m1");
    let helper = credential_helper(&m1);
    let runtime = crate::git::git_runtime(&m1);
    let policy = w.builder().home(home).keychain(true).runtime(runtime);
    let (launcher, _) = launcher(policy.build().unwrap());
    let secret = unique("clauseo-jail-secret");
    let items = Throwaway {
        service: unique("clauseo-jail-test"),
        helper: helper.clone(),
        cwd: m1.clone(),
    };
    items.erase();
    let add = [
        "add-generic-password",
        "-s",
        &items.service,
        "-a",
        "clauseo",
        "-w",
        &secret,
    ];
    run_outside("/usr/bin/security", &add, &m1).assert_ok();
    let store = r#"printf '%spassword=%s\n\n' "$1" "$2" | "$3" store"#;
    let helper = helper.to_str().unwrap();
    run_outside("/bin/sh", &["-c", store, "sh", HOST, &secret, helper], &m1).assert_ok();

    let find = ["find-generic-password", "-s", &items.service, "-w"];
    let found = run(&launcher, "/usr/bin/security", &find, &m1, &[]);
    let get = r#"printf '%s\n' "$1" | "$2" get"#;
    let got = sh(&launcher, &m1, get, &[HOST, helper]);
    // Both tried before either is judged, so a failure names every read that got through.
    let item = found.stdout.contains(&secret);
    let credential = got.stdout.contains(&secret);
    assert!(
        !item && !credential,
        "the jail read: item {item}, git credential {credential}"
    );
}
