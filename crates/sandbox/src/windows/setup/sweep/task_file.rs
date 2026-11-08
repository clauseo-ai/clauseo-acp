//! A task's principal from its file, `%SystemRoot%\System32\Tasks\<folder>\<name>`, for a task
//! the Task Scheduler can't read for the sweep: the path's parts, each held to a plain name
//! ([`parts`]), and a narrow read of the file's XML ([`principal_of`]). The read is no XML
//! parser: no DTD, no entity but the five predefined ones and character references, at most
//! [`TASK_FILE_CAP`] bytes, and only `Task/Principals/Principal/UserId` and `GroupId` kept.
//! Anything it can't read is malformed: the task stays unreadable, never "not the pool's".

use super::{Failed, Principal};

/// The most bytes a task file may hold.
pub const TASK_FILE_CAP: usize = 64 * 1024;

/// The deepest element nesting read.
const DEPTH_MAX: usize = 64;

/// The element paths read: `UserId` and `GroupId` under the task's one principal.
const USER: [&str; 4] = ["Task", "Principals", "Principal", "UserId"];
const GROUP: [&str; 4] = ["Task", "Principals", "Principal", "GroupId"];
const PRINCIPAL: [&str; 3] = ["Task", "Principals", "Principal"];

/// The parts of the task file's path below `Tasks`: each folder of `folder` (`\` the root,
/// `\A\B` below it), then `name`. Every part must be a plain name: not empty, no `..`, no `:`,
/// no path separator, no character Windows refuses in a name and no control character. The
/// names come from the Task Scheduler, which a pool user's task feeds, so none is trusted.
pub fn parts(folder: &str, name: &str) -> Result<Vec<String>, Failed> {
    let below = folder
        .strip_prefix('\\')
        .ok_or_else(|| "the task's folder path doesn't start at the root".to_owned())?;
    let mut parts: Vec<String> = if below.is_empty() {
        Vec::new()
    } else {
        below.split('\\').map(str::to_owned).collect()
    };
    parts.push(name.to_owned());
    if parts.iter().all(|part| plain(part)) {
        Ok(parts)
    } else {
        Err("the task's folder or name isn't a plain name".to_owned())
    }
}

/// Whether `part` is one plain path part, as [`parts`] says.
fn plain(part: &str) -> bool {
    !part.is_empty()
        && part.chars().count() <= 255
        && part != "."
        && !part.contains("..")
        && !part.chars().any(|c| {
            c.is_control() || matches!(c, ':' | '\\' | '/' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

/// The principal a task file names, as the module says: UTF-16 little-endian after its byte
/// order mark (as the Task Scheduler writes it), or UTF-8; one `Task` root, at most one
/// `Principal`, each of `UserId` and `GroupId` at most once, their text without surrounding
/// white space. `Err` for a file over [`TASK_FILE_CAP`] or anything malformed.
pub fn principal_of(bytes: &[u8]) -> Result<Principal, Failed> {
    if bytes.len() > TASK_FILE_CAP {
        return Err("the task file is larger than its cap".to_owned());
    }
    principal_in(&decoded(bytes)?)
}

/// `bytes` as text: UTF-16LE after `FF FE`, else UTF-8 (after `EF BB BF` or none); no NUL.
fn decoded(bytes: &[u8]) -> Result<String, Failed> {
    let malformed = |error: &dyn std::fmt::Display| {
        format!("the task file isn't UTF-16LE or UTF-8 text ({error})")
    };
    let text = if let Some(wide) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (pairs, odd) = wide.as_chunks::<2>();
        if !odd.is_empty() {
            return Err(malformed(&"an odd length"));
        }
        char::decode_utf16(pairs.iter().map(|pair| u16::from_le_bytes(*pair)))
            .collect::<Result<String, _>>()
            .map_err(|error| malformed(&error))?
    } else {
        let utf8 = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8(utf8.to_vec()).map_err(|error| malformed(&error))?
    };
    if text.contains('\0') {
        return Err(malformed(&"a NUL"));
    }
    Ok(text)
}

/// What the read found so far.
#[derive(Default)]
struct Read<'x> {
    /// The open elements, outermost first.
    stack: Vec<&'x str>,
    /// Whether the root has been opened.
    rooted: bool,
    /// `Principal` elements seen.
    principals: usize,
    /// The text of the `UserId` or `GroupId` open now.
    text: String,
    user: Option<String>,
    group: Option<String>,
}

impl Read<'_> {
    /// Whether the element open now is `path`.
    fn at(&self, path: &[&str]) -> bool {
        self.stack == path
    }

    /// Keeps `text`, decoded, when inside `UserId` or `GroupId`; refuses text outside the root.
    fn text(&mut self, raw: &str, decode: bool) -> Result<(), Failed> {
        let text = if decode {
            unescaped(raw)?
        } else {
            raw.to_owned()
        };
        if self.stack.is_empty() {
            if !text.chars().all(xml_space) {
                return Err(malformed("text outside the task"));
            }
        } else if self.at(&USER) || self.at(&GROUP) {
            self.text.push_str(&text);
        }
        Ok(())
    }

    /// Closes `name`, keeping a `UserId` or `GroupId`'s text.
    fn close(&mut self, name: &str) -> Result<(), Failed> {
        let slot = if self.at(&USER) {
            Some(&mut self.user)
        } else if self.at(&GROUP) {
            Some(&mut self.group)
        } else {
            None
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return Err(malformed("a user or group named twice"));
            }
            *slot = Some(
                std::mem::take(&mut self.text)
                    .trim_matches(xml_space)
                    .to_owned(),
            );
        }
        match self.stack.pop() {
            Some(open) if open == name => Ok(()),
            _ => Err(malformed("an end tag that matches no open element")),
        }
    }
}

/// A malformed file's reason.
fn malformed(why: &str) -> Failed {
    format!("the task file is malformed: {why}")
}

/// Whether `c` is XML white space.
fn xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// The principal `xml` names, as [`principal_of`] says.
fn principal_in(xml: &str) -> Result<Principal, Failed> {
    let mut read = Read::default();
    let mut rest = xml;
    while !rest.is_empty() {
        let Some((text, markup)) = rest.split_once('<') else {
            read.text(rest, true)?;
            break;
        };
        read.text(text, true)?;
        rest = if let Some(after) = markup.strip_prefix('?') {
            skip(after, "?>")?
        } else if let Some(after) = markup.strip_prefix("!--") {
            skip(after, "-->")?
        } else if let Some(after) = markup.strip_prefix("![CDATA[") {
            let (data, after) = after
                .split_once("]]>")
                .ok_or_else(|| malformed("an unended CDATA"))?;
            if read.stack.is_empty() {
                return Err(malformed("CDATA outside the task"));
            }
            read.text(data, false)?;
            after
        } else if markup.starts_with('!') {
            return Err(malformed("a DTD or declaration"));
        } else if let Some(after) = markup.strip_prefix('/') {
            let (name, after) = after
                .split_once('>')
                .ok_or_else(|| malformed("an unended tag"))?;
            read.close(name.trim_end_matches(xml_space))?;
            after
        } else {
            open(&mut read, markup)?
        };
    }
    if !read.rooted || !read.stack.is_empty() {
        return Err(malformed("no task, or an element left open"));
    }
    Ok(Principal {
        user_id: read.user.unwrap_or_default(),
        group_id: read.group.unwrap_or_default(),
    })
}

/// What follows `end` in `after`.
fn skip<'x>(after: &'x str, end: &str) -> Result<&'x str, Failed> {
    after
        .split_once(end)
        .map(|(_, rest)| rest)
        .ok_or_else(|| malformed("an unended comment or instruction"))
}

/// Opens the element whose start tag begins `after` (just past its `<`); what follows the tag.
fn open<'x>(read: &mut Read<'x>, after: &'x str) -> Result<&'x str, Failed> {
    let mut quote = None;
    let mut end = None;
    for (at, c) in after.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (_, '<') => return Err(malformed("a < inside a tag")),
            (None, '>') => {
                end = Some(at);
                break;
            }
            _ => {}
        }
    }
    let unended = || malformed("an unended tag");
    let (tag, after) = after
        .split_at_checked(end.ok_or_else(unended)?)
        .ok_or_else(unended)?;
    let after = after.strip_prefix('>').ok_or_else(unended)?;
    let (tag, empty) = match tag.strip_suffix('/') {
        Some(tag) => (tag, true),
        None => (tag, false),
    };
    let name = tag.split(|c: char| xml_space(c)).next().unwrap_or_default();
    let named = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    if !named {
        return Err(malformed("a tag with no name"));
    }
    if read.stack.is_empty() {
        if read.rooted || name != "Task" {
            return Err(malformed("not one Task root"));
        }
        read.rooted = true;
    }
    if read.at(&USER) || read.at(&GROUP) {
        return Err(malformed("an element inside a user or group"));
    }
    if read.stack.len() >= DEPTH_MAX {
        return Err(malformed("elements nested too deep"));
    }
    read.stack.push(name);
    if read.at(&PRINCIPAL) {
        read.principals += 1;
        if read.principals > 1 {
            return Err(malformed("more than one principal"));
        }
    }
    if empty {
        read.close(name)?;
    }
    Ok(after)
}

/// `raw` with its references replaced: the five predefined entities and character references;
/// any other `&` is malformed (no entity is ever defined or expanded).
fn unescaped(raw: &str) -> Result<String, Failed> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let (reference, after) = after
            .split_once(';')
            .filter(|(reference, _)| reference.len() <= 10)
            .ok_or_else(|| malformed("an unended reference"))?;
        let c = match reference {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = reference.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(decimal) = reference.strip_prefix('#') {
                    decimal.parse().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
                    .filter(|c| *c != '\0')
                    .ok_or_else(|| malformed("an entity that isn't predefined"))?
            }
        };
        out.push(c);
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}
