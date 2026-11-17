//! `list_versions`: only this matter's versions, newest first, the limit and its cap, paging,
//! and what each version says.

use clauseo_protocol::{ThreadId, Timestamp};
use serde_json::{Value, json};

use super::{Fixture, call, checkpoint, ok, refused, v7};
use crate::versions::texts::Refusal;
use crate::versions::{MAX_FILES_PER_VERSION, MAX_LIMIT};

fn ids(page: &Value) -> Vec<String> {
    page["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|version| version["id"].as_str().unwrap().to_owned())
        .collect()
}

fn id(n: u32) -> String {
    checkpoint(n).to_string()
}

#[test]
fn it_lists_only_this_matters_versions_newest_first() {
    let mut fixture = Fixture::new("versions-list-own");
    fixture.add_matter(v7(200), "other", 50);
    for n in 2..=4 {
        fixture.put("brief.txt", "x".repeat(n as usize).as_bytes());
        fixture.checkpoint(n);
        fixture.put_in("other", "secret.txt", "y".repeat(n as usize).as_bytes());
        fixture.checkpoint_of(1, 50 + n);
    }
    let backend = fixture.backend();
    let page = call(&backend, "list_versions", json!({}));
    let page = ok(&page);
    assert_eq!(ids(&page), vec![id(4), id(3), id(2), id(1)]);
    assert!(page.get("next_before").is_none(), "{page}");
    assert!(!page.to_string().contains("secret.txt"), "{page}");
}

#[test]
fn it_honours_the_limit_and_pages_with_next_before() {
    let mut fixture = Fixture::new("versions-list-pages");
    for n in 2..=6 {
        fixture.put("brief.txt", "x".repeat(n as usize).as_bytes());
        fixture.checkpoint(n);
    }
    let backend = fixture.backend();
    let first = call(&backend, "list_versions", json!({ "limit": 2 }));
    let first = ok(&first);
    assert_eq!(ids(&first), vec![id(6), id(5)]);
    assert_eq!(first["next_before"], id(5));
    let second = call(
        &backend,
        "list_versions",
        json!({ "limit": 2, "before": first["next_before"] }),
    );
    let second = ok(&second);
    assert_eq!(ids(&second), vec![id(4), id(3)]);
    let last = call(
        &backend,
        "list_versions",
        json!({ "limit": 2, "before": second["next_before"] }),
    );
    let last = ok(&last);
    assert_eq!(ids(&last), vec![id(2), id(1)]);
    assert!(last.get("next_before").is_none(), "{last}");
}

#[test]
fn it_gives_twenty_by_default_and_fifty_at_most() {
    let mut fixture = Fixture::new("versions-list-cap");
    for n in 2..=61 {
        fixture.put("brief.txt", "x".repeat(n as usize).as_bytes());
        fixture.checkpoint(n);
    }
    let backend = fixture.backend();
    let plain = call(&backend, "list_versions", json!({}));
    assert_eq!(ids(&ok(&plain)).len(), 20);
    let most = call(&backend, "list_versions", json!({ "limit": MAX_LIMIT }));
    let most = ok(&most);
    assert_eq!(ids(&most).len(), 50);
    assert_eq!(ids(&most).first().unwrap(), &id(61));
    assert_eq!(most["next_before"], id(12));
    // Every version of a page past one job's diffs has its own files.
    assert!(
        most["versions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|version| version["files"][0]["path"] == "brief.txt"),
        "{most}"
    );
    for limit in [json!(0), json!(51), json!(-1), json!("5"), json!(2.5)] {
        let over = call(&backend, "list_versions", json!({ "limit": limit }));
        assert_eq!(refused(&over), Refusal::BadLimit.text());
    }
}

#[test]
fn a_before_from_another_matter_is_unknown_as_a_made_up_one_is() {
    let mut fixture = Fixture::new("versions-list-before");
    fixture.add_matter(v7(200), "other", 50);
    let backend = fixture.backend();
    let other = call(&backend, "list_versions", json!({ "before": id(50) }));
    let made_up = call(&backend, "list_versions", json!({ "before": id(999) }));
    let not_an_id = call(&backend, "list_versions", json!({ "before": "../x" }));
    assert_eq!(refused(&other), Refusal::UnknownVersion.text());
    assert_eq!(other, made_up);
    assert_eq!(other, not_an_id);
    let number = call(&backend, "list_versions", json!({ "before": 3 }));
    assert_eq!(refused(&number), Refusal::BadBefore.text());
}

#[test]
fn each_version_says_when_what_who_and_its_files() {
    let mut fixture = Fixture::new("versions-list-fields");
    fixture.put("a.txt", b"alpha");
    fixture.put("b.txt", b"bravo");
    fixture.checkpoint(2);
    let other_thread = ThreadId::new(v7(77)).unwrap();
    fixture.rename("a.txt", "c.txt");
    fixture.remove("b.txt");
    fixture.after_turn(3, other_thread);
    fixture.put("c.txt", b"charlie, longer");
    fixture.after_turn(4, fixture.session.thread);
    for n in 0..30 {
        fixture.put(&format!("many/{n:02}.txt"), b"m");
    }
    fixture.checkpoint(5);
    {
        let mut logged = fixture.source.logged.lock().unwrap();
        logged
            .at
            .insert(checkpoint(4), Timestamp::new(1_790_000_004_000).unwrap());
        logged.restored_from.insert(checkpoint(4), checkpoint(2));
    }
    let backend = fixture.backend();
    let page = call(&backend, "list_versions", json!({}));
    let page = ok(&page);
    let versions = page["versions"].as_array().unwrap();
    let many = &versions[0];
    assert_eq!(many["kind"], "before_turn");
    assert_eq!(many["by"], "outside_clauseo");
    assert_eq!(many["files_total"], 30);
    assert_eq!(
        many["files"].as_array().unwrap().len(),
        MAX_FILES_PER_VERSION
    );
    assert_eq!(many["at"], Value::Null);
    let edit = &versions[1];
    assert_eq!(edit["kind"], "after_turn");
    assert_eq!(edit["by"], "agent");
    assert_eq!(edit["this_conversation"], true);
    assert_eq!(edit["at"], "2026-09-21T14:13:24Z");
    assert_eq!(edit["restored_from"], id(2));
    assert_eq!(
        edit["files"],
        json!([{ "path": "c.txt", "change": "modified" }])
    );
    let moved = &versions[2];
    assert_eq!(moved["this_conversation"], false);
    assert!(moved.get("restored_from").is_none(), "{moved}");
    assert!(
        !moved.to_string().contains(&other_thread.to_string()),
        "{moved}"
    );
    assert_eq!(
        moved["files"],
        json!([
            { "path": "b.txt", "change": "deleted" },
            { "path": "c.txt", "change": "renamed", "from": "a.txt" },
        ])
    );
    assert_eq!(moved["files_total"], 2);
    let import = versions.last().unwrap();
    assert_eq!(import["kind"], "import");
    assert_eq!(import["by"], "lawyer");
    assert_eq!(import["id"], id(1));
}

/// A path holding a line or paragraph separator is listed with each written as the text
/// `\u{2028}` or `\u{2029}`, so the list keeps one line per item.
#[test]
fn a_line_or_paragraph_separator_in_a_path_is_written_as_text() {
    let mut fixture = Fixture::new("versions-list-separators");
    fixture.put("line\u{2028}break.txt", b"one");
    fixture.put("para\u{2029}break.txt", b"two");
    fixture.checkpoint(2);
    let backend = fixture.backend();
    let answer = call(&backend, "list_versions", json!({}));
    ok(&answer);
    let text = answer["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains(['\u{2028}', '\u{2029}']), "{text}");
    assert!(text.contains("  added: line\\u{2028}break.txt"), "{text}");
    assert!(text.contains("  added: para\\u{2029}break.txt"), "{text}");
}
