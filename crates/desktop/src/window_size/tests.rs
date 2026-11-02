use super::{DAY_GROUND, NIGHT_GROUND, WINDOW_SIZE, fitted, ground, state_denylist};
use std::path::Path;

/// `actual` and `expected` are equal to within a hundredth of a pixel.
fn close(actual: (f64, f64), expected: (f64, f64)) -> bool {
    (actual.0 - expected.0).abs() < 0.01 && (actual.1 - expected.1).abs() < 0.01
}

#[test]
fn no_monitor_opens_at_the_room_size() {
    assert_eq!(fitted(None), (1280.0, 800.0));
    assert_eq!(WINDOW_SIZE, (1280.0, 800.0));
}

#[test]
fn a_large_work_area_opens_at_the_room_size() {
    assert_eq!(fitted(Some((1920.0, 1080.0))), (1280.0, 800.0));
}

#[test]
fn a_small_work_area_takes_ninety_percent_per_dimension() {
    assert!(close(fitted(Some((1366.0, 728.0))), (1229.4, 655.2)));
    assert!(close(fitted(Some((1024.0, 738.0))), (921.6, 664.2)));
}

#[test]
fn the_state_denylist_holds_main_only_in_the_e2e_build() {
    if cfg!(feature = "e2e") {
        assert_eq!(state_denylist(), &["main"]);
    } else {
        assert_eq!(state_denylist(), [] as [&str; 0]);
    }
}

/// The 6 hex digits after `/* #` on each line that declares `--ground:` with its hex comment in
/// `tokens-colour.css`, in file order: the day value, then the night one. The operating system's
/// day values at night carry no comment and are left out.
fn css_grounds() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/components/src/styles/tokens-colour.css");
    let css = std::fs::read_to_string(path).unwrap();
    css.lines()
        .filter(|line| line.trim_start().starts_with("--ground:") && line.contains("/* #"))
        .map(|line| line.split("/* #").nth(1).unwrap()[..6].to_owned())
        .collect()
}

/// `RRGGBB` as the three bytes.
fn rgb(hex: &str) -> (u8, u8, u8) {
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    (byte(0), byte(2), byte(4))
}

#[test]
fn the_ground_colours_are_the_tokens_hex_comments() {
    let grounds = css_grounds();
    assert_eq!(
        grounds,
        ["E5E2DC", "1B1A18"],
        "--ground lines of tokens-colour.css: day, then night"
    );
    assert_eq!(rgb(&grounds[0]), DAY_GROUND);
    assert_eq!(rgb(&grounds[1]), NIGHT_GROUND);
    assert_eq!(ground(false), DAY_GROUND);
    assert_eq!(ground(true), NIGHT_GROUND);
}
