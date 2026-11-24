//! Adding skills from where a lawyer finds them (decision record 0040, "Adding from a link"): a
//! folder, a `SKILL.md`, a `.zip`, a GitHub repository or link, or a skills.sh name.
//!
//! - [`remote`] parses a link or a skills.sh name into what to download ([`Remote`]); the host
//!   downloads it, on the lawyer's own action only.
//! - [`unzip`] unpacks an archive into a staging folder under the archive caps, refusing any
//!   entry that would land outside it, a link, or a bomb.
//! - [`discover`] finds every skill in a tree as the skills.sh installer does.
//! - [`local`] previews a folder, a `SKILL.md` or a `.zip` on this computer.
//! - [`stage`] keeps a preview's tree under `<data>/skill-packs.adding/<token>/` until the
//!   lawyer picks, and installs a picked skill through `added`, recording where it came from.
//!
//! Nothing here touches the network or starts a process.

pub mod discover;
pub mod local;
pub mod remote;
pub mod stage;
pub mod unzip;

pub use discover::{Found, discover, narrow};
pub use local::preview_local;
pub use remote::{GitHub, Remote, parse_link, parse_skills_sh};
pub use stage::{
    ADDING_DIR, Installed, Preview, STAGE_TTL_MS, Shown, Staged, Tree, close_preview,
    install_picked, open_preview, stage_tree, sweep,
};
pub use unzip::{Unpacked, unzip};

/// The most bytes one download or archive has (the skills.sh installer's cap): 10 MiB.
pub const DOWNLOAD_MAX: u64 = 10 * 1024 * 1024;

/// The most bytes an archive unpacks to, all entries together: 25 MiB.
pub const UNPACKED_MAX: u64 = 25 * 1024 * 1024;

/// The most entries (files and folders) an archive holds.
pub const ENTRIES_MAX: usize = 1000;

/// Why a skill source gave no skills, with the error code each answers in brackets. Closed: the
/// host maps every variant to a code, so a new one does not compile until it is given one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    /// The link or name is not one Clauseo takes (`command.invalid`).
    BadSource,
    /// The download failed: offline, the proxy, a TLS failure, a 5xx, a timeout. Worth a
    /// retry (`skill.download_failed`).
    DownloadFailed,
    /// A 404, or the tree holds no skill, or none by the asked name (`skill.not_found`).
    NotFound,
    /// Past [`DOWNLOAD_MAX`], [`UNPACKED_MAX`] or [`ENTRIES_MAX`], or the skill past its own
    /// caps (`skill.too_big`).
    TooBig,
    /// The archive is not a zip, or an entry is refused: a path outside the folder, a link, a
    /// name a pack cannot hold (`skill.invalid`).
    Unsafe,
    /// The preview's token is unknown or past [`STAGE_TTL_MS`] (`skill.preview_expired`).
    Expired,
    /// A local read or write failed (`skill.invalid`).
    Io,
}

impl SourceError {
    /// A short code for a log line (never the link, a path or a skill's text).
    pub fn code(self) -> &'static str {
        match self {
            Self::BadSource => "bad_source",
            Self::DownloadFailed => "download_failed",
            Self::NotFound => "not_found",
            Self::TooBig => "too_big",
            Self::Unsafe => "unsafe",
            Self::Expired => "expired",
            Self::Io => "io",
        }
    }
}

/// How a skill reached this computer, for the list's `from` (`from.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromKind {
    /// A skills.sh name.
    SkillsSh,
    /// A GitHub repository or link.
    GitHub,
    /// Any other https link to a `.zip`.
    Url,
    /// A `.zip` on this computer.
    Zip,
    /// A folder or a `SKILL.md` on this computer (and every skill added before D3).
    Folder,
}

impl FromKind {
    /// The wire and manifest form: `skills_sh`, `github`, `url`, `zip` or `folder`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SkillsSh => "skills_sh",
            Self::GitHub => "github",
            Self::Url => "url",
            Self::Zip => "zip",
            Self::Folder => "folder",
        }
    }

    /// [`Self::as_str`] read back.
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::SkillsSh,
            Self::GitHub,
            Self::Url,
            Self::Zip,
            Self::Folder,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }
}

/// Where a preview's skills came from: what the screen shows, and what the manifest records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The kind.
    pub kind: FromKind,
    /// What the screen shows: `owner/repo[/skill]` for skills.sh, `owner/repo[/path]` for
    /// GitHub, the link for a `.zip` link, the file or folder name for a local source.
    pub text: String,
    /// The `https` link recorded in the manifest's `source_url`: the skills.sh page, the GitHub
    /// link or the `.zip` link. `None` for a local source.
    pub source_url: Option<String>,
}

impl Origin {
    /// A local source: a folder, a `SKILL.md` or a `.zip`, shown by its last name.
    pub fn local(kind: FromKind, path: &std::path::Path) -> Self {
        let text = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            kind,
            text,
            source_url: None,
        }
    }
}
