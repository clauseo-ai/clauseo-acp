//! The launch message, its checks, the environment, the paths, the frames and the exit codes.

use super::env::{CA_VARS, GIT_OPENSSL, firm_ca_files, profile_folders};
use super::frame::{InnerReport, command_line, read_frame, write_frame};
use super::paths::{CodexReuse, WinPath};
use super::*;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1005";
const PROFILE: &str = r"C:\Users\Lawyer";
const DATA: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo";
const STATE: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo\agent-state\m1\codex-acp";
const RUNTIME: &str = r"C:\Users\Lawyer\AppData\Local\Clauseo\runtime\node";

fn message() -> LaunchMessage {
    LaunchMessage {
        version: LAUNCH_VERSION,
        pool_user: Sid::parse(POOL).unwrap(),
        pool_name: "ClauseoSb05".to_owned(),
        matter_sid: None,
        program: format!(r"{RUNTIME}\node.exe"),
        args: vec!["codex-acp.js".to_owned(), "--flag value".to_owned()],
        cwd: r"D:\Matters\Smith".to_owned(),
        env: vec![
            ("SystemRoot".to_owned(), r"C:\Windows".to_owned()),
            ("CODEX_HOME".to_owned(), STATE.to_owned()),
        ],
        state: STATE.to_owned(),
        tmpdir: None,
        runtime: vec![RUNTIME.to_owned()],
        granted: vec![
            STATE.to_owned(),
            RUNTIME.to_owned(),
            r"D:\Matters\Smith".to_owned(),
        ],
        ca_bundle: Some(format!(r"{DATA}\sandbox-ca\roots.pem")),
        credential_link: Some(format!(r"{STATE}\auth.json")),
        firm_ca: Vec::new(),
    }
}

fn profile() -> WinPath {
    WinPath::parse(PROFILE).unwrap()
}

fn get<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn a_message_round_trips_and_parses() {
    let bytes = message().to_bytes().unwrap();
    assert_eq!(LaunchMessage::parse(&bytes).unwrap(), message());
}

#[test]
fn the_parse_refuses_what_breaks_a_rule() {
    type Change = Box<dyn Fn(&mut LaunchMessage)>;
    let cases: Vec<(&str, Change)> = vec![
        (
            "pool_name",
            Box::new(|m| m.pool_name = "Administrator".to_owned()),
        ),
        ("program", Box::new(|m| m.program = "node.exe".to_owned())),
        (
            "program",
            Box::new(|m| m.program = r"C:\x\run.CMD. ".to_owned()),
        ),
        (
            "program",
            Box::new(|m| m.program = r"C:\x\..\y.exe".to_owned()),
        ),
        ("cwd", Box::new(|m| m.cwd = r"C:relative".to_owned())),
        (
            "state",
            Box::new(|m| m.state = r"\\server\share\state".to_owned()),
        ),
        ("tmpdir", Box::new(|m| m.tmpdir = Some(r"tmp".to_owned()))),
        (
            "runtime",
            Box::new(|m| m.runtime.push(r"\\.\pipe\x".to_owned())),
        ),
        (
            "env",
            Box::new(|m| m.env.push(("A=B".to_owned(), "x".to_owned()))),
        ),
        (
            "env",
            Box::new(|m| m.env.push((String::new(), "x".to_owned()))),
        ),
        (
            "env",
            Box::new(|m| m.env.push(("A".to_owned(), "x\0y".to_owned()))),
        ),
        ("args", Box::new(|m| m.args.push("a\0".to_owned()))),
    ];
    for (field, change) in cases {
        let mut bad = message();
        change(&mut bad);
        match bad.check() {
            Err(LaunchError::Field { field: got, .. }) => assert!(got.contains(field), "{got}"),
            other => panic!("{field}: {other:?}"),
        }
    }
    let mut bad = message();
    bad.version = 2;
    assert_eq!(bad.check(), Err(LaunchError::Version(2)));
    // A per-drive folder variable passes the check and is dropped from the environment.
    let mut drive = message();
    drive.env.push(("=C:".to_owned(), r"C:\x".to_owned()));
    drive.check().unwrap();
    assert!(get(&agent_env(&drive, &profile()).unwrap(), "=C:").is_none());
}

#[test]
fn unknown_fields_and_oversize_files_are_refused() {
    let mut value: serde_json::Value =
        serde_json::from_slice(&message().to_bytes().unwrap()).unwrap();
    value["extra"] = serde_json::json!(1);
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        LaunchMessage::parse(&bytes),
        Err(LaunchError::Format(_))
    ));
    let huge = vec![b' '; usize::try_from(LAUNCH_CAP).unwrap() + 1];
    assert!(matches!(
        LaunchMessage::parse(&huge),
        Err(LaunchError::Format(_))
    ));
}

#[test]
fn debug_never_prints_environment_values() {
    let mut secret = message();
    secret
        .env
        .push(("CLAUSEO_PROXY_TOKEN".to_owned(), "hunter2-token".to_owned()));
    let text = format!("{secret:?}");
    assert!(text.contains("CLAUSEO_PROXY_TOKEN"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
}

#[test]
fn the_profile_variables_lie_under_the_state_folder() {
    let env = agent_env(&message(), &profile()).unwrap();
    assert_eq!(get(&env, "USERPROFILE"), Some(STATE));
    assert_eq!(get(&env, "HOME"), Some(STATE));
    assert_eq!(get(&env, "HOMEDRIVE"), Some("C:"));
    assert_eq!(get(&env, "HOMEPATH"), STATE.strip_prefix("C:"));
    assert_eq!(
        get(&env, "APPDATA"),
        Some(format!(r"{STATE}\AppData\Roaming").as_str())
    );
    assert_eq!(
        get(&env, "LOCALAPPDATA"),
        Some(format!(r"{STATE}\AppData\Local").as_str())
    );
    assert_eq!(get(&env, "TEMP"), Some(format!(r"{STATE}\tmp").as_str()));
    assert_eq!(get(&env, "TMP"), get(&env, "TEMP"));
    assert_eq!(
        profile_folders(&message()).unwrap(),
        [
            format!(r"{STATE}\AppData\Roaming"),
            format!(r"{STATE}\AppData\Local"),
            format!(r"{STATE}\tmp"),
        ]
    );
}

#[test]
fn the_session_temp_folder_wins_when_set() {
    let mut with_tmp = message();
    with_tmp.tmpdir = Some(format!(r"{DATA}\run\s1\tmp"));
    let env = agent_env(&with_tmp, &profile()).unwrap();
    assert_eq!(
        get(&env, "TEMP"),
        Some(format!(r"{DATA}\run\s1\tmp").as_str())
    );
    assert_eq!(get(&env, "TMP"), get(&env, "TEMP"));
    assert_eq!(
        profile_folders(&with_tmp).unwrap().len(),
        2,
        "the host made its temp folder"
    );
}

#[test]
fn verbatim_paths_come_out_plain() {
    let mut verbatim = message();
    verbatim.state = format!(r"\\?\{STATE}");
    let env = agent_env(&verbatim, &profile()).unwrap();
    assert_eq!(get(&env, "USERPROFILE"), Some(STATE));
}

#[test]
fn git_uses_openssl_and_codex_the_bundle() {
    let mut spec = message();
    spec.env.extend([
        ("GIT_CONFIG_COUNT".to_owned(), "2".to_owned()),
        ("GIT_CONFIG_KEY_1".to_owned(), "core.sshCommand".to_owned()),
        ("git_config_value_1".to_owned(), "evil".to_owned()),
        ("SSL_CERT_FILE".to_owned(), r"C:\elsewhere.pem".to_owned()),
    ]);
    let env = agent_env(&spec, &profile()).unwrap();
    for (name, value) in GIT_OPENSSL {
        assert_eq!(get(&env, name), Some(value));
    }
    assert!(get(&env, "GIT_CONFIG_KEY_1").is_none());
    assert!(get(&env, "GIT_CONFIG_VALUE_1").is_none());
    for name in CA_VARS {
        assert_eq!(
            get(&env, name),
            Some(format!(r"{DATA}\sandbox-ca\roots.pem").as_str())
        );
    }
    // With no bundle, the spec's own values pass.
    spec.ca_bundle = None;
    let env = agent_env(&spec, &profile()).unwrap();
    assert_eq!(get(&env, "SSL_CERT_FILE"), Some(r"C:\elsewhere.pem"));
    assert!(get(&env, "CODEX_CA_CERTIFICATE").is_none());
}

#[test]
fn nothing_points_back_at_the_lawyers_profile() {
    let mut spec = message();
    spec.env.extend([
        ("USERPROFILE".to_owned(), PROFILE.to_owned()),
        ("OneDrive".to_owned(), r"C:\Users\Lawyer\OneDrive".to_owned()),
        ("NPM_CONFIG_CACHE".to_owned(), r"c:/users/LAWYER/AppData/Roaming/npm-cache".to_owned()),
        ("MIXED".to_owned(), format!(r"{STATE};C:\Users\Lawyer\Documents")),
        ("QUOTED".to_owned(), r#""C:\Users\Lawyer\x""#.to_owned()),
        ("ProgramData".to_owned(), r"C:\ProgramData".to_owned()),
        ("CLAUSEO_NODE".to_owned(), format!(r"{RUNTIME}\node.exe")),
        ("NODE_OPTIONS".to_owned(), "--require x".to_owned()),
        (
            "Path".to_owned(),
            format!(
                r"C:\Windows\system32;C:\Users\Lawyer\AppData\Local\Programs\fnm;{RUNTIME};;{RUNTIME}\bin;C:\Users\Lawyer\bin"
            ),
        ),
        (
            "PSModulePath".to_owned(),
            r"C:\Users\Lawyer\Documents\PowerShell\Modules;C:\Program Files\PowerShell\Modules".to_owned(),
        ),
    ]);
    let env = agent_env(&spec, &profile()).unwrap();
    for gone in [
        "OneDrive",
        "NPM_CONFIG_CACHE",
        "MIXED",
        "QUOTED",
        "NODE_OPTIONS",
    ] {
        assert!(get(&env, gone).is_none(), "{gone} kept: {env:?}");
    }
    assert_eq!(get(&env, "ProgramData"), Some(r"C:\ProgramData"));
    assert_eq!(
        get(&env, "CLAUSEO_NODE"),
        Some(format!(r"{RUNTIME}\node.exe").as_str())
    );
    assert_eq!(get(&env, "CODEX_HOME"), Some(STATE), "granted paths stay");
    assert_eq!(
        get(&env, "PATH"),
        Some(format!(r"C:\Windows\system32;{RUNTIME};{RUNTIME}\bin").as_str())
    );
    assert_eq!(
        get(&env, "PSModulePath"),
        Some(r"C:\Program Files\PowerShell\Modules")
    );
    for (name, value) in &env {
        let lawyer = WinPath::parse(PROFILE).unwrap();
        for part in value.split(';') {
            let inside = super::paths::text_within(part, &lawyer);
            let granted = [STATE, RUNTIME, DATA]
                .iter()
                .any(|g| super::paths::text_within(part, &WinPath::parse(g).unwrap()));
            assert!(!inside || granted, "{name}={value}");
        }
    }
}

#[test]
fn the_environment_is_sorted_with_one_entry_per_name() {
    let mut spec = message();
    spec.env.push(("path".to_owned(), r"C:\a".to_owned()));
    spec.env.push(("PATH".to_owned(), r"C:\b".to_owned()));
    let env = agent_env(&spec, &profile()).unwrap();
    let names: Vec<String> = env.iter().map(|(n, _)| n.to_uppercase()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted);
    assert_eq!(
        get(&env, "PATH"),
        Some(r"C:\b"),
        "the last of one name wins"
    );
}

#[test]
fn paths_compare_as_windows_compares_them() {
    let root = WinPath::parse(r"C:\Users\Lawyer").unwrap();
    for inside in [
        r"C:\Users\Lawyer",
        r"c:\users\lawyer\x",
        r"\\?\C:\Users\LAWYER\y",
        "C:/Users/Lawyer/z",
    ] {
        assert!(WinPath::parse(inside).unwrap().within(&root), "{inside}");
    }
    for outside in [r"C:\Users\Lawyer2", r"D:\Users\Lawyer", r"C:\Users"] {
        assert!(!WinPath::parse(outside).unwrap().within(&root), "{outside}");
    }
    for refused in [
        "relative",
        r"C:x",
        r"\\.\pipe\x",
        r"C:\a\..\b",
        r"\\server",
        "",
    ] {
        assert!(WinPath::parse(refused).is_none(), "{refused}");
    }
    let unc = WinPath::parse(r"\\server\share\a").unwrap();
    assert_eq!(unc.drive(), None);
    assert_eq!(unc.display(), r"\\server\share\a");
    assert_eq!(WinPath::parse(r"d:\").unwrap().below_drive(), r"\");
}

#[test]
fn m3_refuses_the_lawyers_codex_folder() {
    let profile = profile();
    let codex = r"C:\Users\Lawyer\.codex";
    assert_eq!(codex_reuse(&profile, [("state", STATE)], &[]), Ok(()));
    assert_eq!(
        codex_reuse(
            &profile,
            [("credential_link", r"C:\Users\Lawyer\.CODEX\auth.json")],
            &[]
        ),
        Err(CodexReuse::Path("credential_link"))
    );
    assert_eq!(
        codex_reuse(&profile, [("granted", codex)], &[]),
        Err(CodexReuse::Path("granted"))
    );
    let env = vec![("codex_home".to_owned(), codex.to_owned())];
    assert_eq!(codex_reuse(&profile, [], &env), Err(CodexReuse::CodexHome));
    let near = vec![(
        "CODEX_HOME".to_owned(),
        r"C:\Users\Lawyer\.codex-other".to_owned(),
    )];
    assert_eq!(codex_reuse(&profile, [], &near), Ok(()));
}

#[test]
fn frames_round_trip_and_refuse_oversize() {
    let mut wire = Vec::new();
    write_frame(&mut wire, &InnerReport::Started { pid: 42 }).unwrap();
    assert_eq!(
        read_frame::<InnerReport>(wire.as_slice()).unwrap(),
        InnerReport::Started { pid: 42 }
    );
    let mut huge = (u32::try_from(LAUNCH_CAP).unwrap() + 1)
        .to_le_bytes()
        .to_vec();
    huge.extend_from_slice(b"{}");
    assert!(read_frame::<InnerReport>(huge.as_slice()).is_err());
    let short = [5u8, 0, 0, 0, b'{'];
    assert!(read_frame::<InnerReport>(short.as_slice()).is_err());
}

#[test]
fn command_lines_quote_as_the_c_runtime_reads_them() {
    let args: Vec<String> = [
        "plain",
        "",
        "two words",
        r#"say "hi""#,
        r"C:\dir\",
        r#"a\"b"#,
        r"x\\y",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(
        command_line(r"C:\p\node.exe", &args),
        r#""C:\p\node.exe" plain "" "two words" "say \"hi\"" C:\dir\ "a\\\"b" x\\y"#
    );
    assert_eq!(
        command_line(r"C:\p q\n.exe", &[r"dir\ ".to_owned()]),
        r#""C:\p q\n.exe" "dir\ ""#
    );
}

#[test]
fn exit_codes_are_stable_and_map_to_set_up_causes() {
    assert_eq!(RunExit::Usage.code(), 0xE0C1_5000);
    assert_eq!(RunExit::SecondaryLogon.code(), 0xE0C1_5003);
    assert_eq!(RunExit::Spawn.code(), 0xE0C1_500B);
    for exit in RunExit::ALL {
        assert_eq!(RunExit::from_code(exit.code()), Some(exit));
    }
    assert_eq!(RunExit::from_code(0), None);
    assert_eq!(RunExit::from_code(RUN_EXIT_BASE + 13), None);
    assert_eq!(RunExit::AgentReserved.no_sandbox(), None);
    // An agent exiting with a runner code never passes for a runner failure.
    for code in [
        RunExit::Password.code(),
        RunExit::SecondaryLogon.code(),
        RUN_EXIT_BASE + 0xff,
    ] {
        assert_eq!(agent_code(code), RunExit::AgentReserved.code());
    }
    for code in [0, 7, RUN_EXIT_BASE - 1, RUN_EXIT_BASE + 0x100, 0xC000_0142] {
        assert_eq!(agent_code(code), code);
    }
    assert_eq!(
        RunExit::SecondaryLogon.no_sandbox(),
        Some(crate::NoSandbox::SetUpFailed(SECLOGON_DISABLED.to_owned()))
    );
    assert_eq!(
        RunExit::LogonRight.no_sandbox(),
        Some(crate::NoSandbox::SetUpBlocked(crate::Blocked::LogonRight))
    );
    assert_eq!(RunExit::Spawn.no_sandbox(), None);
    assert_eq!(SECLOGON_DISABLED, crate::windows::setup::SECLOGON_DISABLED);
}

#[test]
fn launch_files_have_the_run_folders_names() {
    assert!(is_launch_name("0123456789abcdef.json"));
    for bad in [
        "0123456789ABCDEF.json",
        "0123456789abcde.json",
        "x.json",
        "0123456789abcdef.txt",
    ] {
        assert!(!is_launch_name(bad), "{bad}");
    }
    let path = launch_path(std::path::Path::new("data"), "0123456789abcdef");
    assert!(path.ends_with(std::path::Path::new("sandbox/run/0123456789abcdef.json")));
    assert_eq!(runner_args(&path)[0], "--launch");
}

#[test]
fn the_firm_ca_files_come_from_the_launch_and_the_spec() {
    let mut spec = message();
    spec.firm_ca = vec![r"C:\Firm\proxy-ca.pem".to_owned()];
    spec.env.extend([
        (
            "ssl_cert_file".to_owned(),
            r#""C:\Firm\other.pem""#.to_owned(),
        ),
        (
            "SSL_CERT_FILE".to_owned(),
            r"c:\firm\PROXY-CA.pem".to_owned(),
        ),
        (
            "SSL_CERT_FILE".to_owned(),
            format!(r"{DATA}\sandbox-ca\roots.pem"),
        ),
    ]);
    assert_eq!(
        firm_ca_files(&spec),
        [r"C:\Firm\proxy-ca.pem", r"C:\Firm\other.pem"],
        "each once, never the bundle itself"
    );
    spec.check().unwrap();
    // Without a bundle there is nothing to merge into: the spec's SSL_CERT_FILE passes.
    spec.ca_bundle = None;
    assert_eq!(firm_ca_files(&spec), Vec::<String>::new());
    let mut bad = message();
    bad.firm_ca = vec!["relative.pem".to_owned()];
    assert!(matches!(
        bad.check(),
        Err(LaunchError::Field {
            field: "firm_ca",
            ..
        })
    ));
}
