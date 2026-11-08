//! The app's check of the set-up pipe (`setup_task::check_pipe`) as a standard user, against a
//! server LOCAL SYSTEM runs (review item B1): the production server end, made by SYSTEM with the
//! production descriptor, and the production client, run as a user. It shows what a standard
//! user can read of a SYSTEM server (the owner, the session, the process list's user, the image
//! by id), and asserts the client takes the pipe and gets a reply.
//!
//! This test binary runs three times. The elevated test registers a LOCAL SYSTEM task in a test
//! folder whose action runs a copy of this binary's [`system_helper_serves`] (cmd sets the pipe's
//! name), then runs the copy's [`user_helper_asks`] as a standard test account through the
//! Secondary Logon service. Each helper is ignored and does nothing unless its variables are
//! set, so no other run starts them; the elevated step's filter (`elevated_`) skips them.
//!
//! The server's host is the fake one: the gate, sessions and set-up are made up, so the user is
//! refused (it isn't the fake session's user). A refusal is still a reply, which the client
//! reads only after its check passed and its request went out.
#![expect(
    unsafe_code,
    reason = "FFI to the Task Scheduler and to wait for the pipe"
)]

use std::path::{Path, PathBuf};

use windows::Win32::System::Pipes::WaitNamedPipeW;
use windows::Win32::System::Variant::VARIANT;
use windows::core::HSTRING;

use super::super::tests::{FakeHost, SET_UP};
use super::super::windows_pipe::WindowsPipe;
use super::super::{ServerFacts, TOKEN_BYTES, pipe_name, run};
use super::gate;
use crate::test_support::TempDir;
use crate::windows::record::current_user_sid;
use crate::windows::refresh::task::{Apartment, service};
use crate::windows::refresh::windows_tests::{Scheduled, try_in_cmd};
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::setup_task::{ask, open, read_facts};
use crate::windows::test_dacl::protect_with;

/// The server helper's variable: the run's token, which names the pipe (`pipe_name`).
const SERVE_PIPE: &str = "CLAUSEO_SET_UP_TEST_SERVE";
/// The client helper's variables: the run's token, and the image it expects.
const ASK_PIPE: &str = "CLAUSEO_SET_UP_TEST_ASK";
const ASK_EXE: &str = "CLAUSEO_SET_UP_TEST_EXE";
/// How many half-second looks the client takes for the pipe: SYSTEM's run starts this binary.
const PIPE_LOOKS: u32 = 120;

/// `test`'s name as this binary's harness knows it, for `--exact`.
fn harness_name(test: &str) -> String {
    let module = module_path!();
    let module = module
        .split_once("::")
        .map_or(module, |(_crate, rest)| rest);
    format!("{module}::{test}")
}

/// `fact` as a line: its value, or why it wasn't given.
fn shown<T: std::fmt::Display>(fact: &Result<T, String>) -> String {
    match fact {
        Ok(value) => value.to_string(),
        Err(why) => format!("unread ({why})"),
    }
}

/// Run as LOCAL SYSTEM by the elevated test's task: serves the pipe its variable names with the
/// production pipe and descriptor, over the fake host, then prints how it ended.
#[test]
#[ignore = "a helper: the elevated test's SYSTEM task runs it"]
fn system_helper_serves() {
    let Ok(short) = std::env::var(SERVE_PIPE) else {
        return;
    };
    let me = current_user_sid().unwrap();
    assert_eq!(me.as_str(), "S-1-5-18", "only SYSTEM serves");
    let name = pipe_name(&short).unwrap();
    let mut host = FakeHost::new();
    let exit = run(&mut host, |_name, sddl| {
        println!("descriptor: {sddl}");
        WindowsPipe::create(&name, sddl)
    });
    println!("exit: {exit:?}\nlines: {:?}", host.lines);
}

/// Run as a standard user by the elevated test: waits for the pipe its variable names, prints
/// what it can read of the server, then asks through the app's own client and prints the answer.
#[test]
#[ignore = "a helper: the elevated test runs it as a standard user"]
fn user_helper_asks() {
    let (Ok(short), Ok(exe)) = (std::env::var(ASK_PIPE), std::env::var(ASK_EXE)) else {
        return;
    };
    let name = pipe_name(&short).unwrap();
    let wait = || {
        for _ in 0..PIPE_LOOKS {
            // SAFETY: a live HSTRING; it only waits, up to half a second.
            if unsafe { WaitNamedPipeW(&HSTRING::from(name.as_str()), 500) }.as_bool() {
                return true;
            }
            pause();
        }
        false
    };
    assert!(wait(), "the pipe never came");
    match open(&name) {
        Ok(pipe) => match read_facts(&pipe) {
            Ok(ServerFacts {
                owner,
                session,
                user,
                image,
            }) => println!(
                "owner: {}\nsession: {}\nuser: {}\nimage: {}",
                shown(&owner),
                shown(&session),
                shown(&user),
                shown(&image)
            ),
            Err(exchange) => println!("facts: {exchange:?}"),
        },
        Err(code) => println!("facts: the pipe wouldn't open ({})", code.0),
    }
    assert!(wait(), "the pipe went");
    // The name published before and after the start: a server already running.
    let published = || Ok(Some(short.clone()));
    let answer = ask(published, SET_UP.as_bytes(), Path::new(&exe), || Ok(()));
    println!("answer: {answer:?}");
}

/// Half a second.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait for another process's pipe, with no clock to inject"
)]
fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(500));
}

/// This test binary copied to `to`, which both helpers' accounts can read, by its long name
/// (`dunce::canonicalize`, no `\\?\`): the image the kernel gives is the long one, and a
/// runner's temporary folder may be named short (`RUNNER~1`).
#[expect(
    clippy::disallowed_methods,
    reason = "a test copies its own binary for another account to run"
)]
fn copy_of_this_binary(to: &Path) -> PathBuf {
    std::fs::copy(std::env::current_exe().unwrap(), to).unwrap();
    dunce::canonicalize(to).unwrap()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_standard_user_takes_a_pipe_system_made_and_asks_on_it() {
    if !gate("elevated_a_standard_user_takes_a_pipe_system_made_and_asks_on_it") {
        return;
    }
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let dir = TempDir::new("set-up-pipe-system");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    // A copy both runs can read, by its long name: the image the kernel gives is the long one.
    let copy = copy_of_this_binary(&dir.path().join("serve-test.exe"));
    let (code, printed) = try_in_cmd(&user, dir.path(), "echo ready");
    assert_eq!(code, Some(0), "{printed}");

    // The run's token, as the server would draw and publish it.
    let short = random_hex(TOKEN_BYTES);
    let served = dir.path().join("server.txt");
    let serve = format!(
        r#"/d /s /c "set {SERVE_PIPE}={short}&& "{}" {} --exact --ignored --nocapture > "{}" 2>&1""#,
        copy.display(),
        harness_name("system_helper_serves"),
        served.display()
    );
    let _apartment = Apartment::enter().unwrap();
    let service = service().unwrap();
    let folder = format!("ClauseoTestServe{}", std::process::id());
    let mut scheduled = Scheduled::new(&service, &folder, "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)");
    scheduled.register(&service, "Serve", &serve, "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)");
    // SAFETY: a live task and an empty VARIANT: no parameters.
    unsafe { scheduled.task("Serve").Run(&VARIANT::default()) }.unwrap();

    let client = format!(
        r#"set {ASK_PIPE}={short}&& set "{ASK_EXE}={}"&& "{}" {} --exact --ignored --nocapture"#,
        copy.display(),
        copy.display(),
        harness_name("user_helper_asks")
    );
    let (code, printed) = try_in_cmd(&user, dir.path(), &client);
    eprintln!("the standard user's client: exit {code:?}\n{printed}");
    drop(scheduled);
    let server = std::fs::read_to_string(&served).unwrap_or_else(|error| format!("({error})"));
    eprintln!("the SYSTEM server:\n{server}");
    assert!(
        printed.contains("owner: S-1-5-18"),
        "the pipe is SYSTEM's: {printed}"
    );
    assert!(
        printed.contains("answer: Ok("),
        "the check passed and the request went: {printed}"
    );
    assert_eq!(code, Some(0), "{printed}");
}
