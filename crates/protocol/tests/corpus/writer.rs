//! The files a kind or variant added after v1 brings, written from `support/examples.rs` into
//! `target/` for the author to move into `corpus/added/` (see `corpus/v1/README.md`).

use std::collections::BTreeSet;

use clauseo_protocol::{DomainEvent, Envelope, StreamId};

use crate::helpers::{inside, kinds, values};
use crate::{Corpus, V1, every_corpus, examples, fixtures, fnv1a, gaps};

/// The files to add for what `corpora` lack, each as `(name, entries)`, from the examples of
/// `support/examples.rs`: `<kind>` with every example of a kind no corpus holds, and
/// `<kind>-<Variant>` with the first example that holds a variant no corpus holds. Then each
/// variant no example holds either.
fn added_pairs(corpora: &[Corpus]) -> (Vec<(String, Vec<Envelope>)>, Vec<String>) {
    let held: BTreeSet<String> = corpora.iter().flat_map(kinds).collect();
    let entries: Vec<Envelope> = examples::every_event()
        .into_iter()
        .map(|event| {
            let mut entry = fixtures::envelope(event);
            if entry.event.is_matter_level() == Some(true) {
                entry.stream = StreamId::Matter(fixtures::matter_id(1));
            }
            entry
        })
        .collect();
    let mut pairs: Vec<(String, Vec<Envelope>)> = Vec::new();
    for kind in DomainEvent::KINDS
        .iter()
        .filter(|kind| !held.contains(**kind))
    {
        let of_kind = entries.iter().filter(|entry| entry.event.kind() == *kind);
        pairs.push(((*kind).to_owned(), of_kind.cloned().collect()));
    }
    let mut unheld = Vec::new();
    for (name, variant) in gaps(&values(corpora)) {
        let holder = entries.iter().find(|entry| {
            let value = serde_json::to_value(entry).unwrap_or_default();
            !gaps(&inside(&value)).contains(&(name, variant))
        });
        match holder {
            Some(entry) => pairs.push((
                format!("{}-{variant}", entry.event.kind()),
                vec![entry.clone()],
            )),
            None => unheld.push(format!("{name}::{variant}")),
        }
    }
    (pairs, unheld)
}

/// Writes the `added/` files for every kind and variant the corpus lacks into
/// `target/tmp/corpus-added/`, never into `corpus/`, and prints each file's move and its
/// [`ADDED_AFTER_V1`] row. Run as the corpus README says, with `INSTA_UPDATE=always`: the files
/// are written as insta binary snapshots, the crate's one way to write a file from a test.
#[test]
#[ignore = "writes the corpus/added files into target/; run it by name, as corpus/v1/README.md says"]
fn write_the_added_pairs() {
    let (pairs, unheld) = added_pairs(&every_corpus());
    assert!(
        unheld.is_empty(),
        "no example in support/examples.rs holds {unheld:?}; add one"
    );
    let out = concat!(env!("CARGO_TARGET_TMPDIR"), "/corpus-added");
    for (name, entries) in &pairs {
        let mut json = String::new();
        for entry in entries {
            json.push_str(&serde_json::to_string(entry).unwrap());
            json.push('\n');
        }
        let packed = rmp_serde::to_vec_named(entries).unwrap();
        for (file, bytes) in [("jsonl", json.as_bytes()), ("msgpack", &packed[..])] {
            let mut settings = insta::Settings::clone_current();
            settings.set_snapshot_path(format!("{out}/{file}"));
            settings.set_prepend_module_to_snapshot(false);
            settings.bind(|| {
                insta::assert_binary_snapshot!(&format!("{name}.{file}"), bytes.to_vec());
            });
            println!(
                "mv {out}/{file}/{name}.snap.{file} crates/protocol/tests/corpus/added/{name}.{file}"
            );
        }
        println!(
            "row: (\"added/{name}\", include_bytes!(\"corpus/added/{name}.jsonl\"), \
             include_bytes!(\"corpus/added/{name}.msgpack\"), ({}, {:#018x}), ({}, {:#018x})),",
            json.len(),
            fnv1a(json.as_bytes()),
            packed.len(),
            fnv1a(&packed)
        );
    }
    if pairs.is_empty() {
        println!("nothing to add: the corpus holds every kind and every variant");
    }
}

/// With a kind's entries and a variant's one entry taken out of v1, the writer would add a file
/// for each, named for the kind and for the kind and variant.
#[test]
fn the_writer_adds_what_the_corpus_lacks() {
    let text = std::str::from_utf8(V1.1).unwrap();
    let kept: String = text
        .lines()
        .filter(|line| {
            !line.contains("\"thread_archived\"") && !line.contains("\"kind\":\"folder\"")
        })
        .fold(String::new(), |kept, line| kept + line + "\n");
    let lacking: Corpus = ("v1/lacking", kept.leak().as_bytes(), V1.2, V1.3, V1.4);
    let (pairs, unheld) = added_pairs(&[lacking]);
    let names: Vec<&str> = pairs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["thread_archived", "matter_created-Folder"]);
    assert!(unheld.is_empty(), "{unheld:?}");
    assert_eq!(added_pairs(&[V1]), (Vec::new(), Vec::new()));
}
