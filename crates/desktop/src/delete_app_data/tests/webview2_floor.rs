//! The template's `Section WebView2` with a runtime older than `minimumWebview2Version`, run on a
//! fake registry (`nsis_run.rs`): per user it never runs Edge Update's `needsadmin=true` update,
//! which for a standard user is a UAC credential prompt, and only says so in the details list
//! (block `webview2-no-elevation`); per machine, elevated already, it updates, and a failed
//! update no longer quits the install. A missing runtime gets the bootstrapper, with no
//! `needsadmin`.

use std::collections::HashMap;

use super::nsis_run::{Machine, Record, Value, defines, parse_all, preprocess};
use super::sources::read;
use super::template::{TEMPLATE, block_code, split_blocks};

/// The upstream lines `webview2-no-elevation` stands in for: tauri-cli 2.12.2's update of an
/// installed runtime older than the floor, with its Abort/Retry/Ignore box and `Quit`, through
/// the end of its `${If} $R1 != ""` (the block closes that `${If}` itself).
pub(super) const UPSTREAM_UPDATE: [&str; 22] = [
    "        update_webview:",
    "          DetailPrint \"$(installingWebview2)\"",
    "          ${If} ${RunningX64}",
    r#"            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path""#,
    "          ${Else}",
    r#"            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path""#,
    "          ${EndIf}",
    "          ${If} $R1 == \"\"",
    r#"            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path""#,
    "          ${EndIf}",
    "          ${If} $R1 != \"\"",
    "            ; Chromium updater docs: \
     https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md",
    r#"            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath""#,
    "            ExecWait `\"$R1\" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1",
    "            ${If} $1 = 0",
    "              DetailPrint \"$(webview2InstallSuccess)\"",
    "            ${Else}",
    "              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE \"$(webview2InstallError)\" \
     IDIGNORE ignore IDRETRY update_webview",
    "              Quit",
    "              ignore:",
    "            ${EndIf}",
    "          ${EndIf}",
];

const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const EDGE_UPDATE: &str = r"C:\Program Files (x86)\Microsoft\EdgeUpdate\MicrosoftEdgeUpdate.exe";

/// The installed runtime's key, as the section reads it first on 64-bit Windows.
fn clients_key() -> String {
    format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{GUID}")
}

/// `Section WebView2`'s code lines as the bundler renders them for `mode`, with the repo's floor
/// and its bootstrapper mode, kept as `!if` selects them.
fn section(mode: &str) -> (HashMap<String, String>, Vec<String>) {
    let text = read(TEMPLATE);
    let code: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect();
    let start = code.iter().position(|l| l == "Section WebView2").unwrap();
    let end = start
        + code[start..]
            .iter()
            .position(|l| l == "SectionEnd")
            .unwrap();
    let mut defines = defines(&code);
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    let floor = conf["bundle"]["windows"]["minimumWebview2Version"]
        .as_str()
        .unwrap();
    for (name, value) in [
        ("INSTALLMODE", mode),
        ("MINIMUMWEBVIEW2VERSION", floor),
        ("INSTALLWEBVIEW2MODE", "downloadBootstrapper"),
        ("WEBVIEW2INSTALLERARGS", "/silent"),
    ] {
        defines.insert(name.to_owned(), value.to_owned());
    }
    assert_eq!(defines["WEBVIEW2APPGUID"], GUID);
    let body = preprocess(&code[start + 1..end], &defines);
    (defines, body)
}

/// Runs the section for `mode` with the runtime at `version` and Edge Update at `path` (both
/// HKLM, 64-bit Windows), every `ExecWait` leaving `exit_code`: what it said.
fn run(mode: &str, version: &'static str, path: Record, exit_code: &'static str) -> Vec<String> {
    let (defines, body) = section(mode);
    let mut machine = Machine::new(defines, None);
    machine.exit_code = exit_code;
    machine
        .keys
        .insert(clients_key(), Some(vec![("pv", Value::Text(version))]));
    machine.keys.insert(
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate".to_owned(),
        path,
    );
    machine.run(&parse_all(&body));
    assert!(machine.reached.is_empty(), "{:?}", machine.reached);
    machine.said
}

fn edge_update() -> Vec<(&'static str, Value)> {
    vec![("path", Value::Text(EDGE_UPDATE))]
}

#[test]
fn per_user_a_runtime_below_the_floor_is_left_to_windows_with_one_details_line() {
    // The floor is 111.0.1661.41: an older runtime, even one just below.
    for version in ["110.0.1587.69", "111.0.1661.40"] {
        let said = run("currentUser", version, Some(edge_update()), "0");
        assert_eq!(said, ["DetailPrint $(webview2TooOld)"], "{version}");
    }
    // No line of the per-user section can start Edge Update's elevated update.
    let (_, body) = section("currentUser");
    assert!(
        !body.iter().any(|line| line.contains("needsadmin")),
        "{body:?}"
    );
    assert!(!body.iter().any(|line| line == "Quit"), "{body:?}");
}

#[test]
fn per_user_a_runtime_at_or_above_the_floor_needs_nothing() {
    for version in ["111.0.1661.41", "120.0.2210.91", "160.0.0.0"] {
        assert_eq!(
            run("currentUser", version, Some(edge_update()), "0"),
            Vec::<String>::new(),
            "{version}"
        );
    }
}

#[test]
fn per_machine_a_runtime_below_the_floor_is_updated_and_a_failed_update_goes_on() {
    let update = format!("ExecWait \"{EDGE_UPDATE}\" /install appguid={GUID}&needsadmin=true");
    let said = run("perMachine", "110.0.1587.69", Some(edge_update()), "0");
    assert_eq!(
        said,
        [
            "DetailPrint $(installingWebview2)",
            update.as_str(),
            "DetailPrint $(webview2InstallSuccess)",
        ]
    );
    // Failed (the VM's -2147219689): two details lines, no box, no Quit; the install goes on.
    let said = run(
        "perMachine",
        "110.0.1587.69",
        Some(edge_update()),
        "-2147219689",
    );
    assert_eq!(
        said,
        [
            "DetailPrint $(installingWebview2)",
            update.as_str(),
            "DetailPrint $(webview2InstallError)",
            "DetailPrint $(webview2TooOld)",
        ]
    );
    // At the floor, nothing.
    assert_eq!(
        run("perMachine", "111.0.1661.41", Some(edge_update()), "0"),
        Vec::<String>::new()
    );
}

#[test]
fn the_no_elevation_block_holds_the_per_user_branch_first() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    let code = block_code(&blocks, "webview2-no-elevation");
    assert_eq!(
        code[..3],
        [
            "!if \"${INSTALLMODE}\" == \"currentUser\"",
            "DetailPrint \"$(webview2TooOld)\"",
            "!else",
        ]
    );
    assert!(!code.iter().any(|line| line.starts_with("MessageBox")));
    assert!(!code.contains(&"Quit"));
}

#[test]
fn a_missing_runtime_gets_the_silent_bootstrapper_with_no_needsadmin() {
    // tauri-cli 2.12.2 renders WEBVIEW2INSTALLERARGS as "/silent" for a silent bootstrapper, ""
    // otherwise (tauri-bundler nsis/mod.rs, webview2_installer_args); nothing adds needsadmin.
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(
        conf["bundle"]["windows"]["webviewInstallMode"],
        serde_json::json!({ "type": "downloadBootstrapper", "silent": true })
    );
    let (_, body) = section("currentUser");
    let installs: Vec<&String> = body.iter().filter(|l| l.starts_with("ExecWait")).collect();
    assert_eq!(
        installs,
        ["ExecWait \"$6 ${WEBVIEW2INSTALLERARGS} /install\" $1"]
    );
    let text = read(TEMPLATE);
    assert!(text.contains("!define WEBVIEW2INSTALLERARGS \"{{webview2_installer_args}}\""));
}
