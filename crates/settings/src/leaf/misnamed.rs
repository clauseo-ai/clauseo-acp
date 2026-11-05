//! The security keys a policy source names in a form this version cannot read (ADR 0009,
//! decision records 0036 and 0048): each is refused, so locked off, since what the firm meant
//! there is unknown and the user's value must not stand in.

#[cfg(test)]
mod tests;

use super::{Leaf, Raw, SECURITY_KEYS, typed};
use crate::lock_reason::{LockCause, LockReason, SourceKind, shown_section};
use crate::plans::shown_plan_key;
use crate::policy::{PolicyValues, kind_of};
use crate::schema::SECTIONS;
use crate::warning::{Origin, Warning, WarningKind};
use unicode_normalization::UnicodeNormalization;

/// How far, in edits ([`distance`]), a word of a name may be from the security section
/// `section` and still stand for it (`skill`, `tool`, `instal`, `mpc`): 1 for a section of 4
/// characters or fewer, so `app` and `tmp` are not `mcp`, and 2 above. No two known sections
/// are this close (`tests::no_two_known_sections_are_within_the_misspelling_distance`).
pub(crate) fn misspelt(section: &str) -> usize {
    if section.chars().count() <= 4 { 1 } else { 2 }
}

/// [`typed`] for a policy source: `policy.json`, the registry, and macOS managed preferences,
/// whose shim asks for every key the domain lists as well as the known ones. A
/// security-relevant key is also refused, so locked off (ADR 0009, [`super::refuse`]), when the
/// source names its topic in a form this version cannot read: its section that is not an object
/// (`"mcp": "off"`), or any unknown key whose name holds the section's name in any case
/// (`"mcp.servers"` at the top of `policy.json`, `"MCP"`, `"mcpServers"`, `mcp.Servers`,
/// `mcp_servers`). A first segment that is a known section is about that section, and what
/// follows it is read by the same rule as any name ([`about_section`]): any segment that holds
/// a security section's name or is within [`misspelt`] edits of it (`skill`, `Tools`,
/// `clauseo.skills_extra`, `ui.tools`) is about it, a deliberate over-lock (`foo.tools_note`
/// locks `tools`). What the
/// firm meant there is unknown, so the user's value must not stand in. Each such key gets one
/// warning naming the shape that sets it, and a lock reason naming the source by kind and the
/// name (by place when it is no plain word). An unrelated unknown key, and an entry left out
/// inside a valid list, only warn. `wrapped` holds the dotted names nested inside an unknown
/// `policy.json` section ([`crate::json::Read`]): `{"clauseo": {"skills": {...}}}` names
/// `clauseo.skills`, which locks `skills` by the same rule.
pub(crate) fn typed_policy(
    raws: Vec<Raw>,
    wrapped: &[Raw],
    mut refused: Vec<String>,
    mut warnings: Vec<Warning>,
    places: &Places,
) -> PolicyValues {
    let mut reasons: Vec<LockReason> = Vec::new();
    let key_places = places.of_keys(&raws, &warnings, &refused);
    for (key, origin, shown) in security_keys_misnamed(&raws, wrapped, &warnings, places) {
        let source = SourceKind::of_origin(&origin);
        let place = u32::try_from(shown.place).unwrap_or(u32::MAX);
        // One reason per name: the keys it locks gather under it.
        match reasons.iter_mut().find(|reason| {
            reason.source == source && reason.place == place && reason.section == shown.section
        }) {
            Some(reason) => reason.keys.push(key.to_owned()),
            None => reasons.push(LockReason {
                keys: vec![key.to_owned()],
                cause: LockCause::MisnamedSection,
                source,
                section: shown.section,
                place,
            }),
        }
        let found = format!("`{}`", shown.text);
        warnings.push(Warning {
            origin,
            kind: WarningKind::InvalidValue {
                key: key.to_owned(),
                message: format!(
                    "the policy names {found}, which this version cannot read, so {key} is \
                     locked off; set it as {}",
                    right_shape(key)
                ),
            },
        });
        refused.push(key.to_owned());
    }
    let leaves = typed(raws, refused, &mut warnings);
    PolicyValues::new(leaves, warnings)
        .with_reasons(reasons)
        .with_places(key_places)
}

/// Where each name a policy source holds sits among the source's entries, from 1, in document
/// order: [`LockReason::place`]. The registry's and managed preferences' entries are their
/// values; `policy.json`'s are its top-level entries, so a name inside a section, or nested in
/// an unknown one, takes its section's place (`{"ui": {}, "matters": {}, "Skills=x": {}}`
/// gives `Skills=x` place 3). One list per list [`typed_policy`] takes, item for item; a
/// missing item is its index in that list, from 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    /// For each raw value.
    pub raws: Vec<usize>,
    /// For each warning.
    pub warnings: Vec<usize>,
    /// For each nested name.
    pub wrapped: Vec<usize>,
}

impl Places {
    /// The place of the `at`th item of `list`.
    fn of(list: &[usize], at: usize) -> usize {
        list.get(at).copied().unwrap_or(at + 1)
    }

    /// The place of each key the source sets (its last value's) or refuses (its warning's), for
    /// a [`LockReason`] of a value that does not type. A refused section of entries by id
    /// (`plans.`) takes its section's warning's place.
    fn of_keys(
        &self,
        raws: &[Raw],
        warnings: &[Warning],
        refused: &[String],
    ) -> Vec<(String, usize)> {
        let mut found: Vec<(String, usize)> = Vec::new();
        for (at, raw) in raws.iter().enumerate().rev() {
            if !found.iter().any(|(key, _)| *key == raw.key) {
                found.push((raw.key.clone(), Places::of(&self.raws, at)));
            }
        }
        for (at, warning) in warnings.iter().enumerate() {
            if let WarningKind::InvalidValue { key, .. } = &warning.kind {
                let place = Places::of(&self.warnings, at);
                for key in [key.clone(), format!("{key}.")] {
                    if refused.contains(&key) && !found.iter().any(|(known, _)| *known == key) {
                        found.push((key, place));
                    }
                }
            }
        }
        found
    }
}

/// The first segment of a dotted name: `skill` in `skill.extra`.
fn first_segment(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// Whether the name `name` is about the section `section`. A first segment that is `section`
/// is about it; one that is another known section is about `section` when what follows it is
/// (`ui.tools`, `privacy.mcp`, `matters.install_bundle`: a security section placed where it
/// is not read), by the rule for a name with no known first segment. That name is
/// read shape-free: [`normalised`] (NFKC, format characters dropped, lowercase, nothing but
/// letters and digits), it is about `section` when it holds `section` (`clauseo.skills.extra`,
/// `SkillsExtra`), when a segment at a section's place still holds a character outside ASCII
/// (`ѕkіlls` with Cyrillic letters, failing closed with no table of confusable letters to
/// keep; [`shape_free_about`] says which segments), or when, from the start
/// of the name or of any word in it, it begins with something near `section` ([`starts_near`]:
/// `skill:extra`, `ToolBrowser`, `x.skils.extra`, `skil\u{200b}ls`). A word starts after
/// any character that is not a letter or digit, and at a capital after a small letter. A
/// deliberate over-lock, failing closed: `foo.tools_note`, `toolbar` and `skillful` lock their
/// sections, since what such a name meant is unknown.
pub(crate) fn about_section(name: &str, section: &str) -> bool {
    let segment = first_segment(name);
    if segment == section {
        return true;
    }
    if SECTIONS.contains(&segment) {
        // What follows another known section is read by the same rule: `ui.tools` and
        // `privacy.mcp` name a security section placed where it is not read. A key directly
        // under a section with no security key (`ui`) stands where a section name would.
        let places = usize::from(!holds_security_key(segment));
        return name
            .split_once('.')
            .is_some_and(|(_, rest)| shape_free_about(rest, section, places));
    }
    // The wrapper itself and the key directly under it stand where a section name would.
    shape_free_about(name, section, 2)
}

/// Whether the known section `section` holds a security key ([`SECURITY_KEYS`]).
fn holds_security_key(section: &str) -> bool {
    SECURITY_KEYS
        .iter()
        .any(|key| first_segment(key) == section)
}

/// [`about_section`]'s rule for a name whose first segment is no known section, or for what
/// follows a known one. Only its first `places` segments, which stand where a section name
/// would, lock for a character outside ASCII: a deeper key (`_comment.note.café`, a Hindi
/// word in a note) is read by the ASCII rules alone.
fn shape_free_about(name: &str, section: &str, places: usize) -> bool {
    let whole = normalised(name);
    let foreign = || {
        name.split('.')
            .take(places)
            .any(|segment| !normalised(segment).is_ascii())
    };
    if whole.contains(section) || foreign() {
        return true;
    }
    word_starts(name).any(|at| {
        name.get(at..)
            .is_some_and(|rest| starts_near(&normalised(rest), section))
    })
}

/// `text` in NFKC, without format characters ([`is_format`]), in lowercase, and with every
/// character that is not a letter or digit dropped: `Skill:Extra` is `skillextra`.
fn normalised(text: &str) -> String {
    text.nfkc()
        .filter(|&c| !is_format(c))
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Where each word of `name` starts, in bytes: at 0, after each character that is not a letter
/// or digit (nor a format character), and at each capital that follows a small letter.
fn word_starts(name: &str) -> impl Iterator<Item = usize> + '_ {
    let mut previous: Option<char> = None;
    name.char_indices().filter_map(move |(at, c)| {
        if is_format(c) {
            return None;
        }
        let starts = match previous {
            None => true,
            Some(before) => {
                !before.is_alphanumeric() || (before.is_lowercase() && c.is_uppercase())
            }
        };
        previous = Some(c);
        (starts && c.is_alphanumeric()).then_some(at)
    })
}

/// Whether `text`, already [`normalised`], begins with something within [`misspelt`] edits of
/// `section`: its first `n - 1`, `n` or `n + 1` characters, `n` being `section`'s length
/// (fewer when `text` is shorter).
fn starts_near(text: &str, section: &str) -> bool {
    let length = section.chars().count();
    (length.saturating_sub(1)..=length + 1).any(|take| {
        let prefix: String = text.chars().take(take).collect();
        !prefix.is_empty() && distance(&prefix, section) <= misspelt(section)
    })
}

/// Whether `c` is a format character, Unicode's general category Cf (Unicode 15.1): invisible
/// marks such as the zero-width space, joiners, direction marks and the byte order mark.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0xAD | 0x600..=0x605
            | 0x61C
            | 0x6DD
            | 0x70F
            | 0x890..=0x891
            | 0x8E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// The edits (insert, delete or replace one character, or swap two adjacent ones) that turn
/// `a` into `b`: the optimal string alignment distance, so `mpc` is one edit from `mcp`.
pub(crate) fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    // The rows for `a`'s characters before the last two, before the last one, and up to it.
    let mut before = vec![0; b.len() + 1];
    let mut above: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for (i, &left) in a.iter().enumerate() {
        row[0] = i + 1;
        let previous = i.checked_sub(1).and_then(|at| a.get(at));
        for (j, &right) in b.iter().enumerate() {
            let mut best = (above[j + 1] + 1)
                .min(row[j] + 1)
                .min(above[j] + usize::from(left != right));
            let swapped = j.checked_sub(1).and_then(|at| b.get(at)) == Some(&left)
                && previous == Some(&right);
            if swapped && let Some(at) = j.checked_sub(1) {
                best = best.min(before[at] + 1);
            }
            row[j + 1] = best;
        }
        std::mem::swap(&mut before, &mut above);
        std::mem::swap(&mut above, &mut row);
    }
    above[b.len()]
}

/// A misnamed name as a message and a [`LockReason`] show it.
struct Shown {
    /// The warning's text.
    text: String,
    /// The section, when its first segment is a plain word.
    section: Option<String>,
    /// Its place in the source, from 1.
    place: usize,
}

/// How a message shows the name `name`, the `place`th of its source (from 1): a name inside a
/// known section as [`shown_plan_key`] shows it (a warning's name is shown already); a misspelt
/// section by its first segment when that is a plain word ([`shown_section`]), else as
/// `<section {place}>`, never echoing text that may be pasted (#411's rule).
fn shown_name(name: &str, place: usize, plans_place: Option<usize>) -> Shown {
    let segment = first_segment(name);
    let section = shown_section(segment);
    let text = if SECTIONS.contains(&segment) {
        // A warning's name is shown already; a raw key's `plans.` place is given.
        plans_place.map_or_else(|| name.to_owned(), |at| shown_plan_key(name, at))
    } else {
        section
            .clone()
            .unwrap_or_else(|| format!("<section {place}>"))
    };
    Shown {
        text,
        section,
        place,
    }
}

/// Whether the unknown `name` is the security `key` itself, or for a section of entries by id
/// (`plans.`) one of its keys. A top-level `"plans.claude-acp"` in `policy.json` sets nothing,
/// and its name is a plan key's, which `about` passes over.
fn names(name: &str, key: &str) -> bool {
    name == key || (key.ends_with('.') && name.starts_with(key))
}

/// How a policy sets the security-relevant `key`, for the warning of a misnamed one.
fn right_shape(key: &str) -> String {
    let (section, name) = key.split_once('.').unwrap_or((key, ""));
    format!(r#"{{"{section}": {{"{name}": {{...}}}}}} in policy.json, or the registry value {key}"#)
}

/// Each security-relevant key that `raws` or `warnings` name in a form that does not set it,
/// once, with where and how it was first named.
///
/// A name refuses only the keys it could stand for ([`could_stand_for`]): `tools.websearch`
/// refuses `tools.web_search`, not `tools.browser`. A name that could stand for any key of its
/// section (`"tools": "off"`, `tools.brwser`) refuses each, except a sibling the same source
/// sets validly by its exact name, which keeps its value. A source that cannot be read at all
/// still locks every key off (`effective.rs`).
fn security_keys_misnamed(
    raws: &[Raw],
    wrapped: &[Raw],
    warnings: &[Warning],
    places: &Places,
) -> Vec<(&'static str, Origin, Shown)> {
    let mut found = Vec::new();
    for key in SECURITY_KEYS {
        let section = key.split('.').next().unwrap_or(key);
        // An unknown name about the key's topic, other than an agent's key. This runs before
        // `typed`, so a bad entry inside a valid list (`mcp.servers.<name>`) is not reported
        // yet and stays a warning that drops that entry; a name the firm wrote as
        // `mcp.servers.<name>` itself, at the top level, inside the section or as a registry
        // value, does lock.
        let about = |name: &str| misnamed_as(name, key, raws);
        let from_raws = raws
            .iter()
            .enumerate()
            .find(|(_, raw)| about(&raw.key))
            .map(|(at, raw)| {
                let place = raws
                    .iter()
                    .take(at + 1)
                    .filter(|raw| raw.key.starts_with("plans."))
                    .count();
                (
                    raw.origin.clone(),
                    shown_name(&raw.key, Places::of(&places.raws, at), Some(place)),
                )
            });
        let shown = |name: &str, at| shown_name(name, Places::of(&places.warnings, at), None);
        let from_warnings = || {
            warnings
                .iter()
                .enumerate()
                .find_map(|(at, warning)| match &warning.kind {
                    WarningKind::UnknownKey { key: name } if about(name) || names(name, key) => {
                        Some((warning.origin.clone(), shown(name, at)))
                    }
                    WarningKind::InvalidValue { key: name, .. }
                        if (name == section && !set_validly(raws, key)) || about(name) =>
                    {
                        Some((warning.origin.clone(), shown(name, at)))
                    }
                    WarningKind::UnknownKey { .. }
                    | WarningKind::InvalidValue { .. }
                    | WarningKind::Unreadable { .. }
                    | WarningKind::Syntax { .. }
                    | WarningKind::DuplicateKey
                    | WarningKind::RelativeAgentCommand { .. } => None,
                })
        };
        // A name nested in an unknown section, by its section's place.
        let from_wrapped = || {
            wrapped
                .iter()
                .enumerate()
                .find(|(_, raw)| about(&raw.key))
                .map(|(at, raw)| {
                    let place = Places::of(&places.wrapped, at);
                    (raw.origin.clone(), shown_name(&raw.key, place, None))
                })
        };
        if let Some((origin, name)) = from_raws.or_else(from_warnings).or_else(from_wrapped) {
            found.push((key, origin, name));
        }
    }
    found
}

/// Whether `name`, which a policy source holds, misnames the security key `key` (from
/// `raws`): an unknown name about `key`'s section, other than an agent's key, that could stand
/// for `key`.
fn misnamed_as(name: &str, key: &str, raws: &[Raw]) -> bool {
    let section = key.split('.').next().unwrap_or(key);
    kind_of(name).is_none()
        && !name.starts_with("agents.")
        && about_section(name, section)
        && could_stand_for(name, key, raws)
}

/// Whether the unknown `name`, which holds `key`'s section, could stand for the security key
/// `key`. What the name holds beyond the section, in any case and with no punctuation, is
/// matched against each key of the section: it stands for the keys whose words it holds
/// (`websearch`, `Web-Search`, `web` for `tools.web_search`). One that matches no key of
/// the section, or holds nothing beyond it, could stand for any of them, except a sibling
/// [`set_validly`] by `raws`. A section with one security key (`mcp`) is always it.
fn could_stand_for(name: &str, key: &str, raws: &[Raw]) -> bool {
    let section = key.split('.').next().unwrap_or(key);
    let siblings: Vec<&str> = SECURITY_KEYS
        .into_iter()
        .filter(|other| other.split('.').next() == Some(section))
        .collect();
    if siblings.len() < 2 {
        return true;
    }
    // What the name holds beyond its section: the first segment less the section's name (or
    // nothing, for a misspelt one), then the rest of the name.
    let lower = name.to_ascii_lowercase();
    let (segment, tail) = lower.split_once('.').unwrap_or((&lower, ""));
    let head = if segment.contains(section) {
        segment.replacen(section, "", 1)
    } else {
        String::new()
    };
    let rest: String = head
        .chars()
        .chain(tail.chars())
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let holds = |other: &str| {
        !rest.is_empty()
            && other
                .split_once('.')
                .map_or("", |(_, leaf)| leaf)
                .split('_')
                .any(|word| !word.is_empty() && rest.contains(word))
    };
    if siblings.iter().any(|other| holds(other)) {
        holds(key)
    } else {
        !set_validly(raws, key)
    }
}

/// Whether `raws` set `key` by its exact name, the last time to a value that types.
fn set_validly(raws: &[Raw], key: &str) -> bool {
    raws.iter()
        .rev()
        .find(|raw| raw.key == key)
        .is_some_and(|raw| Leaf::parse(key, raw.value.clone(), &mut Vec::new()).is_ok())
}
