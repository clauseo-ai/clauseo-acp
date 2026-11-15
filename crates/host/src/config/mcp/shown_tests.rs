//! The shown text: quoting and escaping make it longer than the raw text, and the counts are the
//! shown text's, never the raw text's.

use super::{McpLaunchAsk, ask_chars, import_chars, import_list, import_names, left_out, program};

/// An argument that grows when shown: a quote, a backslash, a newline, a bidi override, and a
/// long letter outside ASCII.
const GROWS: &str = "it's \\ \n\u{202e}é𝔄";

fn local(server: &str, program: &[&str], names: &[&str]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: program.iter().map(|part| (*part).to_owned()).collect(),
        url: None,
        names: names.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn quoting_and_escaping_are_counted_as_shown() {
    let shown = program(&["tool".to_owned(), GROWS.to_owned()]);
    assert_eq!(shown, "tool \"it's \\\\ \\\\n\\\\u{202e}é𝔄\"");
    let ask = local("s", &["tool", GROWS], &["A\tB"]);
    let raw = 4 + GROWS.chars().count() + 3;
    assert_eq!(
        ask_chars(&ask),
        shown.chars().count() + "A\\tB".chars().count()
    );
    assert!(ask_chars(&ask) > raw);
}

#[test]
fn a_url_and_its_names_are_counted_as_shown() {
    let ask = McpLaunchAsk {
        server: "notes".to_owned(),
        program: Vec::new(),
        url: Some("https://notes.example/\u{200b}mcp".to_owned()),
        names: vec!["Authorization".to_owned()],
    };
    let shown = "https://notes.example/\\u{200b}mcp";
    assert_eq!(ask_chars(&ask), shown.chars().count() + 13);
}

#[test]
fn an_imports_count_is_its_list_and_names_line_as_shown() {
    let asks = [
        local("a", &["tool", GROWS], &["TOKEN"]),
        local("b", &["npx"], &[]),
    ];
    let list = import_list(&asks);
    let names = import_names(&asks);
    assert_eq!(list, "a (tool \"it's \\\\ \\\\n\\\\u{202e}é𝔄\"), b (npx)");
    assert_eq!(names, "TOKEN (a)");
    assert_eq!(
        import_chars(&asks),
        list.chars().count() + names.chars().count()
    );
}

#[test]
fn left_out_names_show_whole_or_not_at_all() {
    assert_eq!(left_out(&[]), None);
    let few = ["x".to_owned(), "y".to_owned()];
    assert_eq!(left_out(&few).as_deref(), Some("x, y"));
    // 100 names of 4 characters and their separators pass the 500 characters.
    let many: Vec<String> = (0..100).map(|n| format!("t{n:03}")).collect();
    assert_eq!(left_out(&many), None);
}
