//! A jailed Codex and a reused login (R17 PR-D2a, orchestrator's ruling (a)): a login desktop
//! says is reused, or a linked `auth.json` that is the lawyer's own file by identity, is refused
//! before anything is reserved or any jail built, with `agent.auth_required`; a reused one also
//! sends the split with `SplitWhy::CodexSandbox` (desktop's leave ends it as `SignedOut {
//! CodexSandbox }`). No matter's state folder keeps a name of the lawyer's file, and the
//! lawyer's `~/.codex` is never touched. Clauseo's own login is not refused as reused.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::{LoginLink, LoginSplit, SplitWhy};
use clauseo_protocol::{ErrorCode, StopReason, StreamId};

use crate::codex::{CODEX, codex_dir};
use crate::login_check::{Fake as Hook, codex};
use crate::sandbox_rig::{Answer, Fake, failed, start_with};
use crate::support::{Dir, matter_with_thread, wait};

/// The lawyer's own login file, made under the test's home folder.
fn lawyers_login(dir: &Dir) -> PathBuf {
    let file = dir.home().join(".codex").join("auth.json");
    crate::write::write(&file, b"{\"lawyer\":true}");
    file
}

/// Clauseo's Codex login home's `auth.json`, its folder made.
fn clauseo_login(dir: &Dir) -> PathBuf {
    let home = dir.data_dir().join("agents").join(CODEX).join("home");
    crate::write::create_dir_all(&home);
    home.join("auth.json")
}

/// Every `auth.json` under the data folder's `agent-state`.
fn state_logins(dir: &Dir) -> Vec<PathBuf> {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if entry.file_name() == "auth.json" {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(&dir.data_dir().join("agent-state"), &mut found);
    found
}

/// Starts a jailed host with the sign-in hook answering `link`, the test's home as the
/// lawyer's, sends one prompt on a new Codex matter and gives the turn's failure, the seam and
/// the host's split receiver's next split if any.
fn jailed_turn(
    dir: &Dir,
    link: LoginLink,
    reused: bool,
) -> (ErrorCode, Arc<Fake>, Option<LoginSplit>) {
    let (fake, seam_launched) = Fake::new(dir, Answer::Launch(Vec::new()));
    let hook = Hook::new(link);
    let home = dir.home().to_owned();
    let (host, host_launched, _) = start_with(dir, Some(Arc::clone(&fake)), move |config| {
        config.with_login_check(hook).with_home_folder(Some(home))
    });
    let splits = host.handle.login_splits();
    let (_, thread, _) = matter_with_thread(dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), reused)).unwrap();
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let (code, detail) = failed(&events, turn).expect("refused");
    let detail = detail.unwrap_or_default();
    assert!(!detail.contains(&*dir.home().to_string_lossy()), "{detail}");
    let split = splits.try_recv().ok();
    let launched = seam_launched.load(std::sync::atomic::Ordering::SeqCst)
        + host_launched.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(launched, 0, "nothing launched");
    host.shutdown();
    (code, fake, split)
}

#[test]
fn a_reused_login_is_refused_before_the_jail_with_the_codex_sandbox_split() {
    let dir = codex_dir("sandbox_codex_reused", "codex-handshake");
    let lawyers = lawyers_login(&dir);
    // Reused: Clauseo's login home holds a name of the lawyer's file.
    crate::write::hard_link(&lawyers, &clauseo_login(&dir));
    let (code, fake, split) = jailed_turn(&dir, LoginLink::Shared, true);
    assert_eq!(code, ErrorCode::AgentAuthRequired);
    assert_eq!(
        split,
        Some(LoginSplit {
            agent: codex(),
            why: SplitWhy::CodexSandbox,
        })
    );
    assert_eq!(fake.reserved(), 0, "nothing reserved");
    assert!(fake.calls().is_empty(), "no jail built");
    assert_eq!(
        state_logins(&dir),
        Vec::<PathBuf>::new(),
        "no link in the matter"
    );
    assert_eq!(std::fs::read(&lawyers).unwrap(), b"{\"lawyer\":true}");
}

#[test]
fn a_linked_file_that_is_the_lawyer_s_own_by_identity_is_refused_as_reused() {
    let dir = codex_dir("sandbox_codex_identity", "codex-handshake");
    let lawyers = lawyers_login(&dir);
    // Desktop says Clauseo's own login, but its home's file is the lawyer's.
    crate::write::hard_link(&lawyers, &clauseo_login(&dir));
    let (code, fake, split) = jailed_turn(&dir, LoginLink::Own, false);
    assert_eq!(code, ErrorCode::AgentAuthRequired);
    assert_eq!(split, None, "desktop's own login: no leave to run");
    assert_eq!(fake.reserved(), 0);
    assert!(fake.calls().is_empty());
    assert_eq!(
        state_logins(&dir),
        Vec::<PathBuf>::new(),
        "its name was removed"
    );
    assert_eq!(std::fs::read(&lawyers).unwrap(), b"{\"lawyer\":true}");
    assert!(
        clauseo_login(&dir).is_file(),
        "Clauseo's home is left as it was"
    );
}

#[test]
fn clauseo_s_own_login_is_not_refused_as_reused() {
    let dir = codex_dir("sandbox_codex_own", "codex-handshake");
    lawyers_login(&dir);
    crate::write::write(&clauseo_login(&dir), b"{\"own\":true}");
    let (code, _, split) = jailed_turn(&dir, LoginLink::Own, false);
    // Past the login check: refused only by D2a's placeholder for any jailed Codex (D2c starts
    // it in the jailed preset), never as a reused login.
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert_eq!(split, None);
    assert_eq!(
        state_logins(&dir).len(),
        1,
        "its own login linked into the matter"
    );
}
