use super::geometry::{
    CAPTION_H, CAPTION_W, LEAD_H, LEAD_PAD, LIGHT, LIGHT_PITCH, NATIVE_BUTTON_W, NATIVE_PITCH,
    Rect, maximise_button, over_maximise, traffic_lights,
};
use super::{CAPTION_PERMISSIONS, DRAG_PERMISSIONS, add_frame_capability};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

#[test]
fn the_maximise_button_is_the_middle_caption_button_at_the_top_right() {
    let at_1x = maximise_button(1280, 1.0);
    assert_eq!(
        at_1x,
        Rect {
            left: 1188,
            top: 0,
            right: 1234,
            bottom: 32
        }
    );
    let at_150 = maximise_button(1920, 1.5);
    assert_eq!(
        at_150,
        Rect {
            left: 1782,
            top: 0,
            right: 1851,
            bottom: 48
        }
    );
    assert_eq!((at_150.width(), at_150.height()), (69, 48));
    // 125%: 57.5 rounds to 58, 115 stays.
    assert_eq!(
        maximise_button(1600, 1.25),
        Rect {
            left: 1485,
            top: 0,
            right: 1542,
            bottom: 40
        }
    );
}

#[test]
fn a_point_is_over_the_maximise_button_only_inside_it() {
    let width = 1280;
    assert!(over_maximise(1188, 0, width, 1.0));
    assert!(over_maximise(1233, 31, width, 1.0));
    // Minimise's last column, close's first, and the row under the buttons.
    assert!(!over_maximise(1187, 10, width, 1.0));
    assert!(!over_maximise(1234, 10, width, 1.0));
    assert!(!over_maximise(1200, 32, width, 1.0));
    assert!(!over_maximise(1200, -1, width, 1.0));
}

#[test]
fn a_window_narrower_than_the_buttons_or_a_bad_scale_gives_no_odd_rectangle() {
    let empty = maximise_button(80, 1.0);
    assert_eq!((empty.width(), empty.height()), (0, 0));
    assert!(!over_maximise(0, 0, 80, 1.0));
    for bad in [f64::NAN, 0.0, -2.0, f64::INFINITY] {
        assert_eq!(
            maximise_button(1280, bad),
            maximise_button(1280, 1.0),
            "{bad}"
        );
    }
}

#[test]
fn the_traffic_lights_stand_on_the_first_corner_light() {
    // The close button's 14 by 16 frame, its light centred on the design's 12 px one (20 px in,
    // centred in 52): its top 18 pt down, plus the 10 pt the inset keeps under it.
    assert_eq!(traffic_lights(), (19.0, 28.0));
    assert_eq!(super::TRAFFIC_LIGHTS, traffic_lights());
}

/// The value of `--<name>: <n>px;` in `css`.
fn token_px(css: &str, name: &str) -> f64 {
    let key = format!("--{name}:");
    let start = css.find(&key).unwrap_or_else(|| panic!("no {key}")) + key.len();
    let value = css[start..].split(';').next().unwrap().trim();
    value
        .strip_suffix("px")
        .unwrap_or_else(|| panic!("{key} {value}"))
        .parse()
        .unwrap()
}

/// The declarations of the first rule whose selector is exactly `selector`, alone on its line
/// (so `.room-top` is not found inside `:root[data-frame="own"] .room-top`).
fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let head = format!("{selector} {{");
    let mut at = 0;
    for line in css.split_inclusive('\n') {
        if line.trim() == head {
            let start = at + line.len();
            return &css[start..start + css[start..].find('}').unwrap()];
        }
        at += line.len();
    }
    panic!("no {head}")
}

#[test]
fn the_geometry_follows_the_toolbars_tokens() {
    let tokens = read("../../ui/components/src/styles/tokens-parts.css");
    assert_eq!(token_px(&tokens, "size-caption-w"), CAPTION_W);
    assert_eq!(token_px(&tokens, "size-caption-h"), CAPTION_H);
    assert_eq!(token_px(&tokens, "lead-h"), LEAD_H);
    assert_eq!(token_px(&tokens, "s5"), LEAD_PAD);
    assert_eq!(token_px(&tokens, "s3"), LIGHT);
    assert_eq!(
        token_px(&tokens, "s3") + token_px(&tokens, "s2"),
        LIGHT_PITCH
    );

    let shell = read("../../ui/components/src/clauseo/shell.css");
    let lead = rule(&shell, ".room-lead");
    assert!(lead.contains("padding-left: var(--s5);"), "{lead}");
    assert!(lead.contains("height: var(--lead-h);"), "{lead}");
    assert!(lead.contains("align-items: center;"), "{lead}");
    // The lead keeps to the toolbar's top when a larger text size grows the toolbar.
    assert!(lead.contains("align-self: flex-start;"), "{lead}");
    assert!(rule(&shell, ".room-lights").contains("gap: var(--s2);"));
    // The box kept for the native lights runs from the lead's padding to the last light's edge.
    let (x, _) = traffic_lights();
    let native_end = x + 2.0 * NATIVE_PITCH + NATIVE_BUTTON_W;
    assert_eq!(token_px(&tokens, "lights-native-w"), native_end - LEAD_PAD);
    assert!(rule(&shell, ".room-lights[data-native]").contains("width: var(--lights-native-w);"));
    let light = rule(&shell, ".room-lights i");
    assert!(light.contains("width: var(--s3);") && light.contains("height: var(--s3);"));
}

/// The native maximise window is flush with the window's top right (`maximise_button`). The page
/// puts its caption buttons there by these rules: the buttons at the top of the toolbar, its
/// right padding taken back on a wide window (`mid:pr-0`), and on a narrow one the toolbar's own
/// padding (`--s3` above, `--s4` at the right) taken back by negative margins.
#[test]
fn the_page_puts_its_caption_buttons_where_the_native_window_stands() {
    let toolbar = read("../../ui/components/src/clauseo/room-toolbar.tsx");
    assert!(
        toolbar.contains("self-start mid:ml-s5 max-mid:order-4 max-mid:-mt-s3 max-mid:-mr-s4"),
        "room-toolbar.tsx no longer places CaptionButtons flush top right"
    );
    assert!(
        toolbar.contains(r#"captions && "mid:pr-0""#),
        "room-toolbar.tsx"
    );
    let responsive = read("../../ui/components/src/clauseo/responsive.css");
    let narrow = rule(&responsive, ".room-top");
    assert!(
        narrow.contains(
            "padding: calc(var(--s3) + env(safe-area-inset-top)) calc(var(--s4) + env(safe-area-inset-right))"
        ),
        "{narrow}"
    );
}

/// `file`'s capability, after checking it grants to `windows` alone, and its core permissions.
fn core_permissions(file: &str, windows: &[&str]) -> (Value, BTreeSet<String>) {
    let capability: Value = serde_json::from_str(&read(file)).unwrap();
    assert_eq!(capability["windows"], json!(windows), "{file}");
    let core = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap().to_owned())
        .filter(|permission| !permission.starts_with("allow-"))
        .collect();
    (capability, core)
}

fn set(list: &[&str]) -> BTreeSet<String> {
    list.iter()
        .map(|permission| (*permission).to_owned())
        .collect()
}

/// Both windows have the room's frame (0028): each grants the drag pair, and frame.json the
/// caption buttons' four to both.
#[test]
fn main_and_settings_grant_the_drag_permissions_and_frame_the_caption_ones_on_windows_and_linux() {
    let (_main, core) = core_permissions("capabilities/main.json", &["main"]);
    assert_eq!(core, set(DRAG_PERMISSIONS), "capabilities/main.json");
    let (_settings, core) = core_permissions("capabilities/settings.json", &["settings"]);
    assert_eq!(core, set(DRAG_PERMISSIONS), "capabilities/settings.json");
    let (frame, core) = core_permissions("capabilities/frame.json", &["main", "settings"]);
    assert_eq!(core, set(CAPTION_PERMISSIONS), "capabilities/frame.json");
    assert_eq!(frame["platforms"], json!(["windows", "linux"]));
    // main and settings are in the context; grant_caption_commands adds frame at setup.
    let config: Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(
        config["app"]["security"]["capabilities"],
        json!(["main", "settings"])
    );
    for permission in DRAG_PERMISSIONS.iter().chain(CAPTION_PERMISSIONS) {
        // One command each, of the window plugin: no default set, no scope, nothing that sets a
        // window's properties or reaches another plugin.
        let command = permission.strip_prefix("core:window:allow-").unwrap();
        assert!(
            !command.starts_with("set-") && !command.contains("default"),
            "{permission}"
        );
    }
    // The e2e and marks capabilities stay command-only.
    for file in ["capabilities/e2e.json", "capabilities/marks.json"] {
        assert!(!read(file).contains("core:"), "{file}");
    }
}

/// The app's own origin (`commands/tests.rs` says why it differs).
#[cfg(windows)]
const LOCAL_ORIGIN: &str = "https://tauri.localhost";
#[cfg(not(windows))]
const LOCAL_ORIGIN: &str = "tauri://localhost";

/// Invokes the window plugin's `command` from `window`'s page.
fn invoke(window: &WebviewWindow<MockRuntime>, command: &str) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: format!("plugin:window|{command}"),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: LOCAL_ORIGIN.parse().unwrap(),
        body: InvokeBody::Json(json!({})),
        headers: tauri::http::HeaderMap::default(),
        invoke_key: INVOKE_KEY.to_owned(),
    };
    get_ipc_response(window, request).map(|body| match body {
        InvokeResponseBody::Json(text) => serde_json::from_str(&text).unwrap(),
        InvokeResponseBody::Raw(_) => panic!("a raw answer"),
    })
}

/// Whether Tauri refused `answer` before any command ran.
fn denied(answer: &Result<Value, Value>) -> bool {
    answer
        .as_ref()
        .err()
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("not allowed"))
}

/// Through Tauri's own access check, on the mock runtime with the app's context: the drag
/// commands answer in every build, the caption commands only once `frame.json` is added and
/// never on the Mac, and a command no capability names never.
#[test]
fn the_caption_commands_answer_only_with_the_frame_capability() {
    for label in ["main", "settings"] {
        caption_commands_answer_only_with_the_frame_capability(label);
    }
}

/// [`the_caption_commands_answer_only_with_the_frame_capability`] for the window `label`.
fn caption_commands_answer_only_with_the_frame_capability(label: &str) {
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, label, WebviewUrl::default())
        .build()
        .unwrap();
    for command in ["start_dragging", "internal_toggle_maximize"] {
        let answer = invoke(&window, command);
        assert!(!denied(&answer), "{command}: {answer:?}");
    }
    // Close is left out: once granted it would close the window under the test.
    let caption = ["minimize", "toggle_maximize", "is_maximized"];
    for command in caption.iter().chain(&["close"]) {
        let answer = invoke(&window, command);
        assert!(denied(&answer), "{command} before frame.json: {answer:?}");
    }
    add_frame_capability(&app).unwrap();
    for command in caption {
        let answer = invoke(&window, command);
        assert_eq!(
            denied(&answer),
            cfg!(target_os = "macos"),
            "{command} with frame.json: {answer:?}"
        );
    }
    for command in [
        "set_size",
        "set_position",
        "set_decorations",
        "destroy",
        "hide",
    ] {
        let answer = invoke(&window, command);
        assert!(denied(&answer), "{command}: {answer:?}");
    }
}

/// The source files under `dir`, tests left out, with the text of each.
fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && name != "tests.rs"
            && name != "test_support.rs"
        {
            let relative = path.strip_prefix(manifest_dir()).unwrap();
            out.push((
                relative.display().to_string(),
                std::fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

/// Tauri's window commands take a `label` and act on any window of the app, so a page's grant
/// reaches every window there is. The app builds `main` (`lib.rs`) and `settings`
/// (`settings_window.rs`), whose mutual reach 0028 records as harmless; any other window needs
/// a record first.
#[test]
fn main_and_settings_are_the_only_windows_the_app_builds() {
    let mut files = Vec::new();
    sources(&manifest_dir().join("src"), &mut files);
    let builders: Vec<String> = files
        .iter()
        .flat_map(|(path, text)| {
            // Every way tauri 2.12.4 builds a window or a webview.
            let count = [
                "WindowBuilder::",
                "WebviewBuilder::",
                "WebviewWindow::builder(",
                "Window::builder(",
                "Webview::builder(",
            ]
            .iter()
            .map(|spelling| text.matches(spelling).count())
            .sum::<usize>();
            std::iter::repeat_n(path.clone(), count)
        })
        .collect();
    assert_eq!(
        builders.len(),
        2,
        "{builders:?}: the app builds a window besides main and settings. The pages' window \
         commands (capabilities/main.json, settings.json, frame.json) take a label and would \
         reach it: read docs/decisions/0015-window-frame.md and 0028-settings-window.md, and \
         give the new window label-free commands or record why reaching it is harmless"
    );
    let mut places: Vec<&str> = builders.iter().map(String::as_str).collect();
    places.sort_unstable();
    assert!(
        places[0].ends_with("lib.rs") && places[1].ends_with("settings_window.rs"),
        "{builders:?}"
    );
    let lib = read("src/lib.rs");
    assert!(lib.contains("WebviewWindowBuilder::new(app, MAIN_WINDOW,"));
    let settings = read("src/settings_window.rs");
    assert!(settings.contains("WebviewWindowBuilder::new(manager, SETTINGS_WINDOW,"));
}
