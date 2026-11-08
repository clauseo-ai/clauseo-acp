//! The agent's environment with and without Git Bash (built from an allowlist), and the stub's
//! order (`off.rs`), on every OS through the fake.

use super::fake::*;
use super::off::{NO_STUB, OWN_DIR, machine_stub, off_stub, own_dir_sddl};
use super::*;
use crate::windows::admin_copy::fake::descriptor;
use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};
use crate::windows::runner::LaunchMessage;
use crate::windows::runner::env::{Bash, GIT_BASH_VAR, GitEnv, agent_env, is_base};
use crate::windows::runner::stub::OFF_VAR;

const STUB_PATH: &str = r"C:\Program Files\Clauseo\Sandbox\git\_off\bash.exe";
const DATA: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo";
const OWN_STUB: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo\sandbox\git-off\bash.exe";
const RUNTIME: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo\runtime\node";
const WINDOWS_PATH: [&str; 5] = [
    r"C:\Windows\System32",
    r"C:\Windows",
    r"C:\Windows\System32\Wbem",
    r"C:\Windows\System32\WindowsPowerShell\v1.0",
    r"C:\Windows\System32\OpenSSH",
];

/// A launch whose lawyer's environment has `lawyer` too, and whose host set `host`.
fn launch(lawyer: &[(&str, &str)], host: &[(&str, &str)]) -> LaunchMessage {
    let state = r"C:\Users\Lawyer\AppData\Local\Clauseo\agent-state\m1\claude";
    let pairs = |vars: &[(&str, &str)]| -> Vec<(String, String)> {
        vars.iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    };
    let mut env = pairs(&[
        ("SystemRoot", r"C:\Windows"),
        ("PATHEXT", ".COM;.EXE"),
        ("PROCESSOR_ARCHITECTURE", "AMD64"),
        (
            "CommonProgramFiles(x86)",
            r"C:\Program Files (x86)\Common Files",
        ),
        (
            "Path",
            r"C:\Windows\system32;C:\Program Files\Git\cmd;C:\PROGRA~1\Git\cmd;D:\Tools",
        ),
        (GIT_BASH_VAR, r"C:\Program Files\Git\bin\bash.exe"),
    ]);
    env.extend(pairs(lawyer));
    LaunchMessage {
        version: crate::windows::runner::LAUNCH_VERSION,
        pool_user: sid(POOL),
        pool_name: "ClauseoSb05".to_owned(),
        matter_sid: None,
        program: format!(r"{RUNTIME}\node.exe"),
        args: Vec::new(),
        cwd: r"D:\Matters\Smith".to_owned(),
        env,
        agent_env: pairs(host),
        state: state.to_owned(),
        tmpdir: None,
        runtime: vec![RUNTIME.to_owned()],
        granted: vec![state.to_owned(), RUNTIME.to_owned()],
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

fn off(stub: &str) -> GitEnv {
    GitEnv {
        bash: Bash::Off {
            stub: WinPath::parse(stub).unwrap(),
            reason: Off::Squatted.to_string(),
        },
    }
}

fn system() -> String {
    WINDOWS_PATH.join(";")
}

#[test]
fn with_a_tree_the_agent_s_git_and_bash_are_the_tree_s() {
    let git = GitEnv {
        bash: Bash::Tree(WinPath::parse(&tree()).unwrap()),
    };
    let spec = launch(&[(OFF_VAR, "the lawyer's")], &[(OFF_VAR, "the host's")]);
    let env = env_of(&git, &spec);
    assert_eq!(
        get(&env, GIT_BASH_VAR),
        Some(format!(r"{}\bin\bash.exe", tree()).as_str())
    );
    assert_eq!(
        get(&env, "PATH"),
        Some(format!(r"{}\cmd;{RUNTIME};{}", tree(), system()).as_str()),
        "the tree's cmd, the runtime, Windows' own: nothing of the lawyer's PATH"
    );
    let paths = env.iter().filter(|(n, _)| n.eq_ignore_ascii_case("path"));
    assert_eq!(paths.count(), 1);
    assert_eq!(get(&env, "GIT_CONFIG_VALUE_0"), Some("openssl"));
    assert_eq!(get(&env, OFF_VAR), None, "neither's");
}

/// The opposite of the old fallback: with Git Bash off the variable is never left unset (Claude
/// Code would look for the lawyer's Git Bash); it names the stub, and the stub says why.
#[test]
fn off_always_names_a_stub_never_nothing() {
    for stub in [STUB_PATH, OWN_STUB] {
        let spec = launch(&[], &[(OFF_VAR, "the host's"), (GIT_BASH_VAR, "x")]);
        let env = env_of(&off(stub), &spec);
        assert_eq!(get(&env, GIT_BASH_VAR), Some(stub), "never the lawyer's");
        assert_eq!(get(&env, OFF_VAR), Some(Off::Squatted.to_string().as_str()));
        assert_eq!(
            get(&env, "PATH"),
            Some(format!("{RUNTIME};{}", system()).as_str())
        );
    }
}

#[test]
fn only_the_base_set_the_host_s_variables_and_ours_reach_the_agent() {
    let lawyer = [
        ("PERL5OPT", "-Mevil"),
        ("NODE_OPTIONS", "--require x"),
        ("HTTPS_PROXY", "http://lawyer-proxy:8080"),
        ("EDITOR", r"C:\Program Files\Git\usr\bin\vim.exe"),
        ("VISUAL", r"C:\Program Files\Git\usr\bin\vim.exe"),
        ("PAGER", r"C:\Program Files\Git\usr\bin\less.exe"),
        (
            "GIT_EXEC_PATH",
            r"C:\Program Files\Git\mingw64\libexec\git-core",
        ),
        ("MSYSTEM", "MINGW64"),
        ("SSH_ASKPASS", r"C:\x.exe"),
        ("ANTHROPIC_API_KEY", "the lawyer's"),
        ("HOME", r"D:\elsewhere"),
    ];
    let host = [
        ("HTTPS_PROXY", "http://127.0.0.1:46100"),
        ("ANTHROPIC_API_KEY", "the host's"),
        ("NODE_OPTIONS", "--require y"),
        ("PATH", r"C:\Program Files\Git\cmd"),
        ("HOME", r"D:\host"),
        ("USERPROFILE", r"C:\Users\Lawyer"),
    ];
    let spec = launch(&lawyer, &host);
    let env = env_of(&off(STUB_PATH), &spec);
    for (name, _) in lawyer {
        let host_set = host.iter().any(|(h, _)| h.eq_ignore_ascii_case(name));
        if !host_set {
            assert_eq!(get(&env, name), None, "{name}");
        }
    }
    assert_eq!(get(&env, "HTTPS_PROXY"), Some("http://127.0.0.1:46100"));
    assert_eq!(get(&env, "ANTHROPIC_API_KEY"), Some("the host's"));
    assert_eq!(get(&env, "NODE_OPTIONS"), None, "never, even the host's");
    assert_eq!(get(&env, "HOME"), Some(spec.state.as_str()), "ours");
    assert_eq!(get(&env, "USERPROFILE"), Some(spec.state.as_str()), "ours");
    assert!(!get(&env, "PATH").unwrap().contains("Git"), "ours alone");
    for (name, value) in [
        ("SystemRoot", r"C:\Windows"),
        ("PATHEXT", ".COM;.EXE"),
        ("PROCESSOR_ARCHITECTURE", "AMD64"),
        (
            "CommonProgramFiles(x86)",
            r"C:\Program Files (x86)\Common Files",
        ),
    ] {
        assert_eq!(get(&env, name), Some(value), "the base set: {name}");
    }
    assert!(is_base("windir") && is_base("processor_level") && is_base("COMMONPROGRAMW6432"));
    assert!(!is_base("PATH") && !is_base("EDITOR") && !is_base("HTTPS_PROXY"));
}

#[test]
fn the_own_copy_s_folder_must_lie_in_local_app_data_by_identity() {
    let refuse: [fn(&mut Fake); 3] = [
        |f| {
            f.links
                .push(r"c:\users\lawyer\appdata\local\clauseo".to_owned());
        },
        |f| {
            f.links.push(r"c:\users\lawyer\appdata".to_owned());
        },
        // Another user's LocalAppData: another folder at that depth.
        |f| f.app_data = Ok(r"C:\Users\Other\AppData\Local".to_owned()),
    ];
    for change in refuse {
        let mut fake = Fake::new();
        fake.stub = Err("no machine stub".to_owned());
        change(&mut fake);
        assert!(off_stub(&mut fake, &sid(LAWYER), DATA).is_err());
        assert_eq!(fake.copies, 0, "nothing made outside it");
    }
    // A short name of the same folder passes: the file IDs match, whatever the text.
    let mut fake = Fake::new();
    fake.stub = Err("no machine stub".to_owned());
    fake.app_data = Ok(r"C:\Users\LAWYER~1\AppData\Local".to_owned());
    let same = (7, 7);
    fake.ids
        .insert(r"c:\users\lawyer~1\appdata\local".to_owned(), same);
    fake.ids
        .insert(r"c:\users\lawyer\appdata\local".to_owned(), same);
    assert!(off_stub(&mut fake, &sid(LAWYER), DATA).is_ok());
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
