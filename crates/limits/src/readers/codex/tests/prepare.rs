//! The app-server's spec: its program, arguments, folder and environment, with `CODEX_HOME`
//! forced over the inherited and settings variables of that name.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clauseo_settings::AgentLaunch;

use crate::readers::codex::{Prepared, prepare};
use crate::write::scratch::Scratch;

const SCRIPT: &str = "install/node_modules/@zed-industries/codex-acp/bin/codex-acp.js";
const CODEX_JS: &str = "install/node_modules/@openai/codex/bin/codex.js";

fn node() -> String {
    if cfg!(windows) {
        r"C:\node\node.exe".to_owned()
    } else {
        "/usr/local/bin/node".to_owned()
    }
}

#[test]
fn codex_home_is_forced_over_inherited_and_settings_values_in_any_case() {
    let scratch = Scratch::new("codex-prepare");
    let script = scratch.file(SCRIPT, "");
    let codex_js = scratch.file(CODEX_JS, "");
    let launch = AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: BTreeMap::from([
            ("codex_home".to_owned(), "/settings/codex".to_owned()),
            (
                "OPENAI_BASE_URL".to_owned(),
                "https://example.com".to_owned(),
            ),
        ]),
    };
    let inherited = [
        ("CODEX_HOME", "/home/lawyer/.codex"),
        ("Codex_Home", "/inherited/other"),
        ("HOME", "/home/lawyer"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let run_dir = scratch.0.join("data/limits/run");
    let home = scratch.dir("data/codex-home");
    let Prepared::Spec(spec) = prepare(&launch, &run_dir, &home, inherited.into_iter()).unwrap()
    else {
        panic!("no spec");
    };
    let homes: Vec<_> = spec
        .env()
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("codex_home"))
        .collect();
    assert_eq!(
        homes,
        [(
            &OsString::from("CODEX_HOME"),
            &home.clone().into_os_string()
        )]
    );
    assert_eq!(
        spec.env().get(OsStr::new("HOME")),
        Some(&OsString::from("/home/lawyer"))
    );
    assert_eq!(
        spec.env().get(OsStr::new("OPENAI_BASE_URL")),
        Some(&OsString::from("https://example.com"))
    );
    assert_eq!(spec.program(), std::path::Path::new(&node()));
    assert_eq!(
        spec.args(),
        [codex_js.into_os_string(), OsString::from("app-server")]
    );
    assert_eq!(spec.cwd(), run_dir);
    assert!(run_dir.is_dir());
    assert_eq!(
        std::fs::read_dir(&home).unwrap().count(),
        0,
        "nothing is made in the Codex home"
    );
    let mut install: Vec<_> = std::fs::read_dir(scratch.0.join("install"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    install.sort();
    assert_eq!(
        install,
        [OsString::from("node_modules")],
        "no helper is written for Codex"
    );
}

fn codex_launch(scratch: &Scratch) -> AgentLaunch {
    let script = scratch.file(SCRIPT, "");
    scratch.file(CODEX_JS, "");
    AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: BTreeMap::new(),
    }
}

#[test]
fn a_missing_codex_home_is_signed_out_and_nothing_is_made() {
    let scratch = Scratch::new("codex-home-missing");
    let launch = codex_launch(&scratch);
    let run_dir = scratch.0.join("data/limits/run");
    let home = scratch.0.join("data/codex-home");
    let prepared = prepare(&launch, &run_dir, &home, std::iter::empty()).unwrap();
    assert!(matches!(prepared, Prepared::SignedOut), "{prepared:?}");
    assert!(!home.exists());
    assert!(!run_dir.exists());
}

#[test]
fn a_relative_codex_home_is_refused_and_nothing_is_made() {
    let scratch = Scratch::new("codex-home-relative");
    let launch = codex_launch(&scratch);
    let run_dir = scratch.0.join("data/limits/run");
    let relative = std::path::Path::new("data/codex-home");
    let err = prepare(&launch, &run_dir, relative, std::iter::empty()).unwrap_err();
    assert!(err.ends_with("is not an absolute path"), "{err}");
    assert!(!run_dir.exists());
}

#[test]
fn a_codex_home_that_is_a_file_or_a_link_is_refused() {
    let scratch = Scratch::new("codex-home-bad");
    let launch = codex_launch(&scratch);
    let run_dir = scratch.0.join("data/limits/run");
    let file = scratch.file("data/codex-home", "");
    let err = prepare(&launch, &run_dir, &file, std::iter::empty()).unwrap_err();
    assert!(err.ends_with("is not a folder"), "{err}");
    #[cfg(unix)]
    {
        let real = scratch.dir("real-home");
        let link = scratch.link("data/linked-home", &real);
        let err = prepare(&launch, &run_dir, &link, std::iter::empty()).unwrap_err();
        assert!(err.ends_with("is a link"), "{err}");
    }
    assert!(!run_dir.exists());
}
