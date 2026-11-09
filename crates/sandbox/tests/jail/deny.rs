//! The default-deny audit: each operation class the profile leaves to `(deny default)` (or, where
//! macOS 27 showed that is not enough, to a rule naming it), tried as an ordinary action in the
//! jail and seen refused. Where the user may do the action outside the jail, the test does it
//! there too, so the refusal is the jail's and not the Unix permissions'. File refusals are also
//! seen in the kernel Sandbox's log, through the launcher's denial watcher.
//!
//! The small actions with no system tool are a C program, `PROBE`, built in the test's folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};

use clauseo_sandbox::Denial;

use crate::support::{Output, World, launcher, run, run_outside, sh};

/// One action per run, named by the first argument. Prints `ALLOWED <action>` and exits 0, or
/// prints `REFUSED <action>: <why>` on stderr and exits 1. `<why>` is `strerror` for a system
/// call, `BOOTSTRAP_NOT_PRIVILEGED` for a refused Mach name and `kIOReturnNotPermitted` for a
/// refused `IOKit` open.
const PROBE: &str = r#"
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <semaphore.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/clonefile.h>
#include <sys/ioccom.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/sysctl.h>
#include <unistd.h>
#include <netinet/in.h>
#include <netinet/ip_icmp.h>
#include <mach/mach.h>
#include <servers/bootstrap.h>
#include <IOKit/IOKitLib.h>
#include <libproc.h>

#define FSIOC_SYNC_VOLUME _IOW('A', 1, uint32_t)
int fsctl(const char *, unsigned long, void *, unsigned int);

static int sys(const char *what, int ok) {
    if (ok) { printf("ALLOWED %s\n", what); return 0; }
    fprintf(stderr, "REFUSED %s: %s\n", what, strerror(errno));
    return 1;
}

static int mach(const char *what, kern_return_t kr) {
    if (kr == KERN_SUCCESS) { printf("ALLOWED %s\n", what); return 0; }
    if (kr == BOOTSTRAP_NOT_PRIVILEGED) {
        fprintf(stderr, "REFUSED %s: BOOTSTRAP_NOT_PRIVILEGED\n", what);
    } else {
        fprintf(stderr, "REFUSED %s: kr %d\n", what, kr);
    }
    return 1;
}

static struct sockaddr_in ipv4(const char *addr, int port) {
    struct sockaddr_in a = {0};
    a.sin_len = sizeof a;
    a.sin_family = AF_INET;
    a.sin_port = htons(port);
    inet_pton(AF_INET, addr, &a.sin_addr);
    return a;
}

int main(int argc, char **argv) {
    const char *op = argc > 1 ? argv[1] : "";
    const char *a = argc > 2 ? argv[2] : "";
    const char *b = argc > 3 ? argv[3] : "";
    mach_port_t port;
    if (!strcmp(op, "lookup")) return mach("lookup", bootstrap_look_up(bootstrap_port, a, &port));
    if (!strcmp(op, "checkin")) {
        return mach("checkin", bootstrap_check_in(bootstrap_port, a, &port));
    }
    if (!strcmp(op, "iokit")) {
        io_service_t s = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching(a));
        if (!s) { fprintf(stderr, "NO SERVICE %s\n", a); return 2; }
        io_connect_t c;
        kern_return_t kr = IOServiceOpen(s, mach_task_self(), 0, &c);
        if (kr == kIOReturnSuccess) { printf("ALLOWED iokit\n"); return 0; }
        if (kr == kIOReturnNotPermitted) fprintf(stderr, "REFUSED iokit: kIOReturnNotPermitted\n");
        else fprintf(stderr, "REFUSED iokit: 0x%x\n", kr);
        return 1;
    }
    if (!strcmp(op, "shm")) {
        int fd = shm_open(a, O_CREAT | O_RDWR, 0600);
        if (fd >= 0) shm_unlink(a);
        return sys("shm_open", fd >= 0);
    }
    if (!strcmp(op, "sem")) {
        sem_t *s = sem_open(a, O_CREAT, 0600, 1);
        if (s != SEM_FAILED) sem_unlink(a);
        return sys("sem_open", s != SEM_FAILED);
    }
    if (!strcmp(op, "route")) return sys("route socket", socket(AF_ROUTE, SOCK_RAW, 0) >= 0);
    if (!strcmp(op, "raw")) return sys("raw socket", socket(AF_INET, SOCK_RAW, IPPROTO_ICMP) >= 0);
    if (!strcmp(op, "icmp")) {
        int s = socket(AF_INET, SOCK_DGRAM, IPPROTO_ICMP);
        struct icmp echo = {0};
        echo.icmp_type = ICMP_ECHO;
        struct sockaddr_in to = ipv4(a, 0);
        return sys("icmp echo", sendto(s, &echo, sizeof echo, 0, (void *)&to, sizeof to) >= 0);
    }
    if (!strcmp(op, "tcp")) {
        int s = socket(AF_INET, SOCK_STREAM, 0);
        struct sockaddr_in to = ipv4(a, atoi(b));
        return sys("tcp connect", connect(s, (void *)&to, sizeof to) == 0);
    }
    if (!strcmp(op, "sysctl")) {
        int zero = 0;
        return sys("sysctl write", sysctlbyname(a, NULL, NULL, &zero, sizeof zero) == 0);
    }
    if (!strcmp(op, "fsctl")) {
        uint32_t flags = 0;
        return sys("fsctl", fsctl(a, FSIOC_SYNC_VOLUME, &flags, 0) == 0);
    }
    if (!strcmp(op, "procargs")) {
        int mib[3] = {CTL_KERN, KERN_PROCARGS2, atoi(a)};
        char args[65536];
        size_t size = sizeof args;
        return sys("procargs", sysctl(mib, 3, args, &size, NULL, 0) == 0 && size > 0);
    }
    if (!strcmp(op, "kinfo")) {
        struct kinfo_proc info;
        size_t size = sizeof info;
        int mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, atoi(a)};
        return sys("kinfo", sysctl(mib, 4, &info, &size, NULL, 0) == 0 && size > 0);
    }
    if (!strcmp(op, "kproc")) {
        int kinds[] = {KERN_PROC_ALL, KERN_PROC_UID, KERN_PROC_RUID, KERN_PROC_PGRP, KERN_PROC_PID};
        const char *names[] = {"ALL", "UID", "RUID", "PGRP", "PID"};
        int mib[4] = {CTL_KERN, KERN_PROC, -1, atoi(b)};
        for (int i = 0; i < 5; i++) if (!strcmp(a, names[i])) mib[2] = kinds[i];
        size_t size = 0;
        int n = mib[2] == KERN_PROC_ALL ? 3 : 4;
        return sys("kproc", sysctl(mib, n, NULL, &size, NULL, 0) == 0 && size > 0);
    }
    if (!strcmp(op, "listall")) {
        static pid_t pids[65536];
        return sys("listall", proc_listallpids(pids, sizeof pids) > 0);
    }
    if (!strcmp(op, "children")) {
        static pid_t pids[4096];
        int n = proc_listchildpids(getppid(), pids, sizeof pids), found = 0;
        for (int i = 0; i < n; i++) found |= pids[i] == atoi(a);
        return sys("children", found);
    }
    if (!strcmp(op, "pidpath")) {
        char path[PROC_PIDPATHINFO_MAXSIZE];
        return sys("pidpath", proc_pidpath(atoi(a), path, sizeof path) > 0);
    }
    if (!strcmp(op, "link")) return sys("link", link(a, b) == 0);
    if (!strcmp(op, "clone")) return sys("clonefile", clonefile(a, b, 0) == 0);
    fprintf(stderr, "unknown action %s\n", op);
    return 2;
}
"#;

/// The world and its policy's launcher, with [`PROBE`] built at `tools/probe`.
pub(crate) struct Jail {
    pub(crate) w: World,
    pub(crate) launcher: clauseo_sandbox::SeatbeltLauncher,
    denials: mpsc::Receiver<Denial>,
    probe: PathBuf,
}

impl Jail {
    pub(crate) fn new() -> Jail {
        Jail::with(|b| b)
    }

    pub(crate) fn with(
        policy: impl FnOnce(clauseo_sandbox::PolicyBuilder) -> clauseo_sandbox::PolicyBuilder,
    ) -> Jail {
        let w = World::new();
        let probe = w.compile_c("tools/probe", PROBE, &["IOKit"]);
        let policy = policy(w.builder().read(w.at("tools"))).build().unwrap();
        let (launcher, denials) = launcher(policy);
        // The watcher starts with the first launch; later denials reach it.
        run(&launcher, "/usr/bin/true", &[], &w.at("m1"), &[]).assert_ok();
        Jail {
            w,
            launcher,
            denials,
            probe,
        }
    }

    pub(crate) fn m1(&self) -> PathBuf {
        self.w.at("m1")
    }

    /// `probe args` in the jail.
    pub(crate) fn probe(&self, args: &[&str]) -> Output {
        run(&self.launcher, self.probe_path(), args, &self.m1(), &[])
    }

    /// `probe args` outside the jail.
    pub(crate) fn probe_outside(&self, args: &[&str]) -> Output {
        run_outside(self.probe_path(), args, &self.m1())
    }

    fn probe_path(&self) -> &str {
        self.probe.to_str().unwrap()
    }

    pub(crate) fn sh(&self, script: &str, args: &[&str]) -> Output {
        sh(&self.launcher, &self.m1(), script, args)
    }

    /// Waits for the watcher to report a refused access to `path`, until two seconds pass with
    /// no denial: the kernel Sandbox logged it, so the jail refused it and not the file's mode.
    #[track_caller]
    pub(crate) fn assert_logged(&self, path: &Path) {
        let mut seen = Vec::new();
        while let Ok(denial) = self.denials.recv_timeout(Duration::from_secs(2)) {
            match denial {
                Denial::Path { path: p, .. } if p == path => return,
                other => seen.push(other),
            }
        }
        panic!("no Sandbox denial for {}; saw {seen:#?}", path.display());
    }
}

/// Exited non-zero, `why` on stderr.
#[track_caller]
pub(crate) fn assert_refused_with(out: &Output, why: &str) {
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert!(out.stderr.contains(why), "expected {why:?}: {out:#?}");
}

/// A name no other run of the tests uses.
pub(crate) fn unique(base: &str) -> String {
    format!(
        "{base}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )
    .replace(['(', ')'], "")
}

/// Class 1: writes outside the writable roots.
#[test]
fn writes_outside_the_writable_roots_are_refused() {
    let j = Jail::new();
    let name = unique("csb-audit");
    let mut targets = vec![
        PathBuf::from(format!("/private/tmp/{name}.txt")),
        j.w.at("home/new.txt"),
        j.w.at("outside/new.txt"),
        j.w.at("data/agent-state/m2/notes/new.txt"),
    ];
    // The real Desktop, when this user has one (a missing folder fails before the jail asks).
    let desktop = PathBuf::from(std::env::var("HOME").unwrap()).join("Desktop");
    if desktop.is_dir() {
        targets.push(
            dunce::canonicalize(&desktop)
                .unwrap()
                .join(format!("{name}.txt")),
        );
    }
    for target in &targets {
        let out = j.sh(r#"printf x > "$1""#, &[target.to_str().unwrap()]);
        out.assert_refused();
        assert!(!target.exists(), "{} was made", target.display());
    }
    j.assert_logged(&targets[0]);
}

/// Class 2: the real home folder and its `.ssh`; the stand-ins, the sibling matter and another
/// matter's notes are `files::reads_outside_the_policy_fail`.
#[test]
fn the_home_folder_and_its_ssh_folder_cannot_be_read() {
    let j = Jail::new();
    let home = dunce::canonicalize(std::env::var("HOME").unwrap()).unwrap();
    let ssh = home.join(".ssh");
    run_outside("/bin/ls", &[home.to_str().unwrap()], &j.m1()).assert_ok();
    run(
        &j.launcher,
        "/bin/ls",
        &[home.to_str().unwrap()],
        &j.m1(),
        &[],
    )
    .assert_refused();
    // Refused whether or not this user has one: its metadata is out of reach too.
    let out = run(
        &j.launcher,
        "/bin/ls",
        &[ssh.to_str().unwrap()],
        &j.m1(),
        &[],
    );
    out.assert_refused();
    let known = ssh.join("known_hosts");
    run(
        &j.launcher,
        "/bin/cat",
        &[known.to_str().unwrap()],
        &j.m1(),
        &[],
    )
    .assert_refused();
    // The watcher reports data reads, not listings or metadata reads: the Sandbox's own line.
    if known.exists() {
        j.assert_logged(&known);
    }
}

/// Class 3: a direct connection past the proxy, to an outside address (TCP and an ICMP echo).
/// A missing network fails differently (unreachable, timed out); the jail's refusal is EPERM.
/// A loopback port that is not the proxy's is `net::tcp_reaches_only_the_proxy_port`.
#[test]
fn a_direct_connection_to_an_outside_address_is_refused() {
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = proxy.local_addr().unwrap().port();
    let j = Jail::with(|b| b.proxy_port(port));
    let refused = "Operation not permitted";
    assert_refused_with(&j.probe(&["tcp", "1.1.1.1", "443"]), refused);
    assert_refused_with(&j.probe(&["icmp", "1.1.1.1"]), refused);
    let nc = ["-v", "-z", "-G", "5", "1.1.1.1", "443"];
    run(&j.launcher, "/usr/bin/nc", &nc, &j.m1(), &[]).assert_refused();
    j.probe(&["tcp", "127.0.0.1", &port.to_string()])
        .assert_ok();
}

/// Class 4: running programs is allowed, and what they run stays jailed: a grandchild in a new
/// session of its own (`setsid`) cannot write outside, and the write to the matter works.
#[test]
fn children_in_a_session_of_their_own_stay_jailed() {
    let j = Jail::new();
    let outside = j.w.at("outside/child.txt");
    let inside = j.w.at("m1/child.txt");
    let script = r#"use POSIX; my $pid = fork() // die "fork: $!\n";
        if ($pid == 0) { POSIX::setsid(); exec "/bin/sh", "-c",
            'printf x > "$1"; printf x > "$2"', "sh", @ARGV or die "exec: $!\n"; }
        waitpid($pid, 0); exit($? >> 8)"#;
    let out = j.sh(
        r#"/usr/bin/perl -e "$1" "$2" "$3""#,
        &[script, outside.to_str().unwrap(), inside.to_str().unwrap()],
    );
    assert!(out.stderr.contains("Operation not permitted"), "{out:#?}");
    assert!(!outside.exists());
    assert_eq!(std::fs::read_to_string(&inside).unwrap(), "x");
    j.assert_logged(&outside);
}

/// Class 5: Mach services the base does not name (the Keychain's, the pasteboard,
/// `LaunchServices` behind `open -a`) and registering a name of one's own. Each works outside.
#[test]
fn sensitive_mach_services_are_refused() {
    let j = Jail::new();
    for name in [
        "com.apple.SecurityServer",
        "com.apple.securityd.xpc",
        "com.apple.pasteboard.1",
        "com.apple.coreservices.launchservicesd",
    ] {
        j.probe_outside(&["lookup", name]).assert_ok();
        assert_refused_with(&j.probe(&["lookup", name]), "BOOTSTRAP_NOT_PRIVILEGED");
    }
    let mine = unique("chat.clauseo.audit");
    j.probe_outside(&["checkin", &mine]).assert_ok();
    let mine = unique("chat.clauseo.audit.jail");
    assert_refused_with(&j.probe(&["checkin", &mine]), "BOOTSTRAP_NOT_PRIVILEGED");
    // The tools themselves: no pasteboard read, no app opened.
    let paste = run(&j.launcher, "/usr/bin/pbpaste", &[], &j.m1(), &[]);
    assert_ne!(paste.code, Some(0), "{paste:#?}");
    let open = run(
        &j.launcher,
        "/usr/bin/open",
        &["-a", "Finder"],
        &j.m1(),
        &[],
    );
    assert_ne!(open.code, Some(0), "{open:#?}");
}

/// Class 6: a signal to a process outside the jail, which the same user may signal outside.
#[test]
fn a_signal_to_a_process_outside_is_refused() {
    let j = Jail::new();
    let spec = LaunchSpec::new(
        "/bin/sleep".into(),
        vec!["60".into()],
        BTreeMap::new(),
        j.m1(),
    );
    let outside = PassthroughLauncher::new(SpawnOptions::new());
    let mut sleeper = outside.launch(&spec.unwrap()).unwrap();
    let pid = sleeper.handle.pid();
    let out = run(
        &j.launcher,
        "/bin/kill",
        &["-TERM", &pid.to_string()],
        &j.m1(),
        &[],
    );
    out.assert_refused();
    assert!(clauseo_process::running(pid), "the sleeper ended");
    run_outside("/bin/kill", &["-TERM", &pid.to_string()], &j.m1()).assert_ok();
    async_io::block_on(sleeper.handle.exited());
}

/// Class 7: a sysctl write. `kern.system_version_compat`, a flag on the writing process, is one
/// any user may write, so outside it works and in the jail only the sandbox refuses it.
#[test]
fn a_sysctl_write_is_refused() {
    let j = Jail::new();
    let name = "kern.system_version_compat";
    j.probe_outside(&["sysctl", name]).assert_ok();
    assert_refused_with(&j.probe(&["sysctl", name]), "Operation not permitted");
    let sysctl = ["-w", "kern.maxfiles=1"];
    run(&j.launcher, "/usr/sbin/sysctl", &sysctl, &j.m1(), &[]).assert_refused();
}

/// Class 8: opening an `IOKit` user client other than the power manager's, which the base allows
/// for sleep assertions.
#[test]
fn an_iokit_user_client_cannot_be_opened() {
    let j = Jail::new();
    j.probe_outside(&["iokit", "IOSurfaceRoot"]).assert_ok();
    assert_refused_with(
        &j.probe(&["iokit", "IOSurfaceRoot"]),
        "kIOReturnNotPermitted",
    );
    j.probe(&["iokit", "IOPMrootDomain"]).assert_ok();
}

/// Class 9: hard links and clones across the jail's edge, both ways; within the matter they
/// work.
#[test]
fn links_and_clones_across_the_edge_are_refused() {
    let j = Jail::new();
    j.w.file("m1/brief.txt", "brief\n");
    let secret = j.w.at("outside/secret.txt");
    let brief = j.w.at("m1/brief.txt");
    let s = |p: &Path| p.to_str().unwrap().to_owned();
    for (n, op) in ["link", "clone"].into_iter().enumerate() {
        let into = j.w.at(&format!("m1/in-{n}.txt"));
        let out_of = j.w.at(&format!("outside/out-{n}.txt"));
        let home = j.w.at(&format!("home/out-{n}.txt"));
        for (from, to) in [(&secret, &into), (&brief, &out_of), (&brief, &home)] {
            assert_refused_with(&j.probe(&[op, &s(from), &s(to)]), "Operation not permitted");
            assert!(!to.exists(), "{op} made {}", to.display());
        }
        let copy = j.w.at(&format!("m1/copy-{n}.txt"));
        j.probe(&[op, &s(&brief), &s(&copy)]).assert_ok();
    }
    j.assert_logged(&j.w.at("outside/out-0.txt"));
    // The tools: `ln` and `cp -c` (clonefile).
    let ln = j.sh(r#"ln "$1" "$2""#, &[&s(&secret), &s(&j.w.at("m1/ln.txt"))]);
    ln.assert_refused();
    let cp = j.sh(
        r#"cp -c "$1" "$2""#,
        &[&s(&brief), &s(&j.w.at("outside/cp.txt"))],
    );
    cp.assert_refused();
}

/// Class 10: POSIX shared memory is refused but for notify(3)'s read-only page; POSIX
/// semaphores are allowed by intent (`(allow ipc-posix-sem)` in base.sb: Python's
/// multiprocessing locks).
#[test]
fn posix_shared_memory_is_refused_and_semaphores_work() {
    let j = Jail::new();
    let name = format!("/csb-{}", std::process::id());
    j.probe_outside(&["shm", &name]).assert_ok();
    assert_refused_with(&j.probe(&["shm", &name]), "Operation not permitted");
    j.probe(&["sem", &name]).assert_ok();
}

/// Class 11: NVRAM variables, which `(deny default)` let a jailed `nvram -p` print on macOS
/// 27.2 until the profile named `nvram*`.
#[test]
fn nvram_variables_cannot_be_read() {
    let j = Jail::new();
    // Outside the jail the same user sees variables, so an empty list inside is the jail's.
    let outside = run_outside("/usr/sbin/nvram", &["-p"], &j.m1());
    let name = outside.assert_ok().stdout.split_whitespace().next();
    let name = name.unwrap_or_else(|| panic!("nvram -p printed nothing outside: {outside:#?}"));
    let inside = run(&j.launcher, "/usr/sbin/nvram", &["-p"], &j.m1(), &[]);
    assert!(inside.stdout.trim().is_empty(), "{inside:#?}");
    let one = run(&j.launcher, "/usr/sbin/nvram", &[name], &j.m1(), &[]);
    assert_ne!(one.code, Some(0), "{one:#?}");
    assert!(!one.stdout.contains(name), "{one:#?}");
}

/// Class 12: `system-socket` (a routing socket, which any user may open outside; a raw socket,
/// which Unix refuses a non-root user anyway) and `system-fsctl` (a volume sync).
#[test]
fn system_sockets_and_fsctl_are_refused() {
    let j = Jail::new();
    let refused = "Operation not permitted";
    j.probe_outside(&["route"]).assert_ok();
    assert_refused_with(&j.probe(&["route"]), refused);
    assert_refused_with(&j.probe(&["raw"]), refused);
    let file = j.w.file("m1/f.txt", "f\n");
    let file = file.to_str().unwrap();
    j.probe_outside(&["fsctl", file]).assert_ok();
    assert_refused_with(&j.probe(&["fsctl", file]), refused);
}

/// Class 8, the registry: `(deny default)` let a jailed `ioreg` read the Mac's serial number
/// on macOS 27.2 until the profile named `iokit-get-properties`.
#[test]
fn the_serial_number_cannot_be_read_from_the_registry() {
    let j = Jail::new();
    let args = ["-r", "-d", "1", "-c", "IOPlatformExpertDevice"];
    let key = "\"IOPlatformSerialNumber\" = \"";
    let outside = run_outside("/usr/sbin/ioreg", &args, &j.m1());
    let line = outside.assert_ok().stdout.lines().find(|l| l.contains(key));
    let serial = line.and_then(|l| l.rsplit('"').nth(1));
    let serial = serial.unwrap_or_else(|| panic!("no serial number outside: {outside:#?}"));
    // The serial number itself, in neither stream, and not under any other name.
    let inside = run(&j.launcher, "/usr/sbin/ioreg", &args, &j.m1(), &[]);
    assert!(
        !inside.stdout.contains(serial),
        "the serial number is in stdout"
    );
    assert!(
        !inside.stderr.contains(serial),
        "the serial number is in stderr"
    );
    assert!(
        !inside.stdout.contains("IOPlatformSerialNumber"),
        "{inside:#?}"
    );
}
