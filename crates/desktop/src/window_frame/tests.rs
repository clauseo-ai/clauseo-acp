use super::geometry::{
    CAPTION_H, CAPTION_W, LEAD_H, LEAD_PAD, LIGHT, LIGHT_PITCH, Rect, maximise_button,
    over_maximise, traffic_lights,
};
use super::{CHROME, Chrome, WINDOW_PERMISSIONS, chrome_from};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

#[test]
fn only_room_selects_the_own_frame() {
    assert_eq!(chrome_from(Some("room")), Chrome::Own);
    for other in [
        None,
        Some(""),
        Some("Room"),
        Some("rooms"),
        Some("roo"),
        Some("internal"),
    ] {
        assert_eq!(chrome_from(other), Chrome::Os, "{other:?}");
    }
}

#[test]
fn the_switch_is_this_builds_variable_and_reruns_the_build_when_it_changes() {
    assert_eq!(CHROME, chrome_from(option_env!("VITE_CLAUSEO_WINDOW")));
    assert!(read("build.rs").contains("cargo:rerun-if-env-changed=VITE_CLAUSEO_WINDOW"));
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

/// The declarations of the first rule whose selector is exactly `selector`.
fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let head = format!("{selector} {{");
    let start = css.find(&head).unwrap_or_else(|| panic!("no {head}")) + head.len();
    &css[start..start + css[start..].find('}').unwrap()]
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
    assert!(rule(&shell, ".room-lights").contains("gap: var(--s2);"));
    let light = rule(&shell, ".room-lights i");
    assert!(light.contains("width: var(--s3);") && light.contains("height: var(--s3);"));
}

#[test]
fn main_grants_exactly_the_window_permissions_and_no_other_core_permission() {
    let capability: Value = serde_json::from_str(&read("capabilities/main.json")).unwrap();
    let core: BTreeSet<&str> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap())
        .filter(|permission| !permission.starts_with("allow-"))
        .collect();
    assert_eq!(core, WINDOW_PERMISSIONS.iter().copied().collect());
    for permission in WINDOW_PERMISSIONS {
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
