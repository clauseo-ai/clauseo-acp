//! The set-up pipe's server on a real machine: the production pipe (`windows_pipe.rs`) with the
//! production descriptor (`pipe_sddl`: owned by LOCAL SYSTEM, the pool's group denied first,
//! interactive users the client rights, the Medium label) and the production order ([`run`])
//! over a fake host. Two standard test users ask through PowerShell: one the fake host names as
//! the session's user is served, the other is refused. Both identities are read from the
//! clients' tokens at identification level. A name another process made first is served to no
//! one, and the app's client sends it nothing.
//!
//! CI's runner may sit in session 0 and can't call `WTSQueryUserToken` (it isn't LOCAL SYSTEM),
//! so the fake host names the session's user, and the test's pipe reports every client in one
//! made-up session, [`SESSION`]; the users are the tokens' own. This process makes the pipe with
//! `SeRestorePrivilege` enabled, which lets an administrator make SYSTEM its owner, as the
//! production server is SYSTEM. The PowerShell clients ask for the client rights alone
//! (`pipe::PIPE_RIGHTS`), which the descriptor grants; .NET's read and write would ask for a new
//! instance too. `system.rs` runs the server as SYSTEM and the app's own check as a user.
//!
//! Elevated, from CI's elevated step only (`CLAUSEO_ELEVATED_TESTS=1`): it makes standard
//! accounts (`ClauseoSbTT` and six hex digits), which run PowerShell through the Secondary Logon
//! service, unelevated. Everything is removed on drop, a failed assertion included.

use std::path::Path;
use std::time::Duration;

use super::tests::{FakeHost, SET_UP};
use super::windows_pipe::WindowsPipe;
use super::{
    ClientFacts, Pipe, Refusal, Reply, ServeExit, SetUpError, TOKEN_BYTES, Unmade, pipe_name, run,
};
use crate::test_support::TempDir;
use crate::windows::refresh::windows_tests::{system32, try_in_cmd};
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::result_file::SetupResult;
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::setup_task::ask;
use crate::windows::test_dacl::{elevated, enable_restore, protect_with};

mod system;

/// The session the test pipe reports every client in.
const SESSION: u32 = 7;

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

    fn next(&mut self, wait: Duration) -> Result<Option<Self::Client>, Unmade> {
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

/// The PowerShell client: the pipe `short` opened with the client rights alone, at
/// identification level; a set-up request; the reply line printed.
fn ask_script(short: &str) -> String {
    format!(
        "$rights = [System.IO.Pipes.PipeAccessRights]0x{rights:x}\r\n\
         $p = New-Object System.IO.Pipes.NamedPipeClientStream('.', '{short}', $rights, \
         [System.IO.Pipes.PipeOptions]::None, \
         [System.Security.Principal.TokenImpersonationLevel]::Identification, \
         [System.IO.HandleInheritability]::None)\r\n\
         $p.Connect(60000)\r\n\
         $b = [System.Text.Encoding]::UTF8.GetBytes('{{\"v\":1,\"op\":\"set_up\"}}')\r\n\
         $p.Write($b, 0, $b.Length)\r\n\
         $p.Flush()\r\n\
         $r = New-Object System.IO.StreamReader($p)\r\n\
         Write-Output (\"reply: \" + $r.ReadLine())\r\n",
        rights = crate::windows::pipe::PIPE_RIGHTS
    )
}

/// `reply` as the line a client reads, its newline trimmed.
fn line(reply: &Reply) -> String {
    String::from_utf8(reply.to_line())
        .unwrap()
        .trim_end()
        .to_owned()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_pipe_server_refuses_a_user_who_isnt_the_sessions_user() {
    if !gate("elevated_the_pipe_server_refuses_a_user_who_isnt_the_sessions_user") {
        return;
    }
    clear_leftovers(TASK_TEST_PREFIX);
    let refused = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let served = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let dir = TempDir::new("set-up-pipe");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})(A;OICI;FA;;;{})",
        refused.sid.as_str(),
        served.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let (refused_dir, served_dir) = (dir.dir("refused"), dir.dir("served"));
    // A first logon makes the profile, which may take longer than the server's first wait.
    for (user, folder) in [(&refused, &refused_dir), (&served, &served_dir)] {
        let (code, printed) = try_in_cmd(user, folder, "echo ready");
        assert_eq!(code, Some(0), "{printed}");
    }
    let short = format!("ClauseoTestSetUp{}", std::process::id());
    let name = format!(r"\\.\pipe\{short}");
    let script = dir.file("ask.ps1", &ask_script(&short));
    let powershell = format!(
        r#""{}\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{}""#,
        system32(),
        script.display()
    );
    let mut host = FakeHost::new();
    host.sessions = vec![(SESSION, served.sid.clone())];
    host.profiles = vec![(served.sid.clone(), dir.path().join("profile"))];
    enable_restore().unwrap();
    let mut seen = Vec::new();
    let mut made = None;
    let ask_as = |user: &'static str, who: &TestUser, folder: &Path| {
        let (code, printed) = try_in_cmd(who, folder, &powershell);
        eprintln!("the {user} user's client: exit {code:?}\n{printed}");
        printed
    };
    let (exit, as_refused, as_served) = std::thread::scope(|scope| {
        let first = scope.spawn(|| ask_as("refused", &refused, &refused_dir));
        let second = scope.spawn(|| ask_as("served", &served, &served_dir));
        let exit = run(&mut host, |_name, sddl| {
            made = Some(sddl.to_owned());
            WindowsPipe::create(&name, sddl).map(|pipe| Seen(pipe, &mut seen))
        });
        (exit, first.join().unwrap(), second.join().unwrap())
    });
    eprintln!(
        "the descriptor: {made:?}\nthe server's lines: {:?}",
        host.lines
    );
    let made = made.unwrap();
    assert!(
        made.starts_with("O:SYD:P(D;;GA;;;"),
        "the production one: {made}"
    );
    assert!(made.ends_with("S:(ML;;NWNR;;;ME)"), "{made}");
    assert_eq!(exit, ServeExit::Done);
    // The tokens as read at identification level: each user's own, unelevated.
    assert_eq!(seen.len(), 2, "{seen:?}");
    let facts = seen.iter().find(|facts| facts.user == refused.sid).unwrap();
    assert_eq!(facts.integrity, 0x2000, "Medium");
    assert!(!facts.restricted);
    let holds_iu = facts.groups.iter().any(|group| group.as_str() == "S-1-5-4");
    assert!(holds_iu, "IU, which the descriptor lets in");
    assert!(seen.iter().any(|facts| facts.user == served.sid));
    let refusal = line(&Reply::Refused(Refusal::NotSessionUser.to_string()));
    assert!(as_refused.contains(&refusal), "{as_refused}");
    let done = line(&Reply::Ran(SetupResult::done()));
    assert!(as_served.contains(&done), "{as_served}");
    assert!(
        host.calls
            .iter()
            .all(|call| !call.contains(refused.sid.as_str())),
        "nothing was read or run for the refused user: {:?}",
        host.calls
    );
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_squatted_pipe_name_is_served_to_no_one_and_the_app_sends_it_nothing() {
    if !gate("elevated_a_squatted_pipe_name_is_served_to_no_one_and_the_app_sends_it_nothing") {
        return;
    }
    // A published name someone else holds: a random name can't be made first, so the test
    // makes its own pipe under the name it then publishes, as a squat would have to be.
    let token = random_hex(TOKEN_BYTES);
    let name = pipe_name(&token).unwrap();
    // The squatter, first, as a pool user's process would be: its own owner (this process's,
    // Administrators), everyone let in.
    let mut squatter = WindowsPipe::create(&name, "D:(A;;GA;;;WD)").unwrap();
    enable_restore().unwrap();
    let mut host = FakeHost::new();
    let exit = run(&mut host, |_name, sddl| WindowsPipe::create(&name, sddl));
    eprintln!("the server's lines: {:?}", host.lines);
    assert_eq!(exit, ServeExit::Squatted);
    assert_eq!(exit.as_code(), 64);
    assert!(host.acted().is_empty(), "no one served: {:?}", host.calls);
    // The app's own client, against the squatter: refused before a byte is written.
    let exe = Path::new(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe");
    let (asked, sent) = std::thread::scope(|scope| {
        // Nothing published before the start, then the squatter's name: one this start made.
        let mut reads = 0;
        let published = move || {
            reads += 1;
            Ok((reads > 1).then(|| token.clone()))
        };
        let app = scope.spawn(|| ask(published, SET_UP.as_bytes(), exe, || Ok(())));
        let mut client = squatter
            .next(Duration::from_secs(30))
            .unwrap()
            .expect("the app connects");
        let sent = squatter.request(&mut client);
        (app.join().unwrap(), sent)
    });
    eprintln!("the app: {asked:?}\nthe squatter read: {sent:?}");
    // Its owner (Administrators) is no pool user's: a squat at once, with no pool stop.
    let Err(SetUpError::Squatted(why)) = &asked else {
        panic!("{asked:?}");
    };
    assert!(why.contains("isn't owned by LOCAL SYSTEM"), "{why}");
    assert!(sent.is_err(), "the squatter got nothing: {sent:?}");
}
