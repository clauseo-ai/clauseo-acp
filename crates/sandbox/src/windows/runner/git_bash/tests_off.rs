//! The agent's environment with and without Git Bash, the stub's order (`off.rs`) and the
//! `PATH` entries judged by file ID (`path_ids.rs`), on every OS through the fakes.

use std::collections::HashMap;

use super::fake::*;
use super::off::{NO_STUB, OWN_DIR, machine_stub, off_stub, own_dir_sddl};
use super::path_ids::{FileId, FileIds, dropped_entries};
use super::*;
use crate::windows::admin_copy::fake::descriptor;
use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};
use crate::windows::runner::LaunchMessage;
use crate::windows::runner::env::{Bash, GIT_BASH_VAR, GitEnv, agent_env, lawyer_only};
use crate::windows::runner::stub::OFF_VAR;

const STUB_PATH: &str = r"C:\Program Files\Clauseo\Sandbox\git\_off\bash.exe";
const DATA: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo";
const OWN_STUB: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo\sandbox\git-off\bash.exe";
const SPEC_PATH: &str =
    r"C:\Windows\system32;C:\Program Files\Git\cmd;C:\Program Files\Git\mingw64\bin;D:\Tools";

fn launch(path: &str) -> LaunchMessage {
    let state = r"C:\Users\Lawyer\AppData\Local\Clauseo\agent-state\m1\claude";
    LaunchMessage {
        version: crate::windows::runner::LAUNCH_VERSION,
        pool_user: sid(POOL),
        pool_name: "ClauseoSb05".to_owned(),
        matter_sid: None,
        program: r"C:\Program Files\Clauseo\node.exe".to_owned(),
        args: Vec::new(),
        cwd: r"D:\Matters\Smith".to_owned(),
        env: vec![
            ("Path".to_owned(), path.to_owned()),
            (
                GIT_BASH_VAR.to_owned(),
                r"C:\Program Files\Git\bin\bash.exe".to_owned(),
            ),
        ],
        state: state.to_owned(),
        tmpdir: None,
        runtime: Vec::new(),
        granted: vec![state.to_owned()],
        ca_bundle: None,
        credential_link: None,
        firm_ca: Vec::new(),
    }
}

fn get<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn env_of(git: &GitEnv, spec: &LaunchMessage) -> Vec<(String, String)> {
    agent_env(spec, &WinPath::parse(r"C:\Users\Lawyer").unwrap(), git).unwrap()
}

/// The two entries `path_ids` would drop from [`SPEC_PATH`].
fn dropped() -> Vec<String> {
    vec![
        r"C:\Program Files\Git\cmd".to_owned(),
        r"C:\Program Files\Git\mingw64\bin".to_owned(),
    ]
}

fn off(stub: &str) -> GitEnv {
    GitEnv {
        bash: Bash::Off {
            stub: WinPath::parse(stub).unwrap(),
            reason: Off::Squatted.to_string(),
        },
        dropped: dropped(),
    }
}

#[test]
fn with_a_tree_the_agent_s_git_and_bash_are_the_tree_s() {
    let git = GitEnv {
        bash: Bash::Tree(WinPath::parse(&tree()).unwrap()),
        dropped: dropped(),
    };
    let mut spec = launch(SPEC_PATH);
    spec.env.push((OFF_VAR.to_owned(), "the spec's".to_owned()));
    let env = env_of(&git, &spec);
    assert_eq!(
        get(&env, GIT_BASH_VAR),
        Some(format!(r"{}\bin\bash.exe", tree()).as_str())
    );
    assert_eq!(
        get(&env, "PATH"),
        Some(format!(r"{}\cmd;C:\Windows\system32;D:\Tools", tree()).as_str()),
        "the tree's cmd first, the dropped entries gone, the rest in order"
    );
    let paths = env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("path"));
    assert_eq!(paths.count(), 1);
    assert_eq!(get(&env, "GIT_CONFIG_VALUE_0"), Some("openssl"));
    assert_eq!(get(&env, OFF_VAR), None, "the spec's goes");
}

/// The opposite of the old fallback: with Git Bash off the variable is never left unset (Claude
/// Code would look for the lawyer's Git Bash); it names the stub, and the stub says why.
#[test]
fn off_always_names_a_stub_never_nothing() {
    for stub in [STUB_PATH, OWN_STUB] {
        let mut spec = launch(SPEC_PATH);
        spec.env.push((OFF_VAR.to_owned(), "the spec's".to_owned()));
        let env = env_of(&off(stub), &spec);
        assert_eq!(get(&env, GIT_BASH_VAR), Some(stub), "never the lawyer's");
        assert_eq!(get(&env, OFF_VAR), Some(Off::Squatted.to_string().as_str()));
        assert_eq!(get(&env, "PATH"), Some(r"C:\Windows\system32;D:\Tools"));
    }
    let only_git = launch(r"C:\Program Files\Git\cmd");
    let env = env_of(&off(STUB_PATH), &only_git);
    assert_eq!(get(&env, "PATH"), None);
    assert_eq!(get(&env, GIT_BASH_VAR), Some(STUB_PATH));
}

#[test]
fn the_lawyer_s_git_shell_and_ssh_settings_never_reach_the_agent() {
    let mut spec = launch(SPEC_PATH);
    let gone = [
        "GIT_EXEC_PATH",
        "git_ssh_command",
        "GIT_ASKPASS",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_KEY_7",
        "SSH_ASKPASS",
        "SSH_ASKPASS_REQUIRE",
        "EXEPATH",
        "ORIGINAL_PATH",
        "MSYSTEM",
        "MSYSTEM_PREFIX",
        "MINGW_PREFIX",
        "MSYS2_PATH_TYPE",
        "SHELL",
        "BASH_ENV",
        "XDG_CONFIG_HOME",
        "GNUPGHOME",
    ];
    for name in gone {
        assert!(lawyer_only(name), "{name}");
        spec.env
            .push((name.to_owned(), r"C:\Program Files\Git\x".to_owned()));
    }
    spec.env
        .push(("HOME".to_owned(), r"D:\elsewhere".to_owned()));
    spec.env
        .push(("ANTHROPIC_MODEL".to_owned(), "m".to_owned()));
    let env = env_of(&off(STUB_PATH), &spec);
    for name in gone {
        assert_eq!(get(&env, name), None, "{name}");
    }
    assert_eq!(get(&env, "GIT_CONFIG_COUNT"), Some("1"), "the runner's own");
    assert_eq!(get(&env, "GIT_CONFIG_KEY_0"), Some("http.sslBackend"));
    assert_eq!(get(&env, "HOME"), Some(launch("").state.as_str()));
    assert_eq!(get(&env, "ANTHROPIC_MODEL"), Some("m"), "the rest stays");
    assert!(!lawyer_only("PATH") && !lawyer_only("SystemRoot") && !lawyer_only("HTTPS_PROXY"));
}

/// A machine of folders, each with an identity, some of them links.
#[derive(Default)]
struct Ids {
    plain: HashMap<String, (FileId, bool)>,
    roots: HashMap<String, FileId>,
    remote: Vec<String>,
    opened: Vec<String>,
}

impl Ids {
    fn new() -> Ids {
        let mut ids = Ids::default();
        for (path, id) in [
            (r"C:\", 1),
            (r"C:\Program Files", 2),
            (r"C:\Program Files\Git", 3),
            (r"C:\Program Files\Git\cmd", 4),
            (r"C:\Windows", 5),
            (r"C:\Windows\system32", 6),
            (r"D:\", 7),
            (r"D:\Tools", 8),
        ] {
            ids.plain.insert(path.to_owned(), ((1, id), false));
        }
        // A junction to the Git install, and short names of its folders.
        ids.plain.insert(r"D:\G".to_owned(), ((1, 90), true));
        ids.plain.insert(r"D:\G\cmd".to_owned(), ((1, 4), false));
        ids.plain.insert(r"C:\PROGRA~1".to_owned(), ((1, 2), false));
        ids.plain
            .insert(r"C:\PROGRA~1\Git".to_owned(), ((1, 3), false));
        ids.plain
            .insert(r"C:\PROGRA~1\Git\cmd".to_owned(), ((1, 4), false));
        ids.roots.insert(r"C:\Program Files\Git".to_owned(), (1, 3));
        ids.remote.push("N:".to_owned());
        ids
    }
}

impl FileIds for Ids {
    fn remote(&mut self, drive: &str) -> bool {
        self.remote.iter().any(|d| d == drive)
    }

    fn plain_id(&mut self, path: &str) -> Option<(FileId, bool)> {
        self.opened.push(path.to_owned());
        self.plain.get(path).copied()
    }

    fn root_id(&mut self, path: &str) -> Option<FileId> {
        self.opened.push(path.to_owned());
        self.roots.get(path).copied()
    }
}

#[test]
fn path_entries_in_the_lawyer_s_git_go_by_file_id_whatever_their_text() {
    let mut ids = Ids::new();
    let roots = [
        r"C:\Program Files\Git".to_owned(),
        r"\\server\share\Git".to_owned(),
    ];
    let gone = [
        r"C:\Program Files\Git\cmd",
        r"C:\PROGRA~1\Git\cmd",
        r#""C:\PROGRA~1\Git\cmd""#,
        r"C:\Windows\..\Program Files\Git\cmd",
        r"C:\Program Files\Git.\cmd. ",
        r"\\?\C:\Program Files\Git\cmd",
        r"D:\G\cmd",
        r"\\server\share\bin",
        r"\\?\UNC\server\share\bin",
        r"\\.\C:\Program Files\Git\cmd",
        r"N:\bin",
        r"relative\bin",
        "",
    ];
    let kept = [r"C:\Windows\system32", r"D:\Tools", r"D:\Missing"];
    let path: Vec<&str> = gone.iter().chain(kept.iter()).copied().collect();
    let dropped = dropped_entries(&path.join(";"), &roots, &mut ids);
    assert_eq!(dropped, gone.map(str::to_owned).to_vec());
    assert!(
        ids.opened
            .iter()
            .all(|p| !p.starts_with(r"\\") && !p.starts_with("N:")),
        "no network path is ever opened: {:?}",
        ids.opened
    );
}

#[test]
fn the_machine_stub_is_used_once_its_folders_and_file_pass() {
    let mut fake = Fake::new();
    let (stub, passed_over) = off_stub(&mut fake, &sid(LAWYER), DATA).unwrap();
    assert_eq!(stub.display(), STUB_PATH);
    assert_eq!(passed_over, None);
    assert_eq!(fake.stub_levels[0].0, "git");
    assert_eq!(fake.stub_levels[1].0, "_off");
    assert!(
        fake.stub_levels[1].1.contains(GROUP),
        "set-up's exact DACLs"
    );
    assert!(
        fake.secured.is_empty() && fake.copies == 0,
        "the own copy untouched"
    );
}

#[test]
fn a_machine_stub_another_account_can_change_is_refused() {
    for security in [
        descriptor(LAWYER, Some(&[(0, 0, FULL_ACCESS, SYSTEM)]), true),
        descriptor(
            SYSTEM,
            Some(&[(0, 0, FULL_ACCESS, SYSTEM), (0, 0, FULL_ACCESS, GROUP)]),
            true,
        ),
        descriptor(SYSTEM, None, true),
    ] {
        let mut fake = Fake::new();
        fake.stub = Ok(security);
        assert!(machine_stub(&mut fake).is_err());
    }
    let mut fake = Fake::new();
    fake.stub = Ok(descriptor(
        SYSTEM,
        Some(&[(0, 0, READ_EXECUTE, GROUP), (0, 0, FULL_ACCESS, ADMINS)]),
        true,
    ));
    assert!(machine_stub(&mut fake).is_ok(), "read and execute only");
}

#[test]
fn without_the_machine_stub_the_runner_s_own_copy_stands_in() {
    let mut fake = Fake::new();
    fake.stub = Err("_off is a link".to_owned());
    let (stub, passed_over) = off_stub(&mut fake, &sid(LAWYER), DATA).unwrap();
    assert_eq!(stub.display(), OWN_STUB);
    assert_eq!(passed_over.as_deref(), Some("_off is a link"));
    let dir = OWN_STUB.trim_end_matches(r"\bash.exe");
    assert_eq!(
        fake.secured,
        [(dir.to_owned(), own_dir_sddl(&sid(LAWYER), &sid(GROUP)))]
    );
    assert_eq!(fake.copies, 1, "missing: copied");
    assert_eq!(OWN_DIR, ["sandbox", "git-off"]);
    // The same hash next time: no copy; another: copied again before it is used.
    off_stub(&mut fake, &sid(LAWYER), DATA).unwrap();
    assert_eq!(fake.copies, 1);
    fake.files.insert(OWN_STUB.to_owned(), "old".to_owned());
    off_stub(&mut fake, &sid(LAWYER), DATA).unwrap();
    assert_eq!(fake.copies, 2);
}

#[test]
fn the_own_copy_s_folder_is_its_owner_s_and_the_pool_s_to_read_and_run() {
    let sddl = own_dir_sddl(&sid(LAWYER), &sid(GROUP));
    assert_eq!(
        sddl,
        format!("O:{LAWYER}D:P(A;OICI;FA;;;{LAWYER})(A;OICI;0x1200a9;;;{GROUP})")
    );
    assert!(!sddl.contains("WD") && !sddl.contains("BU") && !sddl.contains("AU"));
}

#[test]
fn with_no_stub_that_passes_the_launch_is_refused() {
    let refusals: [fn(&mut Fake); 6] = [
        |f| f.app_data = Ok(r"\\server\profiles\Lawyer\AppData\Local".to_owned()),
        |f| f.app_data = Err("LocalAppData is on the network".to_owned()),
        |f| f.app_data = Ok(r"D:\Elsewhere".to_owned()),
        |f| f.secure = Err("securing it failed (5)".to_owned()),
        |f| f.copy = Err("copying the runner failed (5)".to_owned()),
        |f| f.copy = Ok("changed meanwhile".to_owned()),
    ];
    for refuse in refusals {
        let mut fake = Fake::new();
        fake.stub = Err("the stub isn't there".to_owned());
        refuse(&mut fake);
        let why = off_stub(&mut fake, &sid(LAWYER), DATA).unwrap_err();
        assert!(
            why.starts_with("the machine's stub: the stub isn't there; "),
            "{why}"
        );
    }
    assert_eq!(
        NO_STUB,
        "Git Bash couldn't be turned off safely, so the agent won't start"
    );
}
