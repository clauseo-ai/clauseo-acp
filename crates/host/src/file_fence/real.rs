//! Where a path an agent's file tool names really lands, before the tool runs: every reading of
//! the agent's text the tool might take ([`readings`]), each walked to its real location by the
//! shared network-safe walk on the real disk ([`crate::network_paths::resolve::walk`] with
//! [`Disk::real`]): every link followed (a dangling one too), `..` taken as the OS takes it,
//! each existing name put in its canonical form (`dunce::canonicalize`: on Windows
//! `GetFinalPathNameByHandleW`, so an 8.3 alias, another case, a junction, a `subst` drive or a
//! `\\?\` prefix all come out as the one long path; on macOS and Linux `realpath`, so `/var` is
//! `/private/var` and a name has its case on the disk). A name that does not exist yet (a new
//! file a `Write` makes) is kept as written after its deepest existing folder, which is resolved.
//!
//! Nothing on the network is touched: a path that is or leads to one, or that the walk cannot
//! vouch for (a link it can't read, a loop, a folder it may not look in), is [`Spot::Unvouched`],
//! which the fence denies, unless it lies by its text in the matter folder the lawyer keeps on
//! a share ([`Spot::ShareMatter`], untouched and left to the other checks).

use std::path::{Path, PathBuf};

use crate::network_paths::resolve::{Disk, Walked, walk};
use crate::network_paths::{NetEnv, refused_windows_name, under_share_matter};

/// Where one reading of a path lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Spot {
    /// A local path, resolved.
    Local(PathBuf),
    /// By its text in a matter folder kept on a network share that the session may use (its
    /// own, or one it is invited into): never touched, judged by its text.
    Share(PathBuf),
    /// A network location outside every such matter, as the agent named it: never touched.
    Network(PathBuf),
    /// A place the walk can't vouch for (a link it can't read, a loop, a name Windows reads as
    /// another place), or a network path that may be a share matter spelled another way (a
    /// `.` or `..` name, a name Windows trims, a drive letter where the matter is stored by
    /// its server name, or the reverse): never touched, denied.
    Unvouched,
}

/// What a path is read against: the folder a relative path is taken from, the lawyer's home
/// for `~`, the matter folders the session may use (for those kept on a share), and the
/// network rules.
pub(crate) struct Ground<'a> {
    /// The tool input's working folder, else the matter folder.
    pub(crate) cwd: &'a Path,
    /// The lawyer's home, when known.
    pub(crate) home: Option<&'a Path>,
    /// The session's matter folder (`--matter`), then the folders of the matters it is
    /// invited into (the fence's roots it may read).
    pub(crate) shares: &'a [PathBuf],
    /// How the disk is looked at and what counts as the network.
    pub(crate) disk: &'a Disk<'a>,
}

impl Ground<'_> {
    fn net(&self) -> &NetEnv {
        self.disk.net
    }
}

/// Every place `text` may name, as absolute paths: the text taken from `cwd` (as it is when
/// absolute); with a leading `~` name, also under `home`; with Windows' rules, a leading
/// `/c/…` or `//c/…` (Git Bash's and Claude Code's own rule form) also as `C:\…`, and only so
/// for `//c/…`, which Windows would otherwise read as a server named `c`.
pub(crate) fn readings(text: &str, ground: &Ground<'_>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let windows = ground.net().windows;
    let drive = windows.then(|| posix_drive(text)).flatten();
    if let Some((drive, doubled)) = drive {
        out.push(PathBuf::from(drive));
        if doubled {
            return out;
        }
    }
    let separator = |c: char| c == '/' || (windows && c == '\\');
    if let Some(rest) = text.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with(separator))
        && let Some(home) = ground.home
    {
        out.push(home.join(rest.trim_start_matches(separator)));
    }
    // With Windows' rules a UNC or drive path is absolute whatever OS reads the text, so the
    // working folder never comes before it.
    let mut chars = text.chars();
    let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next() == Some(':')
        && chars.next().is_some_and(separator);
    let unc = text.chars().take(2).filter(|c| separator(*c)).count() == 2;
    if windows && (drive || unc) {
        out.push(PathBuf::from(text));
    } else {
        out.push(ground.cwd.join(text));
    }
    out
}

/// `/c/rest` or `//c/rest` (either separator) as `C:\rest`, and whether the separator was
/// doubled.
fn posix_drive(text: &str) -> Option<(String, bool)> {
    let separator = |c: char| c == '/' || c == '\\';
    let trimmed = text.trim_start_matches(separator);
    let leading = text.len().saturating_sub(trimmed.len());
    if !(1..=2).contains(&leading) {
        return None;
    }
    let mut chars = trimmed.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str();
    if !(rest.is_empty() || rest.starts_with(separator)) {
        return None;
    }
    let rest = rest.trim_start_matches(separator).replace('/', "\\");
    Some((
        format!("{}:\\{rest}", letter.to_ascii_uppercase()),
        leading == 2,
    ))
}

/// Where the absolute `path` lands ([`Spot`]). What is network is read from the text and
/// from this system's lists alone (`NetEnv::is_network`), never by opening the path.
pub(crate) fn locate(path: &Path, ground: &Ground<'_>) -> Spot {
    let net = ground.net();
    if ground
        .shares
        .iter()
        .any(|share| under_share_matter(path, share, net))
    {
        return Spot::Share(path.to_path_buf());
    }
    if refused_windows_name(path, net) {
        return Spot::Unvouched;
    }
    if net.is_network(path) {
        return if another_spelling(path, ground.shares, net) {
            Spot::Unvouched
        } else {
            Spot::Network(path.to_path_buf())
        };
    }
    match walk(path, ground.disk) {
        Walked::Local(real) => Spot::Local(real),
        Walked::Refused => Spot::Unvouched,
    }
}

/// Every [`Spot`] the agent's `text` may land on ([`readings`], each [`locate`]d).
pub(crate) fn spots(text: &str, ground: &Ground<'_>) -> Vec<Spot> {
    readings(text, ground)
        .iter()
        .map(|path| locate(path, ground))
        .collect()
}

/// Whether the network path `path`, in none of `shares`, may still be one of them spelled
/// another way: it has a `.` or `..` name, or with Windows' rules a name the system trims (a
/// trailing dot or space), or names a drive letter where a share matter is stored by its server
/// name (or the reverse). Only the server could say; the fence asks none.
fn another_spelling(path: &Path, shares: &[PathBuf], net: &NetEnv) -> bool {
    let text = path.to_string_lossy();
    let separator = |c: char| c == '/' || (net.windows && c == '\\');
    let trimmed = |name: &str| net.windows && (name.ends_with('.') || name.ends_with(' '));
    let odd = text
        .split(separator)
        .any(|name| name == "." || name == ".." || (!name.is_empty() && trimmed(name)));
    if odd {
        return true;
    }
    let lettered = |text: &str| net.windows && text.as_bytes().get(1) == Some(&b':');
    shares
        .iter()
        .filter(|share| net.is_network(share))
        .any(|share| lettered(&share.to_string_lossy()) != lettered(&text))
}
