//! R22's built-in skills: `legal-drafting`, `legal-research` and `matter-brief`, embedded from
//! `assets/skills/` and rendered by [`SkillSource::builtin`] beside R21's `stetdoc`, plus the
//! skills index Codex gets in `developer_instructions` (decision record 0031).
//!
//! The renderer and its cleanup act on [`BUILTIN`] alone: [`sources`] yields those three and no
//! other, and a write touches only the skills it is given, so the `stetdoc` folder, its manifest
//! entry and every other folder stay byte for byte.

use std::path::Path;

use super::{Rendered, SKILL_MD, SkillFile, SkillSource, stetdoc::STETDOC};
use crate::error::ProfileError;
use crate::placeholders::IN_INSTRUCTIONS;

/// R22's three skills, by name: the fixed list the renderer and cleanup act on.
pub const BUILTIN: [&str; 3] = ["legal-drafting", "legal-research", "matter-brief"];

/// Built-in skills a past Clauseo shipped and this one does not: `write_skills` removes their
/// files and manifest entries ([`super::retire`]). Empty at launch. A name goes here when it
/// leaves [`BUILTIN`], and never names `stetdoc` or a current built-in (tested).
pub const RETIRED_BUILTINS: [&str; 0] = [];

/// Every source file of the three, as `(skill, path, text)`. A test checks that this list is
/// exactly the files under `assets/skills/<name>/`.
pub const ASSETS: [(&str, &str, &str); 4] = [
    (
        "legal-drafting",
        SKILL_MD,
        include_str!("../../assets/skills/legal-drafting/SKILL.md"),
    ),
    (
        "legal-drafting",
        "references/section-checklists.md",
        include_str!("../../assets/skills/legal-drafting/references/section-checklists.md"),
    ),
    (
        "legal-research",
        SKILL_MD,
        include_str!("../../assets/skills/legal-research/SKILL.md"),
    ),
    (
        "matter-brief",
        SKILL_MD,
        include_str!("../../assets/skills/matter-brief/SKILL.md"),
    ),
];

/// The one-line use the Codex index gives each skill Clauseo renders: the three and stetdoc's.
const USES: [(&str, &str); 4] = [
    (
        "legal-drafting",
        "drafting or revising a legal document, or marking up a draft",
    ),
    (
        "legal-research",
        "Indian legal research that needs more than one search",
    ),
    (
        "matter-brief",
        "reading and updating the matter brief when work starts and before a turn ends",
    ),
    (STETDOC, "reading and editing Word files as tracked changes"),
];

/// The index's heading and its one rule.
const INDEX_HEAD: &str = "## Skills\n\nYour skills are files on this computer. When a task fits \
                          one, read the file before you start, and the references it names, and \
                          follow it.";

/// The source files of `name`, one of [`BUILTIN`], as they are in `assets/skills/<name>/`.
pub fn source_files(name: &str) -> Vec<SkillFile> {
    ASSETS
        .iter()
        .filter(|(skill, _, _)| *skill == name)
        .map(|(_, path, text)| SkillFile {
            path: (*path).to_owned(),
            bytes: text.as_bytes().to_vec(),
        })
        .collect()
}

/// The three as skill sources, in [`BUILTIN`]'s order: each `SKILL.md`'s frontmatter parsed with
/// the crate's parser (`metadata.clauseo` included) and rendered as `name` and `description`
/// alone. Pure. An error is a bug in the shipped text, which the tests catch.
pub fn sources() -> Result<Vec<SkillSource>, ProfileError> {
    BUILTIN
        .iter()
        .map(|name| SkillSource::builtin(name, source_files(name)))
        .collect()
}

/// stetdoc's skill (when it loaded), then the three: the list `ClauseoProfile::with_skills`
/// renders. The three's error, if any, comes back beside the list, which then holds stetdoc's
/// alone.
pub fn with_stetdoc(stetdoc: Option<SkillSource>) -> (Vec<SkillSource>, Vec<ProfileError>) {
    let mut skills: Vec<SkillSource> = stetdoc.into_iter().collect();
    match sources() {
        Ok(builtin) => {
            skills.extend(builtin);
            (skills, Vec::new())
        }
        Err(error) => {
            tracing::warn!(%error, "the built-in legal skills are left out");
            (skills, vec![error])
        }
    }
}

/// The one-line use the index gives `name`, if it has one.
pub fn use_of(name: &str) -> Option<&'static str> {
    USES.iter()
        .find(|(skill, _)| *skill == name)
        .map(|(_, line)| *line)
}

/// The skills index for Codex: a heading, one rule, then for each skill in `rendered` (the list
/// the renderer renders, in its order) its name, its use and the path of its rendered `SKILL.md`
/// under `skills_root` (the Claude plugin's `skills/`, where `write_rendered` puts it). Empty
/// when nothing is rendered. Built per call, from the paths given, so nothing is cached.
///
/// A path that is not UTF-8, holds a control or format character or a Unicode line or
/// paragraph separator, or holds a placeholder the host fills in the instructions is left out
/// with a warning: the first would not name the file, the second could break or hide part of
/// the list for a reader or a tokenizer, the third would be filled by the host. The code span
/// only neutralises Markdown: the model still reads the path's words, which the lawyer's data
/// folder chooses (decision record 0031).
pub fn codex_index(skills_root: &Path, rendered: &Rendered) -> String {
    let mut lines = Vec::new();
    for skill in rendered.skills() {
        let path = skills_root.join(skill.name()).join(SKILL_MD);
        let Some(path) = path.to_str().filter(|path| is_listable(path)) else {
            tracing::warn!(
                skill = skill.name(),
                "a skill's path cannot go in Codex's index; it is left out"
            );
            continue;
        };
        let line = match use_of(skill.name()) {
            Some(use_line) => format!("- {} ({use_line}): {}", skill.name(), code_span(path)),
            None => format!("- {}: {}", skill.name(), code_span(path)),
        };
        lines.push(line);
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("{INDEX_HEAD}\n\n{}", lines.join("\n"))
}

/// Whether `path` may go in the index (see [`codex_index`]).
fn is_listable(path: &str) -> bool {
    !path
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || is_format(c))
        && !IN_INSTRUCTIONS.iter().any(|token| path.contains(token))
}

/// Whether `c` is a Unicode format character (general category Cf): bidi controls, zero-width
/// characters, the soft hyphen, the byte order mark, tags and the like. Unicode 16's list.
fn is_format(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{0600}'..='\u{0605}'
        | '\u{061C}'
        | '\u{06DD}'
        | '\u{070F}'
        | '\u{0890}'..='\u{0891}'
        | '\u{08E2}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{110BD}'
        | '\u{110CD}'
        | '\u{13430}'..='\u{1343F}'
        | '\u{1BCA0}'..='\u{1BCA3}'
        | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0001}'
        | '\u{E0020}'..='\u{E007F}'
    )
}

/// `text` as a Markdown code span: a backtick fence one longer than the longest backtick run in
/// `text`, with a space inside each fence when `text` starts or ends with a backtick or a space
/// (a `CommonMark` reader strips one such space from each side).
pub fn code_span(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with(['`', ' ']) || text.ends_with(['`', ' ']) {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_span_fence_outruns_every_backtick_run() {
        assert_eq!(code_span("a b"), "`a b`");
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("a``b`c"), "```a``b`c```");
        assert_eq!(code_span("`a"), "`` `a ``");
        assert_eq!(code_span("a "), "` a  `");
    }

    #[test]
    fn every_builtin_has_a_use_line_and_sources() {
        for name in BUILTIN.iter().chain([&STETDOC]) {
            assert!(use_of(name).is_some(), "{name}");
        }
        for name in BUILTIN {
            assert!(source_files(name).iter().any(|file| file.path == SKILL_MD));
        }
    }
}
