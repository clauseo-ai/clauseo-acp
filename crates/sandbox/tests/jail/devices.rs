//! Terminals in the jail (review round 1, item 12): no `/dev/tty`; a pty the jailed process
//! opens itself works; another terminal of the same user is refused.

use crate::support::{World, launcher, run, sh};

#[test]
fn the_controlling_terminal_is_refused() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    sh(&launcher, &w.at("m1"), "printf x > /dev/tty", &[]).assert_refused();
}

#[test]
fn a_pty_the_jail_opens_works_and_another_terminal_is_refused() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    // `script` opens a pty through /dev/ptmx and runs the command on it.
    let out = run(
        &launcher,
        "/usr/bin/script",
        &["-q", "/dev/null", "/bin/echo", "in-pty"],
        &m1,
        &[],
    );
    assert!(out.assert_ok().stdout.contains("in-pty"), "{out:#?}");
    // A pty this test opened outside the jail stands for another terminal of the user.
    let pty = nix::pty::openpty(None, None).unwrap();
    let name = nix::unistd::ttyname(&pty.slave).unwrap();
    let out = sh(
        &launcher,
        &m1,
        r#"printf x > "$1""#,
        &[name.to_str().unwrap()],
    );
    out.assert_refused();
    let out = sh(&launcher, &m1, r#"cat < "$1""#, &[name.to_str().unwrap()]);
    out.assert_refused();
}
