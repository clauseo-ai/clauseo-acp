//! Adding a skill from where a lawyer finds it (decision record 0040, "Adding from a link"),
//! in two steps so the lawyer sees what and from where before anything is added:
//! `HostQuery::PreviewSkill` (answered [`SkillPreview`]) then `HostCommand::AddSkills`; and
//! the skills.sh catalogue's search, `HostQuery::SearchSkills` (answered [`SkillSearch`]).
//!
//! View types: the app and the host ship together, so every enum here is closed (decision
//! record 0002).

use serde::{Deserialize, Serialize};

use crate::{AddonFrom, Error};

/// The most skills one `AddSkills` picks (a pack's own cap on skills is 32 per pack; each
/// picked skill is its own pack, so this bounds one command's work).
pub(crate) const MAX_PICKS: usize = 64;

/// The most bytes of a picked name or a token.
const NAME_MAX: usize = 200;

/// `AddSkills`' shape: a token of 1 to 200 bytes, 1 to [`MAX_PICKS`] names, each 1 to 200
/// bytes. Whether the preview holds the names is state, the host's to check.
pub(crate) fn check_add(token: &str, pick: &[String]) -> Result<(), Error> {
    let bad = |rule| {
        Err(Error::Format {
            kind: "AddSkills",
            rule,
        })
    };
    if token.is_empty() || token.len() > NAME_MAX {
        return bad("the token is 1 to 200 bytes");
    }
    if pick.is_empty() || pick.len() > MAX_PICKS {
        return bad("pick names 1 to 64 skills");
    }
    if pick
        .iter()
        .any(|name| name.is_empty() || name.len() > NAME_MAX)
    {
        return bad("each picked name is 1 to 200 bytes");
    }
    Ok(())
}

/// Where a preview reads from. JSON: `{"skills_sh": "anthropics/skills"}`,
/// `{"url": "https://github.com/owner/repo"}` or `{"path": "<what pick_skill returned>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SkillFrom {
    /// A skills.sh name: `owner/repo`, `owner/repo/skill`, `owner/repo@ref` or
    /// `owner/repo@ref/skill`. Read from GitHub's archive of the repository.
    SkillsSh(String),
    /// An `https` link: a GitHub repository (optionally `/tree/<ref>/<path>`), a skills.sh
    /// page, or a `.zip`.
    Url(String),
    /// A folder, a `SKILL.md` or a `.zip` on this computer, as `pick_skill` returned it.
    Path(String),
}

/// `PreviewSkill`'s answer: the token `AddSkills` names it by (it lasts 30 minutes), where it
/// came from, and each skill found that can be added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkillPreview {
    /// The preview's token.
    pub token: String,
    /// Where it came from, as the list's `from` will say it.
    pub from: AddonFrom,
    /// The skills found, in the order found.
    pub skills: Vec<PreviewedSkill>,
}

/// One skill a preview found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PreviewedSkill {
    /// Its `SKILL.md` `name`; `AddSkills`' `pick` names it by this.
    pub name: String,
    /// Its `SKILL.md` `description`, on one line, at most 200 characters.
    pub description: String,
    /// Whether it holds a file an agent could run (anything but Markdown, text and images, or a
    /// file marked executable).
    pub has_scripts: bool,
    /// Whether a skill added before holds its id: adding it replaces that one.
    pub already_added: bool,
}

/// `SearchSkills`' answer: at most 30 catalogue items, most relevant first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkillSearch {
    /// The items.
    pub items: Vec<CatalogueItem>,
}

/// One skills.sh catalogue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CatalogueItem {
    /// `owner/repo/skill`: `PreviewSkill {"skills_sh": id}` previews it.
    pub id: String,
    /// Its name.
    pub name: String,
    /// `owner/repo`, the GitHub repository it comes from.
    pub source: String,
    /// How many times it was installed, as skills.sh counts (capped at `u32::MAX`, so it reads
    /// as a plain number in TypeScript).
    pub installs: u32,
    /// Its page: `https://skills.sh/<id>`.
    pub page: String,
}
