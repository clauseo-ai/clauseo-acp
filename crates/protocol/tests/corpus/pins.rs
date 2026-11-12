//! The pairs added after v1, found in `corpus/added/` rather than listed in Rust. Each name there
//! has exactly three files: `<name>.jsonl`, `<name>.msgpack` and `<name>.pin`, the pin holding
//! each file's byte length and FNV-1a 64 hash, two lines, each ending in LF:
//!
//! ```text
//! jsonl <length> 0x<16 lowercase hex digits>
//! msgpack <length> 0x<16 lowercase hex digits>
//! ```
//!
//! The crate reads no files at run time, so `include_dir!` embeds the folder when the test target
//! is built. A file added since the last build shows up only once the target rebuilds: a change
//! to any `.rs` file of the test does it, and the corpus README's steps end with a `touch`.
//! [`pairs`] works on a listing in memory, so `pins_tests.rs` plants listings without files.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use include_dir::{Dir, DirEntry, include_dir};

use crate::Corpus;

/// `corpus/added/` as it was when the test target was built.
static ADDED: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/tests/corpus/added");

/// The three files of each name in `corpus/added/`, in this order.
const MEMBERS: [&str; 3] = ["jsonl", "msgpack", "pin"];

/// How a pin is written, for the failures that name a malformed one.
const FORMAT: &str = "a pin is two lines, `jsonl <length> 0x<16 lowercase hex digits>` then \
                      `msgpack <length> 0x<16 lowercase hex digits>`, each ending in LF";

/// A pair's two pins, `(length, hash)` for its `.jsonl` then its `.msgpack`.
type Pins = ((usize, u64), (usize, u64));

/// What a listing holds at one name: a file and its bytes, or a folder.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Listed {
    File(&'static [u8]),
    Folder,
}

/// The pairs of `corpus/added/`, sorted by name, each as `added/<name>`; the first call reads
/// the embedded folder and panics with every problem it finds, so does each later call.
pub(crate) fn added() -> &'static [Corpus] {
    static FOUND: OnceLock<Result<Vec<Corpus>, Vec<String>>> = OnceLock::new();
    match FOUND.get_or_init(|| pairs(listing(&ADDED))) {
        Ok(found) => found,
        Err(problems) => panic!(
            "corpus/added/ must hold, for each name, exactly <name>.jsonl, <name>.msgpack and \
             <name>.pin (see corpus/v1/README.md):\n- {}",
            problems.join("\n- ")
        ),
    }
}

/// Each entry directly in `dir`, by its name: a file with its bytes, or a folder. `include_dir`
/// writes a name that is not UTF-8 lossily, with U+FFFD, which [`pairs`] refuses.
fn listing(dir: &'static Dir<'static>) -> Vec<(String, Listed)> {
    let name = |path: &std::path::Path| {
        let name = path.file_name().unwrap_or(path.as_os_str());
        name.to_string_lossy().into_owned()
    };
    dir.entries()
        .iter()
        .map(|entry| match entry {
            DirEntry::Dir(folder) => (name(folder.path()), Listed::Folder),
            DirEntry::File(file) => (name(file.path()), Listed::File(file.contents())),
        })
        .collect()
}

/// The pairs a listing of `corpus/added/` holds, sorted by name, each with the pins its `.pin`
/// holds; or every problem with the listing, all at once: a folder, a name that is not UTF-8, a
/// file that is not a `.jsonl`, `.msgpack` or `.pin`, a name missing one of the three, a
/// malformed pin.
pub(crate) fn pairs(listing: Vec<(String, Listed)>) -> Result<Vec<Corpus>, Vec<String>> {
    let mut problems = Vec::new();
    let mut names: BTreeMap<String, [Option<&'static [u8]>; 3]> = BTreeMap::new();
    for (name, listed) in listing {
        let at = format!("corpus/added/{name}");
        if name.contains('\u{fffd}') {
            problems.push(format!("{at}: the name is not UTF-8"));
            continue;
        }
        let Listed::File(bytes) = listed else {
            problems.push(format!("{at} is a folder; corpus/added/ holds only files"));
            continue;
        };
        let member = name.rsplit_once('.').and_then(|(stem, extension)| {
            let index = MEMBERS.iter().position(|member| *member == extension)?;
            (!stem.is_empty()).then_some((stem, index))
        });
        let Some((stem, index)) = member else {
            problems.push(format!(
                "{at} is not a .jsonl, .msgpack or .pin; move it out of corpus/added/"
            ));
            continue;
        };
        let slots = names.entry(stem.to_owned()).or_default();
        if slots
            .get_mut(index)
            .and_then(|slot| slot.replace(bytes))
            .is_some()
        {
            problems.push(format!("{at} is listed twice"));
        }
    }
    let mut found = Vec::new();
    for (stem, slots) in names {
        if let [Some(json), Some(packed), Some(pin)] = slots {
            match parse_pin(&stem, pin) {
                Ok((json_pin, packed_pin)) => {
                    let name: &'static str = format!("added/{stem}").leak();
                    found.push((name, json, packed, json_pin, packed_pin));
                }
                Err(mut malformed) => problems.append(&mut malformed),
            }
            continue;
        }
        let files = |present: bool| {
            let members = MEMBERS.iter().zip(&slots);
            let chosen = members.filter(|(_, slot)| slot.is_some() == present);
            chosen
                .map(|(member, _)| format!("{stem}.{member}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        problems.push(format!(
            "corpus/added/{stem}: has {} but no {}",
            files(true),
            files(false)
        ));
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

/// The two pins of `corpus/added/<stem>.pin`, `(length, hash)` for the `.jsonl` then the
/// `.msgpack`; or what is wrong with it, naming the file.
fn parse_pin(stem: &str, bytes: &[u8]) -> Result<Pins, Vec<String>> {
    let at = format!("corpus/added/{stem}.pin");
    let fail = |what: &str| vec![format!("{at} {what}; {FORMAT}")];
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(fail("is not UTF-8"));
    };
    if text.contains('\r') {
        return Err(fail(
            "holds a CR: its lines end in LF alone, as corpus/.gitattributes keeps them",
        ));
    }
    let Some(body) = text.strip_suffix('\n') else {
        return Err(fail("does not end in LF"));
    };
    let lines: Vec<&str> = body.split('\n').collect();
    let [json, packed] = lines[..] else {
        return Err(fail(&format!("has {} lines, not 2", lines.len())));
    };
    match (parse_line(json, "jsonl"), parse_line(packed, "msgpack")) {
        (Ok(json), Ok(packed)) => Ok((json, packed)),
        (json, packed) => Err([(1, json.err()), (2, packed.err())]
            .into_iter()
            .filter_map(|(line, what)| Some(format!("{at} line {line}: {}; {FORMAT}", what?)))
            .collect()),
    }
}

/// The `(length, hash)` of one pin line, `<label> <length> 0x<16 lowercase hex digits>`, or what
/// is wrong with it.
fn parse_line(line: &str, label: &str) -> Result<(usize, u64), String> {
    let fields: Vec<&str> = line.split(' ').collect();
    let [named, length, hash] = fields[..] else {
        return Err(format!("`{line}` has {} fields, not 3", fields.len()));
    };
    if named != label {
        return Err(format!("starts with `{named}`, not `{label}`"));
    }
    let decimal = !length.is_empty() && length.bytes().all(|byte| byte.is_ascii_digit());
    if !decimal || (length.len() > 1 && length.starts_with('0')) {
        return Err(format!(
            "length `{length}` is not a decimal number without leading zeros"
        ));
    }
    let length = length
        .parse()
        .map_err(|error| format!("length `{length}`: {error}"))?;
    let Some(digits) = hash.strip_prefix("0x") else {
        return Err(format!("hash `{hash}` does not start with 0x"));
    };
    if !digits
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!("hash `{hash}` is not lowercase hex"));
    }
    if digits.len() != 16 {
        return Err(format!("hash `{hash}` has {} digits, not 16", digits.len()));
    }
    let hash =
        u64::from_str_radix(digits, 16).map_err(|error| format!("hash `{hash}`: {error}"))?;
    Ok((length, hash))
}

/// The line of a pin for one file: `<label> <length> 0x<16 lowercase hex digits>`.
pub(crate) fn pin_line(label: &str, bytes: &[u8]) -> String {
    format!("{label} {} {:#018x}", bytes.len(), fnv1a(bytes))
}

/// The `.pin` of a pair, as its file holds it.
pub(crate) fn pin_text(json: &[u8], packed: &[u8]) -> String {
    format!(
        "{}\n{}\n",
        pin_line("jsonl", json),
        pin_line("msgpack", packed)
    )
}

/// What is wrong with a corpus file's pin, if anything: a new `(0, 0)` pin for a file in
/// `corpus/added/` (`jsonl 0 0x0000000000000000` in its `.pin`) is to be set to the file's
/// length and hash; a `(0, 0)` pin anywhere else is refused, since a frozen file is pinned from
/// the start; any other mismatch means the frozen file changed.
pub(crate) fn pin_problem(path: &str, bytes: &[u8], frozen: (usize, u64)) -> Option<String> {
    let found = (bytes.len(), fnv1a(bytes));
    let added = path
        .strip_prefix("added/")
        .and_then(|name| name.rsplit_once('.'));
    if found == frozen {
        None
    } else if let (Some((stem, label)), (0, 0)) = (added, frozen) {
        Some(format!(
            "corpus/{path}: set its line in corpus/added/{stem}.pin to `{}`",
            pin_line(label, bytes)
        ))
    } else if frozen == (0, 0) {
        Some(format!(
            "corpus/{path} is frozen, so its pin is its length and hash, never (0, 0)"
        ))
    } else {
        Some(format!(
            "corpus/{path} changed: it is frozen, never edit or regenerate it; a new schema adds \
             corpus/v2 beside v1, a new kind a file in corpus/added/"
        ))
    }
}

/// The 64-bit FNV-1a hash of `bytes`.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
