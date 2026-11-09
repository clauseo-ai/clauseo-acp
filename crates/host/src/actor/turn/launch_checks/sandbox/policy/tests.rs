//! The jailed launch's policy on fixture folders (R17 PR-D): a golden per agent (every grant,
//! the hidden list and the ports, paths relative to the fixture root), and the negative goldens
//! the orchestrator asked for: what no agent's policy grants in any form (read, write, runtime,
//! allowed folder), so a widening by mistake fails a test as a narrowing does.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{AgentKind, MatterId, ThreadId};
use clauseo_sandbox::Policy;

use super::super::super::{CodexPrepared, Prepared, RunFolders};
use super::{Grants, NATIVE_CODE_DENY, jail, personal_folders, policy, runtime};
use crate::agent::state_folder::{StateLink, state_dir};
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::write;

/// The credential stores and personal folders under the lawyer's home no policy may touch.
const CREDENTIAL_STORES: [&str; 10] = [
    ".ssh",
    ".aws",
    ".config/gcloud",
    ".azure",
    ".docker/config.json",
    ".netrc",
    ".gnupg",
    ".codex",
    ".claude",
    "Library/Keychains",
];

/// The fixed root the fixture's CA bundle holds.
const ROOT_DER: [u8; 3] = [0x30, 0x01, 0x00];

/// A fixture: a home with credential stores and three matters, a data folder with the session's
/// folders, the agents' login homes and an install, and a Node outside both.
pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
    pub(super) data: PathBuf,
    pub(super) matter: PathBuf,
    pub(super) others: Vec<PathBuf>,
    node: PathBuf,
    install: PathBuf,
}

impl Fixture {
    pub(super) fn new(test: &str) -> Self {
        let raw = std::env::temp_dir().join(format!("clauseo-d2a-{test}-{}", std::process::id()));
        write::remove_dir_all(&raw).unwrap();
        write::create_dir_all(&raw).unwrap();
        let root = dunce::canonicalize(&raw).unwrap();
        let home = root.join("home");
        let data = root.join("data");
        let matters = home.join("Documents").join("Clauseo");
        let matter = matters.join("m1");
        let others = vec![matters.join("m2"), matters.join("m3")];
        let node = root.join("node").join("bin").join("node");
        let install = data.join("agents").join("agent").join("install").join("h1");
        for folder in [
            &matter,
            &others[0],
            &others[1],
            &install,
            &data.join("profile"),
        ] {
            write::create_dir_all(folder).unwrap();
        }
        write::create_dir_all(node.parent().unwrap()).unwrap();
        write::write_file(&node, b"").unwrap();
        for store in CREDENTIAL_STORES {
            let path = home.join(store);
            if Path::new(store).extension().is_some() || store.ends_with("netrc") {
                write::create_dir_all(path.parent().unwrap()).unwrap();
                write::write_file(&path, b"secret").unwrap();
            } else {
                write::create_dir_all(&path).unwrap();
            }
        }
        for folder in ["Desktop", "Downloads"] {
            write::create_dir_all(&home.join(folder)).unwrap();
        }
        Self {
            root,
            home,
            data,
            matter,
            others,
            node,
            install,
        }
    }

    pub(super) fn key() -> SessionKey {
        SessionKey {
            matter: id::<MatterId>(1),
            thread: id::<ThreadId>(2),
            launch: 3,
        }
    }

    /// stetdoc's folder, made.
    fn stetdoc(&self) -> PathBuf {
        let folder = self.root.join("stetdoc");
        write::create_dir_all(&folder).unwrap();
        folder
    }

    /// The session's folders for `agent` (made), its state folder when it has a kind.
    pub(super) fn run(&self, agent: &str, kind: Option<AgentKind>) -> RunFolders {
        let key = Self::key();
        let state = kind.map(|_| state_dir(&self.data, key.matter, agent));
        let run = RunFolders::new(&self.data, key).with_state(state);
        let made = [&run.work, &run.browser, &run.notes, &run.tmp()];
        for folder in made.into_iter().chain(run.state.as_ref()) {
            write::create_dir_all(folder).unwrap();
        }
        run
    }

    pub(super) fn grants(&self, kind: Option<AgentKind>) -> Grants {
        Grants {
            data: self.data.clone(),
            kind,
            command: self.node.clone(),
            install: Some(self.install.clone()),
            stetdoc: Some(self.stetdoc()),
            others: self.others.clone(),
            skills: vec![self.data.join("profile"), self.data.join("no-such-skills")],
            sidecars: Vec::new(),
            reused: false,
            read_only: false,
            ca_cert: None,
            roots: Some(Arc::new(|| vec![ROOT_DER.to_vec()])),
            home: Some(self.home.clone()),
            agent_homes: Vec::new(),
            personal: Arc::new(personal_folders),
            file_cap: clauseo_matters::FILE_CAP_BYTES,
            login_home: self.codex_home(),
        }
    }

    /// Clauseo's own Codex login home.
    pub(super) fn codex_home(&self) -> PathBuf {
        self.data.join("agents").join("codex-acp").join("home")
    }

    pub(super) fn prepared(&self, run: RunFolders) -> Prepared {
        Prepared {
            stored: self.matter.clone(),
            codex: None,
            claude_home: None,
            folder: None,
            run,
            skills_written: None,
            packs_skipped: Vec::new(),
            sandbox: None,
            reused: false,
        }
    }

    pub(super) fn codex(&self) -> (Prepared, Grants) {
        let run = self.run("codex-acp", Some(AgentKind::Codex));
        let state = run.state.clone().unwrap();
        let mut prepared = self.prepared(run);
        prepared.codex = Some(CodexPrepared {
            folder: self.matter.clone(),
            home: self.codex_home(),
            state,
            keys: serde_json::Map::new(),
            login: None,
            link: Some(Ok(StateLink::Linked)),
            lawyers_login: false,
            state_denies: Vec::new(),
        });
        (prepared, self.grants(Some(AgentKind::Codex)))
    }

    pub(super) fn claude(&self) -> (Prepared, Grants) {
        let run = self.run("claude-acp", Some(AgentKind::Claude));
        let home = self.data.join("agents").join("claude-acp").join("home");
        write::create_dir_all(&home).unwrap();
        let mut prepared = self.prepared(run);
        prepared.claude_home = Some(home);
        (prepared, self.grants(Some(AgentKind::Claude)))
    }

    /// `path` relative to the fixture's root, `/`-separated.
    pub(super) fn rel(&self, path: &Path) -> String {
        let rest = path.strip_prefix(&self.root).unwrap_or(path);
        rest.iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// The policy as text: every grant, the hidden list, the ports and the pinned switches.
    pub(super) fn shown(&self, policy: &Policy) -> String {
        let list = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| self.rel(path))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut text = String::new();
        let one = |path: Option<&Path>| path.map_or_else(|| "-".to_owned(), |path| self.rel(path));
        let store = policy.claude_store().map_or_else(
            || "-".to_owned(),
            |store| format!("{} / {}", list(&store.files()), list(&store.lock_folders())),
        );
        for (name, value) in [
            ("matter", self.rel(policy.matter())),
            ("state", self.rel(policy.state())),
            ("tmpdir", one(policy.session_tmpdir())),
            ("notes", one(policy.notes())),
            ("read_only_in_state", list(policy.read_only_in_state())),
            ("credential_link", one(policy.credential_link())),
            ("claude_store", store),
            ("runtime", list(policy.runtime())),
            ("read", list(policy.read())),
            ("allowed_folders", list(policy.allowed_folders())),
            ("hidden", list(policy.hidden())),
            ("proxy_ports", format!("{:?}", policy.proxy_ports())),
            ("loopback", format!("{:?}", policy.loopback())),
            ("browser_socket_dir", one(policy.browser_socket_dir())),
            ("keychain", policy.keychain().to_string()),
            ("native_code_deny", NATIVE_CODE_DENY.to_string()),
        ] {
            writeln!(text, "{name}: {value}").unwrap();
        }
        text
    }

    /// Every path the policy grants in any form, with what it is.
    pub(super) fn granted(policy: &Policy) -> Vec<(&'static str, PathBuf)> {
        let mut granted = vec![
            ("matter", policy.matter().to_owned()),
            ("state", policy.state().to_owned()),
        ];
        let some = [
            ("tmpdir", policy.session_tmpdir()),
            ("notes", policy.notes()),
            ("credential link", policy.credential_link()),
            ("browser socket folder", policy.browser_socket_dir()),
        ];
        for (what, path) in some {
            granted.extend(path.map(|path| (what, path.to_owned())));
        }
        for (what, paths) in [
            ("runtime", policy.runtime()),
            ("read", policy.read()),
            ("allowed folder", policy.allowed_folders()),
            ("read-only in state", policy.read_only_in_state()),
        ] {
            granted.extend(paths.iter().map(|path| (what, path.clone())));
        }
        if let Some(store) = policy.claude_store() {
            let files = store.files().into_iter().chain(store.lock_folders());
            granted.extend(files.map(|path| ("Claude store", path)));
        }
        granted
    }

    /// The negative golden: what `policy` must not grant in any form.
    pub(super) fn assert_absent(&self, policy: &Policy) {
        assert!(!policy.keychain(), "no Keychain rule in any policy");
        const {
            assert!(
                !NATIVE_CODE_DENY,
                "pinned off until stage D's Python runtime"
            );
        }
        assert!(
            policy.allowed_folders().is_empty(),
            "R18 adds allowed folders"
        );
        let state = policy.state();
        // The session's own files in its run folder, each by name: no other run folder's.
        let run = self.data.join("run").join(Fixture::key().id());
        let own_in_data = |path: &Path| {
            path.starts_with(state)
                || policy
                    .session_tmpdir()
                    .is_some_and(|tmp| path.starts_with(tmp))
                || policy.notes().is_some_and(|notes| path.starts_with(notes))
                || path.starts_with(&self.install)
                || path.starts_with(self.data.join("profile"))
                || [
                    run.join(super::super::ca::CA_FILE),
                    run.join("relay-token"),
                    self.data
                        .join(crate::browser::CONFIG_DIR)
                        .join(crate::browser::CONFIG_FILE),
                ]
                .contains(&path.to_path_buf())
                || path.starts_with(run.join("skill-packs"))
                || policy.claude_store().is_some_and(|store| {
                    store.files().iter().any(|file| file == path)
                        || store
                            .lock_folders()
                            .iter()
                            .any(|lock| path.starts_with(lock))
                })
        };
        for (what, path) in Self::granted(policy) {
            let shown = self.rel(&path);
            // The home folder: never it nor a folder above it; inside it only the matter.
            assert!(
                !self.home.starts_with(&path),
                "{what} {shown} holds the home folder"
            );
            if path.starts_with(&self.home) {
                assert!(
                    path.starts_with(&self.matter),
                    "{what} {shown} is in the home folder"
                );
            }
            // Every other matter, open or not.
            for other in &self.others {
                assert!(
                    !path.starts_with(other) && !other.starts_with(&path),
                    "{what} {shown} reaches another matter"
                );
            }
            // The data folder beyond the session's own parts.
            assert!(
                !self.data.starts_with(&path),
                "{what} {shown} holds the data folder"
            );
            if path.starts_with(&self.data) {
                assert!(own_in_data(&path), "{what} {shown} is in the data folder");
            }
            // Credential stores and the login Keychain.
            for store in CREDENTIAL_STORES {
                let store = self.home.join(store);
                assert!(
                    !path.starts_with(&store) && !store.starts_with(&path),
                    "{what} {shown} reaches a credential store"
                );
            }
            // Network paths, in every spelling.
            let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
            for network in ["//", "/volumes/", "/net/", "smb:", "nfs:", "afp:", "cifs:"] {
                assert!(
                    !text.starts_with(network),
                    "{what} {shown} is a network path"
                );
            }
        }
        // The other matters and the data folder are hidden.
        for hidden in self.others.iter().chain([&self.data]) {
            assert!(
                policy.hidden().contains(hidden),
                "{} not hidden",
                self.rel(hidden)
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _gone = write::remove_dir_all(&self.root);
    }
}

#[test]
fn codex_s_policy_golden_and_what_it_never_grants() {
    let fx = Fixture::new("codex");
    let (prepared, grants) = fx.codex();
    let jail = jail(&prepared, &grants, &[46_101, 46_102]).unwrap();
    let session = Fixture::key().id();
    let expected = format!(
        "matter: home/Documents/Clauseo/m1\n\
         state: data/agent-state/{m}/codex-acp\n\
         tmpdir: data/run/{session}/tmp\n\
         notes: data/agent-state/{m}/notes\n\
         read_only_in_state: data/agent-state/{m}/codex-acp/config.toml\n\
         credential_link: data/agent-state/{m}/codex-acp/auth.json\n\
         claude_store: -\n\
         runtime: node/bin, data/agents/agent/install/h1, stetdoc\n\
         read: data/run/{session}/ca-bundle.pem, data/profile\n\
         allowed_folders: \n\
         hidden: data, home/Documents/Clauseo/m2, home/Documents/Clauseo/m3\n\
         proxy_ports: [46101, 46102]\n\
         loopback: None\n\
         browser_socket_dir: -\n\
         keychain: false\n\
         native_code_deny: false\n",
        m = Fixture::key().matter,
    );
    assert_eq!(fx.shown(&jail.policy), expected);
    fx.assert_absent(&jail.policy);
    // `PATH` from the runtime (and Windows' folder), then the CA bundle's variables, at the
    // bundle, and nothing else without a proxy.
    let host = crate::agent::jailed_env::host_vars(jail.policy.runtime());
    let (host_part, ca_part) = jail.env.split_at(host.len());
    assert_eq!(host_part, &host[..]);
    let bundle = prepared.run.root.join(super::super::ca::CA_FILE);
    let names: Vec<_> = ca_part.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(
        names,
        super::super::ca::CA_VARS.map(std::ffi::OsString::from)
    );
    assert!(
        ca_part
            .iter()
            .all(|(_, value)| value.as_deref() == Some(bundle.as_os_str()))
    );
    let pem = std::fs::read_to_string(&bundle).unwrap();
    assert_eq!(
        pem,
        "-----BEGIN CERTIFICATE-----\nMAEA\n-----END CERTIFICATE-----\n"
    );
}

#[test]
fn claude_s_policy_golden_grants_only_its_store_s_files_and_no_keychain() {
    let fx = Fixture::new("claude");
    let (prepared, grants) = fx.claude();
    let jail = jail(&prepared, &grants, &[]).unwrap();
    let session = Fixture::key().id();
    let store = "data/agents/claude-acp/home";
    let expected = format!(
        "matter: home/Documents/Clauseo/m1\n\
         state: data/agent-state/{m}/claude-acp\n\
         tmpdir: data/run/{session}/tmp\n\
         notes: data/agent-state/{m}/notes\n\
         read_only_in_state: data/agent-state/{m}/claude-acp/settings.json\n\
         credential_link: -\n\
         claude_store: {store}/.credentials.json, {store}/.storage-write / \
         {store}/.storage-write.lock, {store}/.oauth_refresh.lock, {store}.lock\n\
         runtime: node/bin, data/agents/agent/install/h1, stetdoc\n\
         read: data/run/{session}/ca-bundle.pem, data/profile\n\
         allowed_folders: \n\
         hidden: data, home/Documents/Clauseo/m2, home/Documents/Clauseo/m3\n\
         proxy_ports: []\n\
         loopback: None\n\
         browser_socket_dir: -\n\
         keychain: false\n\
         native_code_deny: false\n",
        m = Fixture::key().matter,
    );
    assert_eq!(fx.shown(&jail.policy), expected);
    fx.assert_absent(&jail.policy);
}

#[test]
fn a_reused_claude_login_gets_no_store_and_a_codex_without_its_link_no_credential() {
    let fx = Fixture::new("reused");
    let (prepared, mut grants) = fx.claude();
    grants.reused = true;
    let jailed = jail(&prepared, &grants, &[]).unwrap().policy;
    assert!(
        jailed.claude_store().is_none(),
        "never the lawyer's own store"
    );
    fx.assert_absent(&jailed);
    let (mut prepared, grants) = fx.codex();
    for link in [
        Some(Ok(StateLink::NoLogin)),
        Some(Ok(StateLink::Split)),
        None,
    ] {
        prepared.codex.as_mut().unwrap().link = link;
        let jailed = jail(&prepared, &grants, &[]).unwrap().policy;
        assert!(jailed.credential_link().is_none());
    }
}

#[test]
fn an_agent_without_a_kind_writes_the_session_s_work_folder_and_no_login() {
    let fx = Fixture::new("plain");
    let prepared = fx.prepared(fx.run("custom", None));
    let mut grants = fx.grants(None);
    grants.install = None;
    grants.stetdoc = None;
    let jailed = jail(&prepared, &grants, &[]).unwrap().policy;
    assert_eq!(jailed.state(), prepared.run.work);
    assert_eq!(jailed.read_only_in_state(), Vec::<PathBuf>::new());
    assert!(jailed.credential_link().is_none() && jailed.claude_store().is_none());
    assert_eq!(jailed.runtime(), [fx.root.join("node").join("bin")]);
    fx.assert_absent(&jailed);
}

#[test]
fn a_runtime_folder_in_documents_desktop_or_downloads_refuses_the_launch() {
    let fx = Fixture::new("documents");
    let (prepared, mut grants) = fx.codex();
    for folder in ["Documents", "Desktop", "Downloads"] {
        let install = fx.home.join(folder).join("agent");
        write::create_dir_all(&install).unwrap();
        grants.install = Some(install);
        let error = runtime(&grants, &prepared).unwrap_err();
        assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
        let text = error.to_string();
        assert!(!text.contains(&*fx.root.to_string_lossy()), "{text}");
    }
    // A missing install folder refuses too, without its path.
    grants.install = Some(fx.root.join("gone"));
    let text = runtime(&grants, &prepared).unwrap_err().to_string();
    assert!(!text.contains(&*fx.root.to_string_lossy()), "{text}");
}

#[test]
fn hidden_lists_every_other_registry_matter_but_one_nested_with_this_one() {
    // `policy` alone: `jail` refuses the launch first when one is inside this matter (`branches`).
    let fx = Fixture::new("hidden");
    let (prepared, mut grants) = fx.codex();
    let nested = fx.matter.join("sub");
    write::create_dir_all(&nested).unwrap();
    grants.others.push(nested.clone());
    let runtime = runtime(&grants, &prepared).unwrap();
    let built = policy(&prepared, &grants, runtime, None, &[]).unwrap();
    assert!(!built.hidden().contains(&nested));
    for other in &fx.others {
        assert!(built.hidden().contains(other));
    }
}

/// The read-only mode's negative golden (R17 PR-D2c, ruling 1): Codex's policy with the matter
/// read only, no write anywhere in it, and every other grant as the normal policy's (the state,
/// temp and notes folders keep their writes). Anything that widens it fails here; the profile's
/// and the Windows plan's own goldens are the sandbox crate's.
#[test]
fn codex_s_read_only_policy_writes_nothing_in_the_matter() {
    use clauseo_sandbox::{Access, PathClass};
    let fx = Fixture::new("codex-read-only");
    let (prepared, mut grants) = fx.codex();
    let normal = jail(&prepared, &grants, &[46_101]).unwrap().policy;
    grants.read_only = true;
    let read_only = jail(&prepared, &grants, &[46_101]).unwrap().policy;
    assert!(read_only.matter_read_only());
    assert!(!normal.matter_read_only());
    assert_eq!(fx.shown(&read_only), fx.shown(&normal));
    fx.assert_absent(&read_only);
    let matter = read_only.matter().to_owned();
    for path in [
        matter.clone(),
        matter.join("brief.docx"),
        matter.join("a/b.txt"),
    ] {
        let class = read_only.classify(&path, Access::Write);
        assert_eq!(class, PathClass::ReadOnly, "{path:?}");
        assert_eq!(normal.classify(&path, Access::Write), PathClass::Outside);
    }
    let own = [read_only.session_tmpdir(), read_only.notes()];
    for folder in own.into_iter().flatten().chain([read_only.state()]) {
        let class = read_only.classify(&folder.join("x"), Access::Write);
        assert_eq!(class, PathClass::Outside, "{folder:?}");
    }
}

mod branches;
