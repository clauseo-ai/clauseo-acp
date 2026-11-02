use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tauri::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use tauri::http::{HeaderValue, Response};
use tauri::test::{MockRuntime, mock_builder};

use super::{add_to_script, policy};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `app.security.csp` as `tauri.conf.json` writes it, read from the file and not through Tauri.
fn configured() -> String {
    let text = std::fs::read_to_string(manifest_dir().join("tauri.conf.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&text).unwrap();
    config["app"]["security"]["csp"]
        .as_str()
        .expect("tauri.conf.json sets app.security.csp as one string")
        .to_owned()
}

/// A policy as directive → sources, so two spellings of one policy compare equal.
fn directives(policy: &str) -> BTreeMap<String, BTreeSet<String>> {
    policy
        .split(';')
        .filter_map(|directive| {
            let mut words = directive.split_whitespace();
            let name = words.next()?.to_owned();
            Some((name, words.map(str::to_owned).collect()))
        })
        .collect()
}

/// The policy the app's own context gives the hook.
fn app_policy() -> HeaderValue {
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    policy(&app).expect("the app's config has a CSP")
}

/// Tauri 2.12.4's page policy for a page with one inline script: the configured directives in
/// its own order (it parses them into a map), with the script's hash added to `script-src`.
fn tauri_page_policy() -> String {
    let mut directives: Vec<String> = configured().split("; ").map(str::to_owned).collect();
    directives.reverse();
    for directive in &mut directives {
        if directive.starts_with("script-src ") {
            directive.push_str(" 'sha256-stand-in'");
        }
    }
    directives.join("; ")
}

/// A response as Tauri's protocol builds it (2.12.4 `protocol/tauri.rs`): the asset's content
/// type, and the CSP `get_asset` gives `.html` assets only. The mock runtime drops a window's
/// protocols, so no unit test can ask Tauri for one.
fn served(content_type: &str, csp: Option<&str>) -> Response<Cow<'static, [u8]>> {
    let mut builder = Response::builder().header(CONTENT_TYPE, content_type);
    if let Some(csp) = csp {
        builder = builder.header("Content-Security-Policy", csp);
    }
    builder.body(Cow::Borrowed(&b""[..])).unwrap()
}

fn response(content_type: &str) -> Response<Cow<'static, [u8]>> {
    served(content_type, None)
}

fn csp_of<'a>(response: &'a Response<Cow<'static, [u8]>>) -> Option<&'a str> {
    response
        .headers()
        .get(CONTENT_SECURITY_POLICY)
        .map(|value| value.to_str().unwrap())
}

#[test]
fn the_policy_is_the_one_tauri_conf_json_writes() {
    assert_eq!(app_policy().to_str().unwrap(), configured());
}

/// The worker's script (`text/javascript`, as Tauri types `.js` and `.mjs`) carries the
/// configured policy exactly.
#[test]
fn the_worker_script_gets_the_configured_policy() {
    let mut worker = response("text/javascript");
    add_to_script(&app_policy(), &mut worker);
    assert_eq!(csp_of(&worker), Some(configured().as_str()));
}

/// The page keeps the header Tauri wrote: the same directives, with Tauri's nonces and hashes.
#[test]
fn the_page_keeps_the_policy_tauri_sent() {
    let sent = tauri_page_policy();
    let mut page = served("text/html", Some(&sent));
    add_to_script(&app_policy(), &mut page);
    assert_eq!(csp_of(&page), Some(sent.as_str()));
    let mut expected = directives(&configured());
    expected
        .get_mut("script-src")
        .unwrap()
        .insert("'sha256-stand-in'".to_owned());
    assert_eq!(directives(csp_of(&page).unwrap()), expected);
}

/// A script that already has a policy keeps it.
#[test]
fn a_script_with_a_policy_keeps_it() {
    let mut script = served("text/javascript", Some("default-src 'none'"));
    add_to_script(&app_policy(), &mut script);
    assert_eq!(csp_of(&script), Some("default-src 'none'"));
}

#[test]
fn a_script_with_a_charset_gets_the_policy_too() {
    let mut script = response("text/javascript; charset=utf-8");
    add_to_script(&app_policy(), &mut script);
    assert_eq!(csp_of(&script), Some(configured().as_str()));
}

#[test]
fn other_assets_are_left_as_they_are() {
    let policy = app_policy();
    for content_type in [
        "text/css",
        "image/png",
        "application/json",
        "application/octet-stream",
    ] {
        let mut asset = response(content_type);
        add_to_script(&policy, &mut asset);
        assert_eq!(csp_of(&asset), None, "{content_type}");
    }
    let mut untyped = Response::builder().body(Cow::Borrowed(&b""[..])).unwrap();
    add_to_script(&policy, &mut untyped);
    assert_eq!(csp_of(&untyped), None);
}

/// The mock runtime drops a window's protocols, so no unit test can request an asset through the
/// window; this holds the one window to the hook instead. The Windows e2e smoke test reads the
/// header on the scripts the real window loads.
#[test]
fn the_main_window_is_built_with_the_hook() {
    let lib = std::fs::read_to_string(manifest_dir().join("src/lib.rs")).unwrap();
    let start = lib
        .find("fn open_main_window(")
        .expect("lib.rs builds the main window");
    let body = &lib[start..];
    let body = &body[..body.find("\n}\n").expect("open_main_window ends")];
    let hook = body
        .find("csp::on_scripts(builder, app)")
        .expect("open_main_window adds the scripts' CSP (csp::on_scripts)");
    let built = body
        .find(".build()")
        .expect("open_main_window builds the window");
    assert!(hook < built, "the hook goes on before the window is built");
}
