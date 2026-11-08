use std::collections::HashMap;

use super::*;
use crate::windows::runner::LaunchMessage;
use crate::windows::runner::env::{GIT_BASH_VAR, GitEnv, agent_env};

const ADMIN: &str = r"C:\Program Files\Clauseo\Sandbox";
const POOL: &str = "S-1-5-21-1-2-3-1005";
const GROUP: &str = "S-1-5-21-1-2-3-1000";
const LAWYER: &str = "S-1-5-21-1-2-3-1001";
const ID: &str = "2.47.1.windows.1-0123456789ab";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn tree() -> String {
    format!(r"{ADMIN}\git\ClauseoSb05\{ID}")
}

fn image(id: &[u8]) -> Vec<u8> {
    let mut image = b"MZ\x90\0 some code ".to_vec();
    image.extend_from_slice(id);
    image.extend_from_slice(b"\0 more");
    image
}

/// A machine whose tree passes every check, each effect recorded.
struct Fake {
    record: Result<Option<String>, String>,
    admin: Result<WinPath, String>,
    levels: Result<(), String>,
    marker: Result<String, String>,
    runtime: Result<Runtime, String>,
    session: u32,
    existing: Vec<String>,
    fail_make: bool,
    checked: Vec<(String, String)>,
    made: HashMap<String, String>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            record: Ok(Some(tree())),
            admin: Ok(WinPath::parse(ADMIN).unwrap()),
            levels: Ok(()),
            marker: Ok(format!("{ID}\n{POOL}\n")),
            runtime: Ok(Runtime {
                final_path: format!(r"\\?\{}\usr\bin\msys-2.0.dll", tree()),
                image: image(b"%%% MSYS shared id: msys-2.0S5"),
            }),
            session: 2,
            existing: Vec::new(),
            fail_make: false,
            checked: Vec::new(),
            made: HashMap::new(),
        }
    }

    fn run(&mut self) -> Result<Ready<String>, Off> {
        prepare(self, &sid(POOL), "ClauseoSb05", &sid(LAWYER))
    }
}

impl GitBashOs for Fake {
    type Held = String;

    fn record(&mut self, pool_user: &Sid) -> Result<Option<String>, String> {
        assert_eq!(pool_user.as_str(), POOL);
        self.record.clone()
    }

    fn admin_dir(&mut self) -> Result<WinPath, String> {
        self.admin.clone()
    }

    fn group(&mut self) -> Result<Sid, String> {
        Ok(sid(GROUP))
    }

    fn check_levels(&mut self, admin: &WinPath, levels: &[(String, String)]) -> Result<(), String> {
        assert_eq!(admin.display(), ADMIN);
        self.checked = levels.to_vec();
        self.levels.clone()
    }

    fn marker(&mut self, path: &WinPath) -> Result<String, String> {
        assert!(
            path.display()
                .eq_ignore_ascii_case(&format!(r"{}\.clauseo-tree", tree()))
        );
        self.marker.clone()
    }

    fn runtime(&mut self, dll: &WinPath) -> Result<Runtime, String> {
        assert!(
            dll.display()
                .eq_ignore_ascii_case(&format!(r"{}\usr\bin\msys-2.0.dll", tree()))
        );
        self.runtime.clone()
    }

    fn session(&mut self) -> Result<u32, String> {
        Ok(self.session)
    }

    fn make_dir(&mut self, path: &str, sddl: &str) -> Result<String, Made> {
        if self.existing.iter().any(|there| there == path) {
            return Err(Made::Exists);
        }
        if self.fail_make {
            return Err(Made::Failed(
                "NtCreateDirectoryObject failed (0xC0000022)".to_owned(),
            ));
        }
        self.made.insert(path.to_owned(), sddl.to_owned());
        Ok(path.to_owned())
    }
}

#[test]
fn the_hash_port_gives_the_vm_s_keys() {
    for (root, key) in [
        (r"C:\Program Files\Git", "1888ae32e00d56aa"),
        (r"C:\clauseo-test\run-A\git", "45a176edb0b42059"),
        (r"C:\clauseo-test\run-B\git", "5849f6c194aa6698"),
    ] {
        let dll = WinPath::parse(&format!(r"{root}\usr\bin\msys-2.0.dll")).unwrap();
        let found = format!(r"\\?\{root}\usr\bin\msys-2.0.dll");
        assert_eq!(msys_key(&found, &dll).as_deref(), Some(key), "{root}");
        // Upper-cased: the path's own case changes nothing.
        let upper = found.to_uppercase();
        assert_eq!(msys_key(&upper, &dll).as_deref(), Some(key), "{root}");
    }
}

#[test]
fn the_key_needs_the_tree_s_own_plain_path() {
    let dll = WinPath::parse(r"C:\t\usr\bin\msys-2.0.dll").unwrap();
    assert_eq!(
        msys_key(r"\\?\D:\t\usr\bin\msys-2.0.dll", &dll),
        None,
        "elsewhere"
    );
    assert_eq!(
        msys_key(r"C:\t\usr\bin\msys-2.0.dll", &dll),
        None,
        "not a final path"
    );
    assert_eq!(msys_key(r"\\?\UNC\s\c\t\usr\bin\msys-2.0.dll", &dll), None);
    let wide = WinPath::parse("C:\\t\u{e9}\\usr\\bin\\msys-2.0.dll").unwrap();
    assert_eq!(
        msys_key("\\\\?\\C:\\t\u{e9}\\usr\\bin\\msys-2.0.dll", &wide),
        None
    );
    assert_eq!(msys_hash(""), Some(0));
}

#[test]
fn the_shared_id_is_read_from_the_dll() {
    let found = image(b"%%% MSYS shared id: msys-2.0S5");
    assert_eq!(shared_id(&found).as_deref(), Some("msys-2.0S5"));
    let mut newer = b"\x01%%% MSYS shared id: msys-12.34S56\x7f".to_vec();
    newer.extend_from_slice(b"%%% MSYS shared id: msys-9.9S9");
    assert_eq!(
        shared_id(&newer).as_deref(),
        Some("msys-12.34S56"),
        "the first, to a byte"
    );
    assert_eq!(shared_id(&image(b"msys-2.0S5 alone")), None, "missing");
    assert_eq!(shared_id(b""), None);
    for bad in [
        &b"%%% MSYS shared id: msys-2.0S"[..],
        b"%%% MSYS shared id: msys-2S5",
        b"%%% MSYS shared id: cygwin1S5",
        b"%%% MSYS shared id: msys-2.0S5 x",
        b"%%% MSYS shared id: msys-a.0S5",
        b"%%% MSYS shared id: ",
    ] {
        assert_eq!(
            shared_id(&image(bad)),
            None,
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn a_checked_tree_makes_both_folders_with_the_run_s_descriptor() {
    let mut fake = Fake::new();
    let ready = fake.run().unwrap();
    assert_eq!(ready.tree.display(), tree());
    let key = msys_key(
        &fake.runtime.clone().unwrap().final_path,
        &ready.tree.join("usr").join("bin").join("msys-2.0.dll"),
    )
    .unwrap();
    let session = format!(r"\Sessions\2\BaseNamedObjects\msys-2.0S5-{key}");
    let global = format!(r"\BaseNamedObjects\msys-2.0S5-{key}");
    assert_eq!(ready.held, vec![session.clone(), global.clone()]);
    let sddl = object_dir_sddl(&sid(LAWYER), &sid(POOL));
    assert_eq!(fake.made.get(&session), Some(&sddl));
    assert_eq!(fake.made.get(&global), Some(&sddl));
    let names: Vec<&str> = fake.checked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["git", "ClauseoSb05", ID]);
    assert_eq!(fake.checked[0].1, git_dir_sddl(&sid(GROUP)));
    assert_eq!(fake.checked[1].1, account_sddl(&sid(POOL)));
    assert_eq!(fake.checked[2].1, account_sddl(&sid(POOL)));
}

#[test]
fn the_folders_descriptor_is_the_vm_s() {
    assert_eq!(
        object_dir_sddl(&sid(LAWYER), &sid(POOL)),
        "O:S-1-5-21-1-2-3-1001D:(A;;0xF000F;;;SY)(A;;0xF000F;;;S-1-5-21-1-2-3-1001)\
         (A;;0x2000F;;;S-1-5-21-1-2-3-1005)S:(ML;;NW;;;LW)"
    );
    assert_eq!(
        object_dirs(0, "n"),
        [r"\BaseNamedObjects\n"],
        "session 0 has one"
    );
}

#[test]
fn a_missing_unknown_or_unavailable_record_turns_git_bash_off() {
    let cases: [(Result<Option<String>, String>, Off); 5] = [
        (Ok(None), Off::NotRecorded),
        (
            Ok(Some("Git Bash unavailable: no-git".to_owned())),
            Off::Unavailable("no-git".to_owned()),
        ),
        (
            Ok(Some("Git Bash unavailable: <b>".to_owned())),
            Off::Unavailable("unknown".to_owned()),
        ),
        (Ok(Some(String::new())), Off::Malformed),
        (
            Err("access denied".to_owned()),
            Off::Unreadable("access denied".to_owned()),
        ),
    ];
    for (record, off) in cases {
        let mut fake = Fake::new();
        fake.record = record;
        assert_eq!(fake.run().unwrap_err(), off);
        assert!(fake.made.is_empty() && fake.checked.is_empty());
    }
}

#[test]
fn a_record_path_outside_the_account_s_folder_turns_git_bash_off() {
    for path in [
        r"C:\Program Files\Git".to_owned(),
        format!(r"{ADMIN}\git\ClauseoSb06\{ID}"),
        format!(r"{ADMIN}\git\ClauseoSb05"),
        format!(r"{ADMIN}\git\ClauseoSb05\{ID}\usr"),
        format!(r"{ADMIN}\git\ClauseoSb05\{ID}.tmp"),
        format!(r"{ADMIN}\git\_source\{ID}"),
        format!(r"{ADMIN}\other\ClauseoSb05\{ID}"),
        format!(r"D:\Sandbox\git\ClauseoSb05\{ID}"),
        format!(r"{ADMIN}\git\ClauseoSb05\.."),
        r"relative\git".to_owned(),
    ] {
        let mut fake = Fake::new();
        fake.record = Ok(Some(path.clone()));
        assert_eq!(fake.run().unwrap_err(), Off::Elsewhere, "{path}");
        assert!(fake.made.is_empty() && fake.checked.is_empty(), "{path}");
    }
    let mut cased = Fake::new();
    cased.record = Ok(Some(tree().to_uppercase()));
    cased.runtime = Ok(Runtime {
        final_path: format!(r"\\?\{}\USR\BIN\MSYS-2.0.DLL", tree().to_uppercase()),
        image: image(b"%%% MSYS shared id: msys-2.0S5"),
    });
    assert!(cased.run().is_ok(), "paths compare without case");
}

#[test]
fn a_failed_tree_check_turns_git_bash_off() {
    let mut admin = Fake::new();
    admin.admin = Err("its chain was refused".to_owned());
    assert!(matches!(admin.run(), Err(Off::AdminFolder(_))));
    let mut level = Fake::new();
    level.levels = Err("ClauseoSb05's owner or DACL isn't set-up's".to_owned());
    assert!(matches!(level.run(), Err(Off::Unverified(_))));
    for text in [
        String::new(),
        format!("{ID}\n"),
        format!("{ID}\nS-1-5-21-1-2-3-1006\n"),
        format!("other-id\n{POOL}\n"),
        format!("{ID}\n{POOL}\nmore\n"),
    ] {
        let mut marker = Fake::new();
        marker.marker = Ok(text.clone());
        assert!(matches!(marker.run(), Err(Off::Unverified(_))), "{text:?}");
    }
    let mut missing = Fake::new();
    missing.marker = Err("its marker failed (2)".to_owned());
    assert!(matches!(missing.run(), Err(Off::Unverified(_))));
    let mut unread = Fake::new();
    unread.runtime = Err("opening it failed (5)".to_owned());
    assert!(matches!(unread.run(), Err(Off::Runtime(_))));
    let mut moved = Fake::new();
    moved.runtime = Ok(Runtime {
        final_path: r"\\?\C:\Program Files\Git\usr\bin\msys-2.0.dll".to_owned(),
        image: image(b"%%% MSYS shared id: msys-2.0S5"),
    });
    assert_eq!(moved.run().unwrap_err(), Off::Moved);
    let mut no_id = Fake::new();
    no_id.runtime = Ok(Runtime {
        final_path: format!(r"\\?\{}\usr\bin\msys-2.0.dll", tree()),
        image: image(b"no id here"),
    });
    assert_eq!(no_id.run().unwrap_err(), Off::NoSharedId);
    for fake in [admin, level, unread, moved, no_id] {
        assert!(fake.made.is_empty(), "no folder for an unchecked tree");
    }
    assert!(marker_ok(&format!("{ID}\r\n{POOL}"), ID, &sid(POOL)));
}

#[test]
fn an_existing_folder_is_squatted_and_turns_git_bash_off() {
    let key = {
        let mut fake = Fake::new();
        let ready = fake.run().unwrap();
        ready.held[1].clone()
    };
    let mut global = Fake::new();
    global.existing = vec![key];
    assert_eq!(global.run().unwrap_err(), Off::Squatted);
    let mut session = Fake::new();
    session.existing = vec![format!(
        r"\Sessions\2\BaseNamedObjects\msys-2.0S5-{}",
        msys_key(
            &format!(r"\\?\{}\usr\bin\msys-2.0.dll", tree()),
            &WinPath::parse(&format!(r"{}\usr\bin\msys-2.0.dll", tree())).unwrap()
        )
        .unwrap()
    )];
    assert_eq!(session.run().unwrap_err(), Off::Squatted);
    assert!(session.made.is_empty());
    let mut failed = Fake::new();
    failed.fail_make = true;
    assert!(matches!(failed.run(), Err(Off::ObjectDir(_))));
}

#[test]
fn each_off_reason_is_one_plain_line_naming_no_path() {
    let reasons = [
        Off::NotRecorded,
        Off::Unavailable("no-git".to_owned()),
        Off::Malformed,
        Off::Unreadable("access denied".to_owned()),
        Off::AdminFolder("x".to_owned()),
        Off::Elsewhere,
        Off::Unverified("x".to_owned()),
        Off::Runtime("x".to_owned()),
        Off::Moved,
        Off::NoSharedId,
        Off::Squatted,
        Off::ObjectDir("x".to_owned()),
    ];
    for reason in reasons {
        let line = reason.to_string();
        assert!(!line.contains('\n') && !line.contains(":\\"), "{line}");
    }
}

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

const SPEC_PATH: &str =
    r"C:\Windows\system32;C:\Program Files\Git\cmd;C:\Program Files\Git\mingw64\bin;D:\Tools";

fn env_with(tree: Option<&str>) -> Vec<(String, String)> {
    let git = GitEnv {
        tree: tree.map(|t| WinPath::parse(t).unwrap()),
        lawyer_git: vec![WinPath::parse(r"C:\Program Files\Git").unwrap()],
    };
    let profile = WinPath::parse(r"C:\Users\Lawyer").unwrap();
    agent_env(&launch(SPEC_PATH), &profile, &git).unwrap()
}

#[test]
fn with_a_tree_the_agent_s_git_and_bash_are_the_tree_s() {
    let env = env_with(Some(&tree()));
    assert_eq!(
        get(&env, GIT_BASH_VAR),
        Some(format!(r"{}\bin\bash.exe", tree()).as_str())
    );
    assert_eq!(
        get(&env, "PATH"),
        Some(format!(r"{}\cmd;C:\Windows\system32;D:\Tools", tree()).as_str()),
        "the tree's cmd first, the lawyer's Git gone, the rest in order"
    );
    assert_eq!(
        env.iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("path"))
            .count(),
        1
    );
    assert_eq!(
        get(&env, "GIT_CONFIG_VALUE_0"),
        Some("openssl"),
        "kept as it is"
    );
}

#[test]
fn with_no_tree_the_agent_gets_no_git_bash_and_never_the_lawyer_s() {
    let env = env_with(None);
    assert_eq!(get(&env, GIT_BASH_VAR), None, "the spec's goes");
    assert_eq!(get(&env, "PATH"), Some(r"C:\Windows\system32;D:\Tools"));
    let git = GitEnv {
        tree: None,
        lawyer_git: vec![WinPath::parse(r"C:\Program Files\Git").unwrap()],
    };
    let profile = WinPath::parse(r"C:\Users\Lawyer").unwrap();
    let only_git = launch(r"C:\Program Files\Git\cmd");
    assert_eq!(
        get(&agent_env(&only_git, &profile, &git).unwrap(), "PATH"),
        None
    );
}
