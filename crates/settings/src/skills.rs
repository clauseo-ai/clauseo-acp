//! The `skills` section (decision record 0040): which extra skill packs a firm admits
//! (`skills.extra`, `skills.curated`), whether a pack may carry scripts (`skills.allow_scripts`),
//! and the packs the lawyer turned on, each with the hash it had then (`skills.enabled`).
//!
//! Every message here is fixed text, a key, a pack id that passes the id rule's charset, or an
//! entry's place (`skills.enabled.<id 2>`): none quotes a hash or text that is no id, which may
//! be pasted (#411's rule, as `plans.` keys are shown).

#[cfg(test)]
mod tests;

use crate::warning::WarningKind;
use serde_json::Value;
use std::collections::BTreeMap;

/// Why an id is refused.
pub(crate) const BAD_ID: &str = "a pack id is 2 to 30 lowercase letters, digits and single \
                                 hyphens, and not a name Clauseo keeps for its own skills";

/// Why a recorded hash is refused.
pub(crate) const BAD_HASH: &str = "a pack's hash is sha256: followed by 64 lowercase hex digits";

/// What a pack's recorded hash starts with.
const HASH_PREFIX: &str = "sha256:";

/// The fewest and most bytes a pack id has.
const ID_LEN: std::ops::RangeInclusive<usize> = 2..=30;

/// Ids no pack may take: the built-in pack's id, `builtin`, `stetdoc` and each built-in skill's
/// name (`profile`'s `RESERVED`, `STETDOC` and `BUILTIN`). Every `clauseo-*` is refused too.
/// `tests::the_pack_id_rule_matches_profile` pins this to `profile`'s own table.
const RESERVED: [&str; 6] = [
    "clauseo",
    "builtin",
    "stetdoc",
    "legal-drafting",
    "legal-research",
    "matter-brief",
];

/// Windows device names, refused as ids (`profile`'s `DEVICE_NAMES`, plus `com0`-`com9` and
/// `lpt0`-`lpt9`).
const DEVICE_NAMES: [&str; 4] = ["con", "prn", "aux", "nul"];

/// Whether `id` may name a pack: `^[a-z0-9]+(-[a-z0-9]+)*$`, 2 to 30 bytes, no Windows device
/// name, and none of [`RESERVED`] nor any `clauseo-*`. A copy of `profile`'s `packs::is_pack_id`,
/// since `settings` depends on no crate of ours; PR-C moves the rule to `protocol`'s `PackId`.
pub(crate) fn is_pack_id(id: &str) -> bool {
    ID_LEN.contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !is_device_name(id)
        && !RESERVED.contains(&id)
        && !id.starts_with("clauseo-")
}

/// Whether `id` is a Windows device name (`con`, `com1`, …).
fn is_device_name(id: &str) -> bool {
    let bytes = id.as_bytes();
    DEVICE_NAMES.contains(&id)
        || (bytes.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && bytes.get(3).is_some_and(u8::is_ascii_digit))
}

/// Whether `hash` is a pack hash as recorded: `sha256:` and 64 lowercase hex digits.
pub(crate) fn hash_ok(hash: &str) -> bool {
    hash.strip_prefix(HASH_PREFIX).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// How a message shows the entry `id`, the `place`th of the list or map `key` (from 1): quoted
/// when it is id-shaped (an agent id's characters, [`crate::plans::id_ok`], so a reserved or
/// misspelt id still reads as itself), and as `<id {place}>` otherwise, never echoing text that
/// may be pasted.
fn shown(key: &str, id: &str, place: usize) -> String {
    if crate::plans::id_ok(id) {
        format!("{key}.{id}")
    } else {
        format!("{key}.<id {place}>")
    }
}

/// Types `skills.curated`: a list of pack ids, each kept once in its first place. A value that
/// is not a list of strings, or a list holding any entry that is no pack id, refuses the key:
/// what the firm meant to admit is unknown, so it admits nothing (a policy's locks it empty).
pub(crate) fn curated(key: &str, value: Value) -> Result<Vec<String>, WarningKind> {
    let refused = |message: String| WarningKind::InvalidValue {
        key: key.to_owned(),
        message,
    };
    let Value::Array(items) = value else {
        return Err(refused("it must be a list of pack ids".to_owned()));
    };
    let mut ids: Vec<String> = Vec::with_capacity(items.len());
    for (at, item) in items.into_iter().enumerate() {
        match item {
            Value::String(id) if is_pack_id(&id) => {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Value::String(id) => {
                return Err(refused(format!(
                    "{} is not a pack id ({BAD_ID}), so the list admits nothing",
                    shown(key, &id, at + 1)
                )));
            }
            _ => {
                return Err(refused(format!(
                    "{} is not a string, so the list admits nothing",
                    shown(key, "", at + 1)
                )));
            }
        }
    }
    Ok(ids)
}

/// Types `skills.enabled`: an object from pack id to the hash the pack had when it was turned
/// on. A value that is not an object refuses the key. Inside it, an entry whose id is no pack
/// id, or whose value is not a hash ([`hash_ok`]), is left out with a warning in `dropped`
/// that gives its place (or its id, when id-shaped), and the other entries load.
pub(crate) fn enabled(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<BTreeMap<String, String>, WarningKind> {
    let Value::Object(entries) = value else {
        return Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be an object from pack id to sha256:<64 lowercase hex digits>"
                .to_owned(),
        });
    };
    let mut packs = BTreeMap::new();
    for (at, (id, hash)) in entries.into_iter().enumerate() {
        let message = match hash {
            _ if !is_pack_id(&id) => BAD_ID,
            Value::String(hash) if hash_ok(&hash) => {
                packs.insert(id, hash);
                continue;
            }
            _ => BAD_HASH,
        };
        dropped.push(WarningKind::InvalidValue {
            key: shown(key, &id, at + 1),
            message: message.to_owned(),
        });
    }
    Ok(packs)
}
