//! The set-up pipe's server on a real machine: the production pipe (`windows_pipe.rs`) and the
//! production order ([`run`]) over a fake host, with two real clients. A standard test user's
//! PowerShell, which isn't the session's user, is refused; this elevated process, which the fake
//! host names as the session's user, is served. Both identities are read from the clients'
//! tokens at identification level.
//!
//! CI's runner may sit in session 0 and can't call `WTSQueryUserToken` (it isn't LOCAL SYSTEM),
//! so the fake host names the session's user, and the test's pipe reports every client in one
//! made-up session, [`SESSION`]; the users are the tokens' own. The test pipe's DACL lets
//! interactive users read and write (`GRGW`), as .NET's client asks for both; the production
//! DACL is `pipe_sddl`'s, tested on every OS.
//!
//! Elevated, from CI's elevated step only (`CLAUSEO_ELEVATED_TESTS=1`): it makes a standard
//! account (`ClauseoSbTT` and six hex digits), which runs PowerShell through the Secondary Logon
//! service, unelevated. Everything is removed on drop, a failed assertion included.
#![expect(unsafe_code, reason = "FFI to open the pipe as a client")]

use std::time::Duration;

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::core::HSTRING;

use super::tests::FakeHost;
use super::windows_pipe::WindowsPipe;
use super::{ClientFacts, Pipe, Refusal, Reply, ServeExit, run};
use crate::test_support::TempDir;
use crate::windows::acl::object::Handle;
use crate::windows::record::current_user_sid;
use crate::windows::refresh::windows_tests::try_in_cmd;
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::test_dacl::{elevated, protect_with};

/// The session the test pipe reports every client in.
const SESSION: u32 = 7;
/// The test pipe's DACL: SYSTEM and Administrators everything (the server makes its spare
/// instances), interactive users read and write.
const TEST_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

/// Whether the test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// The production pipe, each client's real facts kept and its session made [`SESSION`].
struct Seen<'a>(WindowsPipe, &'a mut Vec<ClientFacts>);

impl Pipe for Seen<'_> {
    type Client = <WindowsPipe as Pipe>::Client;

    fn next(&mut self, wait: Duration) -> Result<Option<Self::Client>, String> {
        self.0.next(wait)
    }

    fn request(&mut self, client: &mut Self::Client) -> Result<Vec<u8>, String> {
        self.0.request(client)
    }

    fn facts(&mut self, client: &mut Self::Client) -> Result<ClientFacts, String> {
        let facts = self.0.facts(client)?;
        self.1.push(facts.clone());
        Ok(ClientFacts {
            session: SESSION,
            ..facts
        })
    }

    fn reply(&mut self, client: Self::Client, line: &[u8]) {
        self.0.reply(client, line);
    }
}

/// This process as the pipe's client, identification only: the reply line it reads.
fn ask_as_this_process(name: &str) -> String {
    // SAFETY: the name is a live HSTRING; the handle returned is owned by `Handle`.
    let pipe = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
            None,
        )
    }
    .map(Handle)
    .unwrap();
    let mut written = 0u32;
    // SAFETY: a live client end; the request and `written` outlive the call.
    unsafe {
        WriteFile(
            pipe.0,
            Some(br#"{"v":1,"op":"set_up"}"#),
            Some(&raw mut written),
            None,
        )
    }
    .unwrap();
    let mut reply = Vec::new();
    let mut chunk = [0u8; 4096];
    while !reply.contains(&b'\n') {
        let mut read = 0u32;
        // SAFETY: a live client end; `chunk` and `read` outlive the call.
        let done = unsafe { ReadFile(pipe.0, Some(&mut chunk), Some(&raw mut read), None) };
        if done.is_err() || read == 0 {
            break;
        }
        reply.extend_from_slice(&chunk[..read as usize]);
    }
    String::from_utf8_lossy(&reply).into_owned()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_pipe_server_refuses_a_user_who_isnt_the_sessions_user() {
    if !gate("elevated_the_pipe_server_refuses_a_user_who_isnt_the_sessions_user") {
        return;
    }
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let dir = TempDir::new("set-up-pipe");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    // Its first logon makes its profile, which may take longer than the server's first wait.
    let (code, printed) = try_in_cmd(&user, dir.path(), "echo ready");
    assert_eq!(code, Some(0), "{printed}");

    let short = format!("ClauseoTestSetUp{}", std::process::id());
    let name = format!(r"\\.\pipe\{short}");
    let script = dir.file(
        "ask.ps1",
        &format!(
            "$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', '{short}', \
             [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::None, \
             [System.Security.Principal.TokenImpersonationLevel]::Identification)\r\n\
             $p.Connect(60000)\r\n\
             $b = [System.Text.Encoding]::UTF8.GetBytes('{{\"v\":1,\"op\":\"set_up\"}}')\r\n\
             $p.Write($b, 0, $b.Length)\r\n\
             $p.Flush()\r\n\
             $r = New-Object System.IO.StreamReader($p)\r\n\
             Write-Output (\"reply: \" + $r.ReadLine())\r\n"
        ),
    );
    let me = current_user_sid().unwrap();
    let mut host = FakeHost::new();
    host.sessions = vec![(SESSION, me.clone())];
    host.profiles = vec![(me.clone(), dir.path().join("me"))];
    let mut seen = Vec::new();
    let (exit, as_user, as_me) = std::thread::scope(|scope| {
        let clients = scope.spawn(|| {
            let powershell = format!(
                r#""{}\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{}""#,
                crate::windows::refresh::windows_tests::system32(),
                script.display()
            );
            let (code, printed) = try_in_cmd(&user, dir.path(), &powershell);
            eprintln!("the test user's client: exit {code:?}\n{printed}");
            (printed, ask_as_this_process(&name))
        });
        let exit = run(&mut host, |_production_sddl| {
            WindowsPipe::create(&name, TEST_SDDL).map(|pipe| Seen(pipe, &mut seen))
        });
        let (as_user, as_me) = clients.join().unwrap();
        (exit, as_user, as_me)
    });
    eprintln!(
        "this process's client: {as_me}\nthe server's lines: {:?}",
        host.lines
    );
    assert_eq!(exit, ServeExit::Done);
    // The tokens as read at identification level: the test user's, unelevated, then this one's.
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].user, user.sid);
    assert_eq!(seen[0].integrity, 0x2000, "Medium");
    assert!(!seen[0].restricted);
    assert!(
        seen[0]
            .groups
            .iter()
            .any(|group| group.as_str() == "S-1-5-4"),
        "IU"
    );
    assert_eq!(seen[1].user, me);
    let refused = Reply::Refused(Refusal::NotSessionUser.to_string()).to_line();
    let refused = String::from_utf8(refused).unwrap();
    assert!(as_user.contains(refused.trim_end()), "{as_user}");
    assert_eq!(
        Reply::parse(as_me.as_bytes()),
        Ok(Reply::Ran(
            crate::windows::setup::result_file::SetupResult::done()
        ))
    );
    assert!(
        host.calls
            .iter()
            .all(|call| !call.contains(user.sid.as_str())),
        "nothing was read or run for the refused user: {:?}",
        host.calls
    );
}
