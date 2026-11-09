//! Unit tests of the runner-stderr check. Its planted cases are in `tests/planted.rs`, and the
//! repos they run on are built in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;
use calls::{Hit, body_problem, formatted_errors, is_errorish, placeholders, scan, split};
use proc_macro2::TokenStream;

/// The tokens of a macro's text, as syn hands them over.
fn tokens(text: &str) -> TokenStream {
    text.parse().expect("tokens")
}

/// The violations the scan of `text` finds, judged outside any `Detail::new`.
fn hits(text: &str) -> Vec<Hit> {
    let mut found = Vec::new();
    scan(&tokens(text), false, &mut found);
    found
}

/// The fix of each violation the scan of `text` finds.
fn fixes(text: &str) -> Vec<Fix> {
    hits(text).into_iter().map(|hit| hit.fix).collect()
}

/// The names the format arguments of `text` (a macro's arguments) format as errors.
fn error_names(text: &str) -> Vec<String> {
    formatted_errors(&split(&tokens(text)))
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn placeholders_name_each_placeholder_but_not_an_escaped_brace() {
    assert_eq!(
        placeholders("{error} {{e}} {x:?} {0} {err:>5} {}"),
        vec!["error", "x", "0", "err", ""]
    );
}

#[test]
fn an_error_is_named_e_err_error_or_a_suffix_of_either() {
    for name in ["e", "err", "error", "io_err", "source_error"] {
        assert!(is_errorish(name), "{name}");
    }
    for name in ["errors", "ex", "kind", "error_kind", "errs", "stderr"] {
        assert!(!is_errorish(name), "{name}");
    }
}

#[test]
fn a_formatted_error_is_a_placeholder_or_a_positional_argument() {
    assert_eq!(error_names(r#""{error} {x} {{e}}", e"#), ["error", "e"]);
    assert_eq!(error_names(r#""x {}", err"#), ["err"]);
    assert_eq!(error_names(r#""x {}", &error"#), ["error"]);
    assert_eq!(error_names(r#""{e}", e = error"#), ["e", "error"]);
    assert_eq!(error_names(r#"error, "{}", e"#), ["e"]);
}

#[test]
fn a_format_argument_that_is_not_just_an_error_is_not_formatted_as_one() {
    assert_eq!(error_names(r#""x {}", error.kind()"#), Vec::<String>::new());
    assert_eq!(error_names(r#""x {}", self.error"#), Vec::<String>::new());
    assert_eq!(
        error_names(r#""x {}", e.to_string()"#),
        Vec::<String>::new()
    );
    assert_eq!(error_names(r#""x {}", count"#), Vec::<String>::new());
    assert_eq!(
        error_names("error, other"),
        Vec::<String>::new(),
        "no format string"
    );
}

#[test]
fn an_error_in_a_format_macro_is_judged_and_a_detail_argument_is_exempt() {
    assert_eq!(fixes(r#"format!("{error}")"#), [Fix::ErrorText]);
    assert_eq!(fixes(r#"format!("x {}", err)"#), [Fix::ErrorText]);
    assert_eq!(
        fixes(r#"Detail::new(|| format!("{error}"))"#),
        Vec::<Fix>::new()
    );
    assert_eq!(
        fixes(r#"Detail::new(move || format!("x {}", err))"#),
        Vec::new()
    );
    assert_eq!(
        fixes("Detail::new(move || e.to_string())"),
        Vec::<Fix>::new()
    );
}

#[test]
fn a_stderr_call_is_judged_even_inside_a_detail_argument() {
    assert_eq!(fixes("io::stderr().lock()"), [Fix::StderrCall]);
    assert_eq!(
        fixes(r#"format!("{}", std::io::stderr())"#),
        [Fix::StderrCall]
    );
    assert_eq!(fixes("Detail::new(|| io::stderr())"), [Fix::StderrCall]);
    assert_eq!(fixes("self.stderr"), Vec::<Fix>::new());
    assert_eq!(fixes("stderr"), Vec::<Fix>::new());
}

#[test]
fn a_write_to_stderr_is_judged_by_its_first_argument() {
    assert_eq!(fixes(r#"writeln!(stderr, "x")"#), [Fix::WriteStderr]);
    assert_eq!(fixes(r#"write!(self.stderr, "x")"#), [Fix::WriteStderr]);
    assert_eq!(
        fixes(r#"write!(io::stderr(), "x")"#),
        [Fix::WriteStderr, Fix::StderrCall]
    );
    assert_eq!(fixes(r#"write!(f, "{}", self.stderr)"#), Vec::<Fix>::new());
}

#[test]
fn a_console_macro_is_judged_and_a_format_macro_is_not() {
    assert_eq!(fixes(r#"eprintln!("x")"#), [Fix::Console]);
    assert_eq!(fixes(r#"print!("x")"#), [Fix::Console]);
    assert_eq!(fixes("dbg!(x)"), [Fix::Console]);
    assert_eq!(fixes(r#"format!("x")"#), Vec::<Fix>::new());
}

#[test]
fn a_to_string_on_a_bare_error_is_judged_and_a_field_or_other_method_is_not() {
    let found = hits(r#"format!("{}", e.to_string())"#);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].fix, Fix::ErrorText);
    assert!(
        found[0].message.contains("`e.to_string()`"),
        "{}",
        found[0].message
    );
    assert_eq!(
        fixes(r#"format!("{}", self.error.to_string())"#),
        Vec::new()
    );
    assert_eq!(fixes(r#"format!("{:?}", error.kind())"#), Vec::<Fix>::new());
}

#[test]
fn a_macro_rules_body_that_writes_to_the_console_or_names_stderr_is_judged() {
    assert_eq!(
        body_problem(&tokens(r#"($x:expr) => { eprintln!("{}", $x) }"#)),
        Some("calls eprintln!".to_owned())
    );
    assert_eq!(
        body_problem(&tokens("() => { stderr::refuse(K, D) }")),
        Some("names stderr".to_owned())
    );
    assert_eq!(
        body_problem(&tokens(r#"() => { tracing::warn!("x") }"#)),
        None
    );
}

#[test]
fn a_file_that_renames_detail_or_stderr_or_writes_to_the_console_has_a_violation_each() {
    let text = "use super::stderr::Detail as D;\nfn f() { println!(\"x\"); }\n";
    let found = inspect("crates/sandbox/src/windows/runner/x.rs", text);
    assert_eq!(found.len(), 2);
}

#[test]
fn a_cfg_test_module_is_skipped_and_other_cfgs_are_read() {
    let formatted = "fn f(e: E) -> String {\n    format!(\"{e}\")\n}\n";
    for attr in ["#[cfg(test)]", "#[cfg(test)] #[allow(dead_code)]"] {
        let text = format!("{attr}\nmod tests {{\n{formatted}}}\n");
        assert!(
            inspect("crates/sandbox/src/windows/runner/x.rs", &text).is_empty(),
            "{attr}"
        );
    }
    for attr in [
        "#[cfg(not(test))]",
        "#[cfg(any(test, feature = \"x\"))]",
        "#[cfg(all(test))]",
    ] {
        let text = format!("{attr}\nmod m {{\n{formatted}}}\n");
        assert_eq!(
            inspect("crates/sandbox/src/windows/runner/x.rs", &text).len(),
            1,
            "{attr}"
        );
    }
}

#[test]
fn runner_files_are_read_but_the_api_and_tests_are_not() {
    for file in [
        "crates/sandbox/src/windows/runner.rs",
        "crates/sandbox/src/windows/runner/outer.rs",
        "crates/sandbox/src/windows/runner/outer/x.rs",
        "crates/sandbox/src/bin/clauseo-sandbox-run.rs",
    ] {
        assert!(is_read(file), "{file}");
    }
    for file in [
        "crates/sandbox/src/windows/runner/stderr.rs",
        "crates/sandbox/src/windows/runner/stderr/x.rs",
        "crates/sandbox/src/windows/runner/tests.rs",
        "crates/sandbox/src/windows/runner/x_tests.rs",
        "crates/sandbox/src/windows/runner/tests/y.rs",
        "crates/sandbox/src/windows/acl.rs",
        "crates/sandbox/src/bin/other.rs",
        "crates/sandbox/src/windows/runner/notes.md",
    ] {
        assert!(!is_read(file), "{file}");
    }
}
