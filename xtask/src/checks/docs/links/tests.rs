//! Tests of how a link destination is read and where it lands.

use super::*;

/// `target` as `relative_path` reads it.
fn path(target: &str) -> Option<&str> {
    relative_path(target)
}

#[test]
fn absolute_urls_pure_anchors_and_empty_destinations_are_not_checked() {
    for target in [
        "https://example.com/x",
        "mailto:team@example.com",
        "HTTP://x",
        "//cdn.example.com/x.js",
        "#intro",
        "",
    ] {
        assert_eq!(path(target), None, "{target} is not checked");
    }
}

#[test]
fn the_anchor_or_query_is_cut_at_whichever_comes_first_and_the_path_kept() {
    assert_eq!(path("nope.md#x"), Some("nope.md"));
    assert_eq!(path("docs/a.md"), Some("docs/a.md"));
    assert_eq!(path("docs/a.md?plain=1"), Some("docs/a.md"));
    assert_eq!(path("docs/a.md?plain=1#L3"), Some("docs/a.md"));
    assert_eq!(path("docs/a.md#x?y"), Some("docs/a.md"));
    assert_eq!(path("?plain=1"), None);
}

#[test]
fn a_destination_with_a_colon_in_a_folder_is_a_path() {
    assert_eq!(path("docs/a:b.md"), Some("docs/a:b.md"));
}

#[test]
fn links_resolve_from_the_linking_files_folder() {
    assert_eq!(resolve("docs", "a.md"), Target::Inside("docs/a.md".into()));
    assert_eq!(resolve("docs", "../A.md"), Target::Inside("A.md".into()));
    assert_eq!(resolve("", "./x/../y.md"), Target::Inside("y.md".into()));
    assert_eq!(
        resolve("docs/decisions", "../../ui/app"),
        Target::Inside("ui/app".into())
    );
}

#[test]
fn a_leading_slash_starts_at_the_repo_root() {
    assert_eq!(resolve("docs", "/x/y.md"), Target::Inside("x/y.md".into()));
}

#[test]
fn climbing_above_the_root_leaves_the_repo() {
    assert_eq!(
        resolve("", "../../outside.md"),
        Target::Outside { private: false }
    );
    assert_eq!(
        resolve("docs", "../../../x.md"),
        Target::Outside { private: false }
    );
}

#[test]
fn the_private_folder_beside_the_repo_is_outside_and_private() {
    assert_eq!(
        resolve("", "../clauseo-acp-private/START-HERE.md"),
        Target::Outside { private: true }
    );
    assert_eq!(
        resolve("docs/decisions", "../../../clauseo-acp-private/x.md"),
        Target::Outside { private: true }
    );
    assert_eq!(
        resolve("", "../clauseo-acp-private/../x.md"),
        Target::Outside { private: false }
    );
}

#[test]
fn percent_escapes_are_read_and_bad_ones_stay() {
    assert_eq!(percent_decode("my%20file.md"), "my file.md");
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("a%zz%+1"), "a%zz%+1");
}

#[test]
fn a_file_in_the_root_has_no_folder() {
    assert_eq!(dir_of("AGENTS.md"), "");
    assert_eq!(dir_of("docs/a.md"), "docs");
}
