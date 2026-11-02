use std::borrow::Cow;
use std::path::Path;

use tauri::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use tauri::http::{HeaderValue, Response};
use tauri::test::{MockRuntime, mock_builder};

use super::{add_to_script, header_value, policy};

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

/// The page keeps the header Tauri wrote, with its own order and its nonces and hashes.
#[test]
fn the_page_keeps_the_policy_tauri_sent() {
    let sent = tauri_page_policy();
    let mut page = served("text/html", Some(&sent));
    add_to_script(&app_policy(), &mut page);
    assert_eq!(csp_of(&page), Some(sent.as_str()));
}

/// A script that already has a policy keeps it.
#[test]
fn a_script_with_a_policy_keeps_it() {
    let mut script = served("text/javascript", Some("default-src 'none'"));
    add_to_script(&app_policy(), &mut script);
    assert_eq!(csp_of(&script), Some("default-src 'none'"));
}

/// The media type before any parameter, trimmed, in any case.
#[test]
fn a_script_type_is_matched_without_its_parameters_or_case() {
    let policy = app_policy();
    for content_type in [
        "text/javascript; charset=utf-8",
        "Text/JavaScript",
        " text/javascript ;charset=utf-8",
    ] {
        let mut script = response(content_type);
        add_to_script(&policy, &mut script);
        assert_eq!(
            csp_of(&script),
            Some(configured().as_str()),
            "{content_type}"
        );
    }
}

#[test]
fn other_assets_are_left_as_they_are() {
    let policy = app_policy();
    for content_type in [
        "text/css",
        "image/png",
        "application/json",
        "application/octet-stream",
        "text/javascriptx",
        "text/html; text/javascript",
    ] {
        let mut asset = response(content_type);
        add_to_script(&policy, &mut asset);
        assert_eq!(csp_of(&asset), None, "{content_type}");
    }
    let mut untyped = Response::builder().body(Cow::Borrowed(&b""[..])).unwrap();
    add_to_script(&policy, &mut untyped);
    assert_eq!(csp_of(&untyped), None);
}

#[test]
fn a_policy_a_header_cannot_hold_gives_none() {
    assert_eq!(header_value("default-src 'self'\nscript-src *"), None);
    assert!(header_value(&configured()).is_some());
}

/// Every `.rs` file under `src/`, as (path relative to the crate, text).
fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let relative = path.strip_prefix(manifest_dir()).unwrap();
            out.push((
                relative.display().to_string().replace('\\', "/"),
                std::fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

/// Tauri keeps only a builder's last `on_web_resource_request` handler, so a second call anywhere
/// would drop the scripts' CSP without a sound. The mock runtime drops a window's protocols, so
/// no unit test can request an asset through the window: the Windows e2e smoke test checks the
/// header on the real window's scripts and on the colouring worker.
#[test]
fn csp_rs_is_the_only_place_that_sets_a_web_resource_handler() {
    let mut files = Vec::new();
    sources(&manifest_dir().join("src"), &mut files);
    let callers: Vec<&str> = files
        .iter()
        .filter(|(path, text)| {
            !path.ends_with("csp/tests.rs") && text.contains("on_web_resource_request")
        })
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(callers, ["src/csp.rs"]);
    let lib = &files
        .iter()
        .find(|(path, _)| path == "src/lib.rs")
        .unwrap()
        .1;
    assert_eq!(
        lib.matches("csp::on_scripts(").count(),
        1,
        "lib.rs installs the hook once"
    );
}
