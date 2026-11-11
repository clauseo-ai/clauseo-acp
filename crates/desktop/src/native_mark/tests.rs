use super::{
    DAY_PRESS, HEAD, NIGHT_PRESS, Placement, TAIL, VIEW_HEIGHT, VIEW_LEFT, VIEW_TOP, VIEW_WIDTH,
    colours, outline, placement,
};
use crate::window_size::{DAY_GROUND, NIGHT_GROUND};

/// `ui/app/index.html`, whose first paint the native mark copies.
const INDEX_HTML: &str = include_str!("../../../../ui/app/index.html");

/// The app's colour tokens, `--ground-press` among them.
const TOKENS: &str = include_str!("../../../../ui/components/src/styles/tokens-colour.css");

/// `actual` is `expected` to within a thousandth of a pixel.
fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 0.001
}

/// The `#RRGGBB` comments of `name`'s declarations in the tokens, in file order.
fn token_hexes(name: &str) -> Vec<String> {
    TOKENS
        .lines()
        .filter(|line| line.trim_start().starts_with(&format!("{name}:")))
        .filter_map(|line| line.split("/* #").nth(1))
        .map(|rest| rest.chars().take(6).collect())
        .collect()
}

/// `hex`, six hex digits, as RGB.
fn rgb(hex: &str) -> (u8, u8, u8) {
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    (byte(0), byte(2), byte(4))
}

#[test]
fn the_comma_is_ground_press_on_the_ground_of_the_theme() {
    let presses = token_hexes("--ground-press");
    assert_eq!(
        presses,
        ["D2CDC5", "292725"],
        "--ground-press lines of tokens-colour.css with a hex comment: day, then night"
    );
    assert_eq!(rgb(&presses[0]), DAY_PRESS);
    assert_eq!(rgb(&presses[1]), NIGHT_PRESS);
    assert_eq!(colours(false), (DAY_GROUND, DAY_PRESS));
    assert_eq!(colours(true), (NIGHT_GROUND, NIGHT_PRESS));
}

#[test]
fn the_comma_is_index_html_s_in_shape_size_and_colour() {
    let d = INDEX_HTML
        .split(" d=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the first mark's path in index.html");
    let tokens: Vec<&str> = d.split_whitespace().collect();
    let commands: String = tokens
        .iter()
        .filter(|token| token.parse::<f32>().is_err())
        .copied()
        .collect();
    assert_eq!(
        commands, "MCCLCCCCZ",
        "index.html's path: M, 2 C, L, 4 C, Z"
    );
    let numbers: Vec<f32> = tokens.iter().filter_map(|t| t.parse().ok()).collect();
    let ours: Vec<f32> = HEAD
        .iter()
        .chain(&TAIL)
        .flat_map(|&(x, y)| [x, y])
        .collect();
    assert_eq!(
        numbers, ours,
        "HEAD then TAIL are index.html's path, point by point"
    );
    assert!(INDEX_HTML.contains(&format!(
        "viewBox=\"{VIEW_LEFT} {VIEW_TOP} {VIEW_WIDTH} {VIEW_HEIGHT}\""
    )));
    assert!(INDEX_HTML.contains("#first-mark svg {\n        width: 40px;"));
    assert!(INDEX_HTML.contains("color: #d2cdc5;"));
    assert!(INDEX_HTML.contains("color: #292725;"));
}

#[test]
fn at_96_dpi_the_comma_is_40_px_wide_and_centred() {
    let at = placement((1280, 800), 96);
    let height = VIEW_HEIGHT * at.scale;
    assert!(near(VIEW_WIDTH * at.scale, 40.0));
    assert!(near(height, 40.0 * 695.0 / 411.0));
    assert!(near(at.left, 620.0));
    assert!(near(at.top, (800.0 - height) / 2.0));
}

#[test]
fn at_144_dpi_the_comma_is_60_px_wide_and_centred() {
    let at = placement((1920, 1200), 144);
    assert!(near(VIEW_WIDTH * at.scale, 60.0));
    assert!(near(at.left, 930.0));
    assert!(near(at.top + VIEW_HEIGHT * at.scale / 2.0, 600.0));
    assert_eq!(placement((1920, 1200), 144), at);
}

#[test]
fn a_failed_dpi_counts_as_96_and_a_negative_client_as_empty() {
    assert_eq!(placement((1280, 800), 0), placement((1280, 800), 96));
    let empty = placement((-5, -5), 96);
    assert!(near(empty.left, -20.0));
    assert_eq!(empty, placement((0, 0), 96));
}

#[test]
fn the_outline_stays_in_its_box_and_closes() {
    let at = placement((1280, 800), 144);
    let (head, tail) = outline(at);
    let (right, bottom) = (
        at.left + VIEW_WIDTH * at.scale,
        at.top + VIEW_HEIGHT * at.scale,
    );
    for (x, y) in head.iter().chain(&tail) {
        assert!(
            (at.left..=right).contains(x),
            "{x} in {}..={right}",
            at.left
        );
        assert!(
            (at.top..=bottom).contains(y),
            "{y} in {}..={bottom}",
            at.top
        );
    }
    assert_eq!(tail[12], head[0], "the path ends where it began");
    let Placement { scale, left, top } = at;
    let (x, y) = head[0];
    assert!(near(x, left + (8162.05 - VIEW_LEFT) * scale));
    assert!(near(y, top + (1149.65 - VIEW_TOP) * scale));
}
