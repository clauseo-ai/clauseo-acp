//! A skill's link or skills.sh name, parsed into what the host downloads. Pure.
//!
//! - `https://github.com/<owner>/<repo>`, optionally `/tree/<ref>/<path>` (the first segment
//!   after `tree` is the ref, so a branch with a `/` in its name is not reachable this way) or
//!   `/blob/<ref>/<path>/SKILL.md` (narrowed to the folder holding it): GitHub's zip archive of
//!   the ref, the default branch (`HEAD`) when there is none.
//! - `https://skills.sh/<owner>/<repo>[/<skill>]`, and the bare name forms [`parse_skills_sh`]
//!   takes: the same GitHub archive, narrowed to the skill.
//! - Any other `https` link whose path ends in `.zip`: that file.
//!
//! No port, no user info, no plain `http`. The skills.sh mapping is [`skills_sh_repo`] alone.

use url::Url;

use super::{FromKind, Origin, SourceError};

/// GitHub's archive host.
pub const CODELOAD: &str = "https://codeload.github.com";

/// The skills.sh site, for the page a skill added from it records.
const SKILLS_SH: &str = "https://skills.sh";

/// The most bytes a link has (the manifest's `source_url` cap).
const LINK_MAX: usize = super::super::manifest::URL_MAX;

/// What to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    /// A GitHub repository's archive, narrowed.
    GitHub(GitHub),
    /// A `.zip` at this `https` link.
    Zip {
        /// The link.
        url: String,
    },
}

/// A GitHub repository, the ref and the narrowing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHub {
    /// The owner (user or organisation).
    pub owner: String,
    /// The repository, without `.git`.
    pub repo: String,
    /// The branch, tag or commit; `None` is the default branch.
    pub git_ref: Option<String>,
    /// The folder inside the repository to look in, `/`-separated, never empty.
    pub path: Option<String>,
    /// The skill asked for by name (skills.sh's `skillId`): the folder whose `SKILL.md` `name`
    /// or whose folder name is this.
    pub skill: Option<String>,
    /// Whether the lawyer gave a skills.sh name or page (for `from`).
    pub via_skills_sh: bool,
}

impl GitHub {
    /// The archive's link: `https://codeload.github.com` and [`Self::archive_path`].
    pub fn archive_url(&self) -> String {
        format!("{CODELOAD}{}", self.archive_path())
    }

    /// The archive's path on its host: `/<owner>/<repo>/zip/<ref, or HEAD for the default>`.
    pub fn archive_path(&self) -> String {
        let git_ref = self.git_ref.as_deref().unwrap_or("HEAD");
        format!("/{}/{}/zip/{git_ref}", self.owner, self.repo)
    }

    /// What the screen shows and the manifest records.
    pub fn origin(&self) -> Origin {
        let mut text = format!("{}/{}", self.owner, self.repo);
        if self.via_skills_sh {
            if let Some(skill) = &self.skill {
                text = format!("{text}/{skill}");
            }
            let source_url = Some(format!("{SKILLS_SH}/{text}"));
            if let Some(git_ref) = &self.git_ref {
                text = format!("{text}@{git_ref}");
            }
            return Origin {
                kind: FromKind::SkillsSh,
                text,
                source_url,
            };
        }
        let mut link = format!("https://github.com/{}/{}", self.owner, self.repo);
        if self.git_ref.is_some() || self.path.is_some() {
            let git_ref = self.git_ref.as_deref().unwrap_or("HEAD");
            link = format!("{link}/tree/{git_ref}");
            if let Some(path) = &self.path {
                link = format!("{link}/{path}");
                text = format!("{text}/{path}");
            }
        }
        Origin {
            kind: FromKind::GitHub,
            text,
            source_url: Some(link),
        }
    }
}

impl Remote {
    /// The link the host downloads first.
    pub fn download_url(&self) -> String {
        match self {
            Self::GitHub(github) => github.archive_url(),
            Self::Zip { url } => url.clone(),
        }
    }

    /// What the screen shows and the manifest records.
    pub fn origin(&self) -> Origin {
        match self {
            Self::GitHub(github) => github.origin(),
            Self::Zip { url } => {
                let shown = without_query(url);
                Origin {
                    kind: FromKind::Url,
                    text: shown.clone(),
                    source_url: Some(shown),
                }
            }
        }
    }
}

/// `link` without its query and fragment: a signed URL's token stays out of what is shown and
/// recorded (the download still uses the whole link).
fn without_query(link: &str) -> String {
    match Url::parse(link) {
        Ok(mut url) => {
            url.set_query(None);
            url.set_fragment(None);
            url.as_str().to_owned()
        }
        Err(_bad) => link.split(['?', '#']).next().unwrap_or_default().to_owned(),
    }
}

/// A skills.sh name: `<owner>/<repo>`, `<owner>/<repo>/<skill>`, `<owner>/<repo>@<ref>` or
/// `<owner>/<repo>@<ref>/<skill>`.
pub fn parse_skills_sh(text: &str) -> Result<Remote, SourceError> {
    let parts: Vec<&str> = text.trim().split('/').collect();
    let (owner, repo, skill) = match parts.as_slice() {
        [owner, repo] => (*owner, *repo, None),
        [owner, repo, skill] => (*owner, *repo, Some(*skill)),
        _ => return Err(SourceError::BadSource),
    };
    let (repo, git_ref) = match repo.split_once('@') {
        Some((repo, git_ref)) => (repo, Some(git_ref)),
        None => (repo, None),
    };
    if git_ref.is_some_and(|git_ref| !is_ref(git_ref) || git_ref.contains('/')) {
        return Err(SourceError::BadSource);
    }
    if skill.is_some_and(|skill| !is_skill_id(skill)) {
        return Err(SourceError::BadSource);
    }
    let mut github = skills_sh_repo(owner, repo, skill)?;
    github.git_ref = git_ref.map(str::to_owned);
    Ok(Remote::GitHub(github))
}

/// The one mapping from a skills.sh `<owner>/<repo>[/<skill>]` to what is downloaded:
/// GitHub's archive of `github.com/<owner>/<repo>`, narrowed to the skill. Never skills.sh's
/// own download API, and nothing is ever sent to skills.sh.
fn skills_sh_repo(owner: &str, repo: &str, skill: Option<&str>) -> Result<GitHub, SourceError> {
    if !is_owner(owner) || !is_repo(repo) {
        return Err(SourceError::BadSource);
    }
    Ok(GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        git_ref: None,
        path: None,
        skill: skill.map(str::to_owned),
        via_skills_sh: true,
    })
}

/// An `https` link: GitHub, skills.sh or a `.zip`.
pub fn parse_link(text: &str) -> Result<Remote, SourceError> {
    let text = text.trim();
    if text.len() > LINK_MAX {
        return Err(SourceError::BadSource);
    }
    let url = Url::parse(text).map_err(|_bad| SourceError::BadSource)?;
    let plain = url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !plain {
        return Err(SourceError::BadSource);
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let segments: Vec<&str> = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();
    match host.as_str() {
        "github.com" | "www.github.com" => github_link(&segments),
        "skills.sh" | "www.skills.sh" => match segments.as_slice() {
            [owner, repo] => parse_skills_sh(&format!("{owner}/{repo}")),
            [owner, repo, skill] => parse_skills_sh(&format!("{owner}/{repo}/{skill}")),
            _ => Err(SourceError::BadSource),
        },
        _ if url.path().to_ascii_lowercase().ends_with(".zip") => Ok(Remote::Zip {
            url: url.as_str().to_owned(),
        }),
        _ => Err(SourceError::BadSource),
    }
}

/// A `github.com` link's path segments.
fn github_link(segments: &[&str]) -> Result<Remote, SourceError> {
    let (owner, repo, rest) = match segments {
        [owner, repo, rest @ ..] => (*owner, repo.strip_suffix(".git").unwrap_or(repo), rest),
        _ => return Err(SourceError::BadSource),
    };
    if !is_owner(owner) || !is_repo(repo) {
        return Err(SourceError::BadSource);
    }
    let (git_ref, path) = match rest {
        [] => (None, Vec::new()),
        ["tree", git_ref, path @ ..] => (Some(*git_ref), path.to_vec()),
        ["blob", git_ref, folder @ .., last] if last.eq_ignore_ascii_case("SKILL.md") => {
            (Some(*git_ref), folder.to_vec())
        }
        _ => return Err(SourceError::BadSource),
    };
    if git_ref.is_some_and(|git_ref| !is_ref(git_ref)) {
        return Err(SourceError::BadSource);
    }
    if !path.iter().all(|part| is_path_part(part)) {
        return Err(SourceError::BadSource);
    }
    Ok(Remote::GitHub(GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        git_ref: git_ref.map(str::to_owned),
        path: (!path.is_empty()).then(|| path.join("/")),
        skill: None,
        via_skills_sh: false,
    }))
}

/// A GitHub owner: 1 to 39 ASCII letters, digits and `-`, not starting with `-`.
fn is_owner(text: &str) -> bool {
    (1..=39).contains(&text.len())
        && !text.starts_with('-')
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A GitHub repository: 1 to 100 ASCII letters, digits, `.`, `_` and `-`, not `.` or `..`.
fn is_repo(text: &str) -> bool {
    (1..=100).contains(&text.len())
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A ref: 1 to 200 ASCII letters, digits, `.`, `_`, `-` and `/`, no `..`, no leading `-`.
fn is_ref(text: &str) -> bool {
    (1..=200).contains(&text.len())
        && !text.starts_with('-')
        && !text.contains("..")
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

/// A skills.sh `skillId`: 1 to 64 ASCII letters, digits, `.`, `_` and `-`, not dot-led.
fn is_skill_id(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A folder name inside the repository: [`is_skill_id`]'s charset, up to 100 bytes, no `..`.
fn is_path_part(text: &str) -> bool {
    (1..=100).contains(&text.len())
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
