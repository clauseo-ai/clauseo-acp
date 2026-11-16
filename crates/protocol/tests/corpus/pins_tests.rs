//! Tests of `corpus/pins.rs`: the pin check, and the folder's discovery on planted listings.
//! None writes a file, since the listing [`pairs`] reads is a value.

use crate::pins::{Listed, added, fnv1a, pairs, pin_problem, pin_text};

/// A well-formed pin for one byte `a` in each file.
const PIN: &[u8] = b"jsonl 1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n";

/// A listing of files, each `(name, bytes)`.
fn files(names: &[(&str, &'static [u8])]) -> Vec<(String, Listed)> {
    let listed = names
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), Listed::File(bytes)));
    listed.collect()
}

/// The problems `pairs` finds in `listing`; it must find some.
fn problems(listing: Vec<(String, Listed)>) -> Vec<String> {
    match pairs(listing) {
        Ok(found) => panic!("expected problems, found {} pairs", found.len()),
        Err(problems) => problems,
    }
}

/// The problems of a listing with one pair, `x`, whose pin is `pin`.
fn pin_problems(pin: &'static [u8]) -> Vec<String> {
    problems(files(&[
        ("x.jsonl", b"a"),
        ("x.msgpack", b"a"),
        ("x.pin", pin),
    ]))
}

/// The real folder holds pairs, every one pinned, each named `added/<name>`, in name order.
#[test]
fn the_added_folder_holds_only_pinned_pairs() {
    let found = added();
    assert!(!found.is_empty(), "corpus/added/ holds no pairs");
    let names: Vec<&str> = found.iter().map(|corpus| corpus.0).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert!(
        names.iter().all(|name| name.starts_with("added/")),
        "{names:?}"
    );
}

#[test]
fn a_listing_gives_its_pairs_sorted_with_their_pins() {
    let listing = files(&[
        (
            "b.pin",
            b"jsonl 0 0x0000000000000000\nmsgpack 10 0x00000000000000ff\n",
        ),
        ("a.msgpack", b"a"),
        ("b.jsonl", b""),
        ("a.pin", PIN),
        ("b.msgpack", b"0123456789"),
        ("a.jsonl", b"a"),
    ]);
    let found = pairs(listing).unwrap();
    let a = (1, 0xaf63_dc4c_8601_ec8c);
    assert_eq!(
        found,
        [
            ("added/a", &b"a"[..], &b"a"[..], a, a),
            ("added/b", &b""[..], &b"0123456789"[..], (0, 0), (10, 0xff)),
        ]
    );
}

/// The writer's pin parses back to the files' lengths and hashes.
#[test]
fn a_written_pin_reads_back() {
    let pin = pin_text(b"a", b"a");
    assert_eq!(pin.as_bytes(), PIN);
    assert!(
        pairs(files(&[
            ("x.jsonl", b"a"),
            ("x.msgpack", b"a"),
            ("x.pin", PIN)
        ]))
        .is_ok()
    );
}

/// A zero pin in a `.pin` parses, and the frozen-bytes check then says what to set it to.
#[test]
fn a_zero_pin_in_a_pin_file_says_what_to_set_it_to() {
    let pin = b"jsonl 0 0x0000000000000000\nmsgpack 1 0xaf63dc4c8601ec8c\n";
    let found = pairs(files(&[
        ("x.jsonl", b"a"),
        ("x.msgpack", b"a"),
        ("x.pin", pin),
    ]))
    .unwrap();
    let [(name, json, _, json_pin, _)] = found[..] else {
        panic!("{found:?}");
    };
    assert_eq!(
        pin_problem(&format!("{name}.jsonl"), json, json_pin).as_deref(),
        Some(
            "corpus/added/x.jsonl: set its line in corpus/added/x.pin to `jsonl 1 0xaf63dc4c8601ec8c`"
        )
    );
}

#[test]
fn a_pair_without_its_pin_fails() {
    let found = problems(files(&[("x.jsonl", b"a"), ("x.msgpack", b"a")]));
    assert_eq!(
        found,
        ["corpus/added/x: has x.jsonl, x.msgpack but no x.pin"]
    );
}

#[test]
fn a_pin_without_its_pair_fails() {
    let found = problems(files(&[("x.pin", PIN)]));
    assert_eq!(
        found,
        ["corpus/added/x: has x.pin but no x.jsonl, x.msgpack"]
    );
}

#[test]
fn a_jsonl_without_its_msgpack_fails() {
    let found = problems(files(&[("x.jsonl", b"a"), ("x.pin", PIN)]));
    assert_eq!(
        found,
        ["corpus/added/x: has x.jsonl, x.pin but no x.msgpack"]
    );
}

/// Anything but the three files of a name fails: another extension, no extension, a bare
/// extension, a folder, a name that is not UTF-8 (`include_dir` writes it with U+FFFD).
#[test]
fn a_stray_file_or_folder_fails() {
    let mut listing = files(&[
        ("x.jsonl", b"a"),
        ("x.msgpack", b"a"),
        ("x.pin", PIN),
        ("notes.txt", b""),
        ("x.json", b""),
        ("README", b""),
        (".pin", PIN),
        ("x\u{fffd}.pin", PIN),
    ]);
    listing.push(("old".to_owned(), Listed::Folder));
    let stray = "is not a .jsonl, .msgpack or .pin; move it out of corpus/added/";
    assert_eq!(
        problems(listing),
        [
            format!("corpus/added/notes.txt {stray}"),
            format!("corpus/added/x.json {stray}"),
            format!("corpus/added/README {stray}"),
            format!("corpus/added/.pin {stray}"),
            "corpus/added/x\u{fffd}.pin: the name is not UTF-8".to_owned(),
            "corpus/added/old is a folder; corpus/added/ holds only files".to_owned(),
        ]
    );
}

/// Every problem is listed at once, not the first alone.
#[test]
fn every_problem_is_listed_at_once() {
    let found = problems(files(&[
        ("a.jsonl", b"a"),
        ("b.pin", PIN),
        ("c.jsonl", b"a"),
        ("c.msgpack", b"a"),
        (
            "c.pin",
            b"jsonl 1 0xAF63DC4C8601EC8C\nmsgpack 1 0xaf63dc4c8601ec8c\n",
        ),
        ("stray.txt", b""),
    ]));
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(
        found[0].starts_with("corpus/added/stray.txt "),
        "{found:#?}"
    );
    assert!(found[1].starts_with("corpus/added/a: "), "{found:#?}");
    assert!(found[2].starts_with("corpus/added/b: "), "{found:#?}");
    assert!(
        found[3].starts_with("corpus/added/c.pin line 1: "),
        "{found:#?}"
    );
}

/// Each malformed pin fails, naming the file, the line where there is one, and what is wrong.
#[test]
fn a_malformed_pin_fails() {
    let cases: [(&[u8], &str); 14] = [
        (
            b"jsonl 1 0xaf63dc4c8601ec8c\r\nmsgpack 1 0xaf63dc4c8601ec8c\r\n",
            "x.pin holds a CR: its lines end in LF alone",
        ),
        (
            b"jsonl 1 0xAF63DC4C8601EC8C\nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: hash `0xAF63DC4C8601EC8C` is not lowercase hex",
        ),
        (
            b"msgpack 1 0xaf63dc4c8601ec8c\njsonl 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: starts with `msgpack`, not `jsonl`",
        ),
        (
            b"jsonl 1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n\n",
            "x.pin has 3 lines, not 2",
        ),
        (
            b"jsonl 1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\nmsgpack 1 0x0\n",
            "x.pin has 3 lines, not 2",
        ),
        (b"jsonl 1 0xaf63dc4c8601ec8c\n", "x.pin has 1 lines, not 2"),
        (
            b"jsonl 1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c",
            "x.pin does not end in LF",
        ),
        (
            b"jsonl 1 af63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: hash `af63dc4c8601ec8c` does not start with 0x",
        ),
        (
            b"jsonl 1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8\n",
            "x.pin line 2: hash `0xaf63dc4c8601ec8` has 15 digits, not 16",
        ),
        (
            b"jsonl 01 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: length `01` is not a decimal number without leading zeros",
        ),
        (
            b"jsonl +1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: length `+1` is not a decimal number without leading zeros",
        ),
        (
            b"jsonl  1 0xaf63dc4c8601ec8c\nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: `jsonl  1 0xaf63dc4c8601ec8c` has 4 fields, not 3",
        ),
        (
            b"jsonl 1 0xaf63dc4c8601ec8c \nmsgpack 1 0xaf63dc4c8601ec8c\n",
            "x.pin line 1: `jsonl 1 0xaf63dc4c8601ec8c ` has 4 fields, not 3",
        ),
        (b"\xff", "x.pin is not UTF-8"),
    ];
    for (pin, expected) in cases {
        let found = pin_problems(pin);
        let first = found.first().map(String::as_str).unwrap_or_default();
        assert!(
            first.starts_with(&format!("corpus/added/{expected}"))
                && found.iter().all(|problem| problem.contains("; a pin is")),
            "{found:?}\nexpected: corpus/added/{expected}"
        );
        // Swapped lines are wrong on both, and both are listed.
        let lines_wrong = if expected.contains("`msgpack`") { 2 } else { 1 };
        assert_eq!(found.len(), lines_wrong, "{found:?}");
    }
}

#[test]
fn a_new_pin_says_what_to_set_it_to() {
    let set = pin_problem("added/x.jsonl", b"a", (0, 0)).unwrap();
    assert_eq!(
        set,
        "corpus/added/x.jsonl: set its line in corpus/added/x.pin to `jsonl 1 0xaf63dc4c8601ec8c`"
    );
    let set = pin_problem("added/x.msgpack", b"", (0, 0)).unwrap();
    assert!(
        set.ends_with("x.pin to `msgpack 0 0xcbf29ce484222325`"),
        "{set}"
    );
    let changed = pin_problem("added/x.jsonl", b"a", (1, 7)).unwrap();
    assert!(changed.contains("changed: it is frozen"), "{changed}");
    assert_eq!(
        pin_problem("added/x.jsonl", b"a", (1, 0xaf63_dc4c_8601_ec8c)),
        None
    );
}

/// A `(0, 0)` pin is the friendly "set this pin" message only for a new file in `corpus/added/`;
/// a frozen file's pin is never `(0, 0)`.
#[test]
fn a_zero_pin_is_refused_outside_added() {
    for path in ["v1/events.jsonl", "v1/events.msgpack"] {
        assert_eq!(
            pin_problem(path, b"a", (0, 0)),
            Some(format!(
                "corpus/{path} is frozen, so its pin is its length and hash, never (0, 0)"
            ))
        );
    }
}

#[test]
fn fnv1a_matches_the_reference_values() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
}
