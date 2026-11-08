use super::*;

const MARKS: PageMarks = PageMarks {
    navigation: 900,
    script: Some(600),
    render: Some(580),
    welcome: None,
    button: Some(5),
};

#[test]
fn the_targets_are_ones_host_log_keeps() {
    for target in [TARGET, PAGE_TARGET] {
        assert!(target.starts_with("clauseo_"), "{target}");
        assert!(target.ends_with("::lifecycle"), "{target}");
    }
}

#[test]
fn each_mark_goes_on_the_desktops_clock_as_the_call_less_its_age() {
    assert_eq!(
        on_clock(MARKS, 1_000),
        [Some(100), Some(400), Some(420), None, Some(995)]
    );
}

#[test]
fn a_mark_older_than_the_clock_reads_zero_never_wraps() {
    assert_eq!(on_clock(MARKS, 500)[0], Some(0));
    assert_eq!(on_clock(MARKS, 500)[1], Some(0));
}

#[test]
fn the_marks_read_as_the_page_sends_them() {
    let sent = r#"{"navigation":900,"script":600,"render":580,"welcome":null,"button":5}"#;
    assert_eq!(serde_json::from_str::<PageMarks>(sent).unwrap(), MARKS);
}

#[test]
fn a_negative_or_missing_age_is_refused() {
    let negative = r#"{"navigation":-1,"script":null,"render":null,"welcome":null,"button":null}"#;
    assert!(serde_json::from_str::<PageMarks>(negative).is_err());
    assert!(serde_json::from_str::<PageMarks>("{}").is_err());
}

#[test]
fn only_the_page_or_the_root_counts_as_the_page() {
    for path in ["", "/", "/index.html"] {
        assert!(is_page(path), "{path}");
    }
    for path in [
        "/settings.html",
        "/assets/index-1.js",
        "/index.html/x",
        "/fonts/a.woff2",
    ] {
        assert!(!is_page(path), "{path}");
    }
}
