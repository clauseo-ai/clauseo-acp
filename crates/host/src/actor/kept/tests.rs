use clauseo_protocol::{KeptKind, KeptPermission};

use super::describe;

fn shown(rule: &str, folder: &str, windows: bool) -> Option<(KeptKind, String, Option<String>)> {
    let roots = crate::agent::always::matter_roots([folder], windows);
    describe(rule, &roots, windows).map(
        |KeptPermission {
             rule: kept,
             kind,
             subject,
             server,
         }| {
            assert_eq!(kept, rule, "the exact rule is the removal key");
            (kind, subject, server)
        },
    )
}

#[test]
fn each_rule_reads_as_a_kind_and_a_subject() {
    let folder = "/work/smith";
    let cases = [
        ("Bash(npm test)", KeptKind::Run, "npm test", None),
        (
            "WebFetch(domain:example.com)",
            KeptKind::Site,
            "example.com",
            None,
        ),
        (
            "mcp__court-search__find_case",
            KeptKind::Tool,
            "find_case",
            Some("court-search"),
        ),
        (
            "Edit(//work/smith/work/out.txt)",
            KeptKind::Edit,
            "work/out.txt",
            None,
        ),
        (
            "Read(//work/smith/notes.txt)",
            KeptKind::Read,
            "notes.txt",
            None,
        ),
        (
            "Read(//srv/elsewhere.txt)",
            KeptKind::Read,
            "/srv/elsewhere.txt",
            None,
        ),
        // A sibling folder that starts with the matter's name is outside it.
        (
            "Edit(//work/smithers/a.txt)",
            KeptKind::Edit,
            "/work/smithers/a.txt",
            None,
        ),
    ];
    for (rule, kind, subject, server) in cases {
        assert_eq!(
            shown(rule, folder, false),
            Some((kind, subject.to_owned(), server.map(str::to_owned))),
            "{rule}"
        );
    }
}

#[test]
fn a_windows_file_inside_the_matter_is_relative_in_any_case() {
    let shown = shown(
        "Edit(//c/users/ann/smith/Drafts/a.docx)",
        r"C:\Users\Ann\Smith",
        true,
    );
    assert_eq!(
        shown,
        Some((KeptKind::Edit, "Drafts/a.docx".to_owned(), None))
    );
}

#[test]
fn text_no_kept_rule_has_reads_as_none() {
    for rule in [
        "PowerShell(ls)",
        "Bash(ls",
        "mcp__nameonly",
        "WebFetch(example.com)",
        "",
    ] {
        assert_eq!(shown(rule, "/work/smith", false), None, "{rule}");
    }
}
