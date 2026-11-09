//! The jailed launch's policy on its other branches (R17 PR-D2a's review): the browser's,
//! the case-law relay's and the skill packs' grants pinned whole, with the negative golden; the
//! runtime refusals (Windows' known folders, any matter of the registry); the files over the
//! checkpoint size cap; and Codex's login link only for Clauseo's own login (ruling (a)).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::ErrorCode;
use clauseo_sandbox::PathRule;

use super::super::super::super::egress::EgressGuard;
use super::super::super::super::relay::tests::rig;
use super::super::super::super::state_prep::is_lawyers_login;
use super::super::{jail, personal_folders, runtime};
use super::Fixture;
use crate::agent::state_folder::{self, LOGIN_FILE, StateLink};
use crate::config::{AgentListener, EgressProxy, SessionKey};
use crate::write;

/// The browser listener's port the stand-in proxy answers.
const BROWSER_PORT: u16 = 47_011;
/// Its agent listener's port.
const AGENT_PORT: u16 = 47_012;

/// A proxy stand-in with fixed ports; opens and closes nothing.
struct Ports;

impl EgressProxy for Ports {
    fn open(
        &self,
        _session: SessionKey,
        _config: clauseo_sandbox::ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error> {
        Ok(())
    }

    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(BROWSER_PORT)
    }

    fn agent_listener(&self, _session: SessionKey) -> Option<AgentListener> {
        let url = OsString::from(format!("http://clauseo:t@127.0.0.1:{AGENT_PORT}"));
        Some(AgentListener::new(
            AGENT_PORT,
            vec![("HTTPS_PROXY".into(), Some(url))],
        ))
    }

    fn close(&self, _session: SessionKey) {}
}

/// An empty file at `path`, its folder made.
fn file(path: &Path) {
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(path, b"").unwrap();
}

#[test]
fn the_browser_relay_and_skill_packs_add_exactly_their_grants_and_nothing_else() {
    let fx = Fixture::new("branches");
    let (mut prepared, mut grants) = fx.claude();
    let sidecars = [
        fx.root.join("sidecars").join("mcp").join("clauseo-mcp"),
        fx.root.join("sidecars").join("ab").join("agent-browser"),
    ];
    for sidecar in &sidecars {
        file(sidecar);
    }
    grants.sidecars = sidecars.to_vec();
    let chrome = fx.root.join("chrome").join("chrome");
    file(&chrome);
    let config = fx
        .data
        .join(crate::browser::CONFIG_DIR)
        .join(crate::browser::CONFIG_FILE);
    file(&config);
    let run = &mut prepared.run;
    run.chrome = Some(chrome);
    #[cfg(unix)]
    let sockets = fx.root.join("sockets");
    #[cfg(unix)]
    write::create_dir_all(&sockets).unwrap();
    let session = crate::browser::session::BrowserSession::claim(
        "branches",
        #[cfg(unix)]
        Some(&sockets),
    )
    .unwrap();
    let mut loopback = session.ports.loopback();
    loopback.sort_unstable();
    #[cfg(unix)]
    let socket = fx.rel(session.socket_dir.path());
    #[cfg(not(unix))]
    let socket = "-".to_owned();
    run.browser_session = Some(Arc::new(session));
    run.egress = Some(Arc::new(EgressGuard::for_test(Arc::new(Ports), run.key)));
    let relays = rig();
    relays.sign_in();
    run.case_law = relays.prep(true, false).open(run);
    let relay_port = run.case_law.relay().expect("the relay opened").port();
    run.packs_written = Some(Arc::new(
        clauseo_profile::write_packs(&run.skill_packs, &[]).unwrap(),
    ));

    let jail = jail(&prepared, &grants, &[46_101]).unwrap();
    let session = Fixture::key().id();
    let store = "data/agents/claude-acp/home";
    let mut ports = vec![46_101, AGENT_PORT, BROWSER_PORT, relay_port];
    ports.sort_unstable();
    let expected = format!(
        "matter: home/Documents/Clauseo/m1\n\
         state: data/agent-state/{m}/claude-acp\n\
         tmpdir: data/run/{session}/tmp\n\
         notes: data/agent-state/{m}/notes\n\
         read_only_in_state: data/agent-state/{m}/claude-acp/settings.json\n\
         credential_link: -\n\
         claude_store: {store}/.credentials.json, {store}/.storage-write / \
         {store}/.storage-write.lock, {store}/.oauth_refresh.lock, {store}.lock\n\
         runtime: node/bin, data/agents/agent/install/h1, stetdoc, sidecars/mcp, sidecars/ab, \
         chrome\n\
         read: data/run/{session}/ca-bundle.pem, data/profile, data/run/{session}/skill-packs, \
         data/run/{session}/relay-token, data/browser/agent-browser.json\n\
         allowed_folders: \n\
         hidden: data, home/Documents/Clauseo/m2, home/Documents/Clauseo/m3\n\
         proxy_ports: {ports:?}\n\
         loopback: Ports({loopback:?})\n\
         browser_socket_dir: {socket}\n\
         keychain: false\n\
         native_code_deny: false\n",
        m = Fixture::key().matter,
    );
    assert_eq!(fx.shown(&jail.policy), expected);
    fx.assert_absent(&jail.policy);
}

#[test]
fn a_runtime_folder_in_a_redirected_documents_folder_or_any_matter_refuses_the_launch() {
    let fx = Fixture::new("redirected");
    let (prepared, mut grants) = fx.codex();
    // Windows' known Documents, moved by OneDrive out of the home folder: refused all the same.
    let moved = fx.root.join("OneDrive").join("Documents");
    let install = moved.join("agent");
    write::create_dir_all(&install).unwrap();
    grants.personal = Arc::new(move |_home: Option<&Path>| vec![moved.clone()]);
    grants.install = Some(install);
    let text = runtime(&grants, &prepared).unwrap_err().to_string();
    assert!(text.contains("Documents, Desktop or"), "{text}");
    assert!(!text.contains(&*fx.root.to_string_lossy()), "{text}");
    // In another matter of the registry, or this one: refused, the path never named (the
    // fixture's matters sit in its Documents, so that refusal is set aside here).
    grants.personal = Arc::new(|_home: Option<&Path>| Vec::new());
    for matter in [fx.others[0].clone(), fx.matter.clone()] {
        let install = matter.join("tools");
        write::create_dir_all(&install).unwrap();
        grants.install = Some(install);
        let error = runtime(&grants, &prepared).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
        let text = error.to_string();
        assert!(text.contains("a matter's folder"), "{text}");
        assert!(!text.contains(&*fx.root.to_string_lossy()), "{text}");
    }
    grants.install = None;
    assert!(runtime(&grants, &prepared).is_ok());
    // Off Windows the home's names; on Windows the known folders too.
    let home = personal_folders(Some(&fx.home));
    for name in ["Documents", "Desktop", "Downloads"] {
        assert!(home.contains(&fx.home.join(name)), "{name}");
    }
    #[cfg(windows)]
    {
        use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
        for folder in [KnownFolder::Desktop, KnownFolder::Downloads] {
            if let Some(known) = SystemDirs.known_folder(folder) {
                assert!(personal_folders(None).contains(&known), "{folder:?}");
            }
        }
    }
}

#[test]
fn files_over_the_size_cap_are_read_only_in_the_matter() {
    let fx = Fixture::new("over-cap");
    let (prepared, mut grants) = fx.codex();
    grants.file_cap = 4;
    let big = fx.matter.join("film").join("big.mov");
    write::create_dir_all(big.parent().unwrap()).unwrap();
    write::write_file(&big, b"0123456789").unwrap();
    write::write_file(&fx.matter.join("small.txt"), b"ok").unwrap();
    let in_git = fx.matter.join(".git").join("pack");
    write::create_dir_all(in_git.parent().unwrap()).unwrap();
    write::write_file(&in_git, b"0123456789").unwrap();
    let policy = jail(&prepared, &grants, &[]).unwrap().policy;
    assert_eq!(policy.over_cap(), std::slice::from_ref(&big));
    assert!(
        policy
            .read_only_in_matter()
            .iter()
            .any(|rule| *rule == PathRule::Literal(big.clone()))
    );
    // With the real cap nothing in the fixture is over it.
    grants.file_cap = clauseo_matters::FILE_CAP_BYTES;
    let policy = jail(&prepared, &grants, &[]).unwrap().policy;
    assert_eq!(policy.over_cap(), Vec::<PathBuf>::new());
}

/// A change to a Codex launch's prep or grants.
type Case<'a> = dyn Fn(&mut super::super::Prepared, &mut super::super::Grants) + 'a;

#[test]
fn codex_s_login_link_is_granted_only_for_clauseo_s_own_login() {
    let fx = Fixture::new("codex-login");
    let (mut prepared, mut grants) = fx.codex();
    let state = prepared.codex.as_ref().unwrap().state.clone();
    let granted = jail(&prepared, &grants, &[]).unwrap().policy;
    assert_eq!(granted.credential_link(), Some(&*state.join(LOGIN_FILE)));
    // Reused, the lawyer's own file by identity, or a home that is not Clauseo's: no link, and
    // nothing under the lawyer's ~/.codex (the negative golden's credential stores).
    let lawyers_home = fx.home.join(".codex");
    let cases: [&Case<'_>; 3] = [
        &|_, grants| grants.reused = true,
        &|prepared, _| prepared.codex.as_mut().unwrap().lawyers_login = true,
        &|prepared, _| prepared.codex.as_mut().unwrap().home = lawyers_home.clone(),
    ];
    for case in cases {
        let (mut p, mut g) = fx.codex();
        case(&mut p, &mut g);
        let policy = jail(&p, &g, &[]).unwrap().policy;
        assert!(policy.credential_link().is_none());
        fx.assert_absent(&policy);
    }
    prepared.codex.as_mut().unwrap().link = Some(Ok(StateLink::Same));
    grants.reused = false;
    assert!(
        jail(&prepared, &grants, &[])
            .unwrap()
            .policy
            .credential_link()
            .is_some()
    );
}

#[test]
fn a_linked_file_that_is_the_lawyer_s_own_by_identity_is_reused_whatever_its_link_count() {
    let fx = Fixture::new("identity");
    let (prepared, _) = fx.codex();
    let mut codex = prepared.codex.unwrap();
    let lawyers = fx.home.join(".codex").join(LOGIN_FILE);
    write::write_file(&lawyers, b"{\"lawyer\":1}").unwrap();
    let home = fx.codex_home();
    write::create_dir_all(&home).unwrap();
    // Clauseo's login home holds a name of the lawyer's file: linked into the matter, it is the
    // lawyer's login by identity.
    write::hard_link(&lawyers, &home.join(LOGIN_FILE)).unwrap();
    codex.link = Some(state_folder::link_own(&home, &codex.state).map_err(|e| e.refusal()));
    assert!(matches!(codex.link, Some(Ok(StateLink::Linked))));
    assert!(is_lawyers_login(&codex, std::slice::from_ref(&lawyers)));
    // Clauseo's own file with a name in three more matters: not the lawyer's.
    write::remove_link(&home.join(LOGIN_FILE)).unwrap();
    write::write_file(&home.join(LOGIN_FILE), b"{\"own\":1}").unwrap();
    for matter in ["a", "b", "c"] {
        let other = fx.data.join("agent-state").join(matter);
        write::create_dir_all(&other).unwrap();
        write::hard_link(&home.join(LOGIN_FILE), &other.join(LOGIN_FILE)).unwrap();
    }
    codex.link = Some(state_folder::link_own(&home, &codex.state).map_err(|e| e.refusal()));
    assert!(matches!(codex.link, Some(Ok(StateLink::Linked))));
    assert!(!is_lawyers_login(&codex, std::slice::from_ref(&lawyers)));
    // Not linked: nothing to compare.
    codex.link = Some(Ok(StateLink::NoLogin));
    assert!(!is_lawyers_login(&codex, &[lawyers]));
    // The lawyer's file is untouched.
    let kept: PathBuf = fx.home.join(".codex").join(LOGIN_FILE);
    assert_eq!(std::fs::read(kept).unwrap(), b"{\"lawyer\":1}");
}
