//! The fence's decision for one tool call: the paths the call names (by tool), where each lands
//! ([`super::real`]), and whether that is in a fenced place ([`Fence`]).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::pattern::{Pattern, folded, glob_base};
use super::real::{Ground, Spot, readings, spots};
use super::{FenceFile, FileId, RootKind};
use crate::network_paths::fold_name;
use crate::network_paths::resolve::comparable;

/// The tools the fence checks; the hook entry runs before each (`agent::loads::GUARD_MATCHER`).
pub(crate) const FENCED_TOOLS: [&str; 7] = [
    "Read",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "Grep",
    "Glob",
];

/// Whether a network path outside every matter the session may use is refused (decided
/// 2026-11-16, to be confirmed: to-do 20). `false` lets it go on, judged by its text against
/// the fenced places alone; a path the fence can't vouch for is refused either way.
pub(crate) const REFUSE_NETWORK_OUTSIDE_MATTERS: bool = true;

/// Why the fence refuses a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Denial {
    /// The path lands in a fenced place of this kind.
    Fenced(RootKind),
    /// The search folder is in, or holds, a fenced place of this kind.
    Holds(RootKind),
    /// A network location outside every matter the session may use
    /// ([`REFUSE_NETWORK_OUTSIDE_MATTERS`]).
    Network,
    /// A `Glob` pattern that may reach past where it starts.
    Unbounded,
    /// A path the fence can't vouch for: a link it can't follow, a name Windows reads as another
    /// place, a share matter's folder spelled another way.
    Unvouched,
    /// The fence could not decide: input or a fence file it can't read or trust, or a failure.
    Broken,
    /// The fence did not decide within its deadline.
    Late,
}

/// The way out a refusal offers, which the agent passes on to the lawyer.
const WAY_OUT: &str = "The lawyer can add that folder as a matter and invite this conversation \
                       into it, or attach or copy the file into this matter.";

impl Denial {
    /// What the agent is told: each holds `has been denied` and `your permission settings`,
    /// the words the host's refusal detector reads. No path.
    pub(crate) fn reason(self) -> String {
        let denied = "Permission to use this path has been denied by your permission settings:";
        match self {
            Self::Fenced(RootKind::Data) => format!(
                "{denied} it is in Clauseo's own data, which the agent may not read or change."
            ),
            Self::Fenced(RootKind::Matter) => format!(
                "{denied} it is in another matter's folder, which this conversation may not use \
                 this way. Tell the lawyer it belongs to another matter. The lawyer can invite \
                 this conversation into that matter, or attach or copy the file into this one."
            ),
            Self::Fenced(RootKind::Protected) => format!(
                "{denied} it is in a folder that holds sign-ins or settings the agent may not use."
            ),
            Self::Holds(_) => "Permission to search this folder has been denied by your \
                               permission settings: it holds or is in another matter's folder, \
                               Clauseo's own data or a protected folder. Search a folder inside \
                               this matter's folder instead."
                .to_owned(),
            Self::Network => format!(
                "{denied} it is on a network drive or share outside this conversation's \
                 matters. {WAY_OUT}"
            ),
            Self::Unbounded => "Permission to search with this pattern has been denied by your \
                                permission settings: it may reach folders outside where it \
                                starts (a `..`, a brace group holding a separator, or another \
                                drive). Search inside this matter's folder with a pattern that \
                                stays below its start."
                .to_owned(),
            Self::Unvouched => format!(
                "{denied} Clauseo could not check where it leads (a link it can't follow, a \
                 name Windows reads as another place, or a network folder written another way \
                 than the matter's). Write the path the way the matter's folder is written. \
                 {WAY_OUT}"
            ),
            Self::Broken => "Permission to use this tool has been denied by your permission \
                             settings: Clauseo could not check the path it names. Try again; if \
                             it keeps happening, tell the lawyer."
                .to_owned(),
            Self::Late => "Permission to use this tool has been denied by your permission \
                           settings: Clauseo's check of the path it names took too long. Try \
                           again; if it keeps happening, tell the lawyer."
                .to_owned(),
        }
    }
}

/// How a folder's identity is read: `None` for no such name.
pub(crate) type Identity = Box<dyn Fn(&Path) -> Option<FileId>>;

/// One root of the fence file, ready to compare.
struct Root {
    kind: RootKind,
    /// Each spelling's names, folded when the match ignores case.
    keys: Vec<Vec<String>>,
    id: Option<FileId>,
    line: Vec<FileId>,
    read: Vec<Pattern>,
    edit: Vec<Pattern>,
}

/// The fence file, read for matching.
pub(crate) struct Fence {
    roots: Vec<Root>,
    insensitive: bool,
    /// Names split at `\` too, a drive or server read as text (Windows' rules).
    windows: bool,
    /// The identity of a path's folder, `None` for no such name (or one that can't be read: the
    /// walk looked at each name already, so the agent's tool can't open it either).
    identity: Identity,
}

impl Fence {
    /// `file` read for matching, comparing names ignoring case when `insensitive`, identities
    /// read by `identity`.
    pub(crate) fn new(file: &FenceFile, insensitive: bool, identity: Identity) -> Self {
        Self::with_rules(file, (insensitive, cfg!(windows)), identity)
    }

    /// [`Self::new`], names read with Windows' rules when `windows`.
    pub(crate) fn with_rules(
        file: &FenceFile,
        (insensitive, windows): (bool, bool),
        identity: Identity,
    ) -> Self {
        let roots = file
            .roots
            .iter()
            .map(|root| Root {
                kind: root.kind,
                keys: root
                    .spellings
                    .iter()
                    .map(|spelling| key(Path::new(spelling), insensitive, windows))
                    .collect(),
                id: root.id.clone(),
                line: root.line.clone(),
                read: patterns(&root.read, insensitive),
                edit: patterns(&root.edit, insensitive),
            })
            .collect();
        Self {
            roots,
            insensitive,
            windows,
            identity,
        }
    }

    /// The fence on the real disk ([`crate::network_paths::resolve::RealFs`]).
    pub(crate) fn on_disk(file: &FenceFile, insensitive: bool) -> Self {
        use crate::network_paths::resolve::{Fs as _, RealFs};
        // Missing, or unreadable: the walk looked at each name already.
        let identity = |path: &Path| RealFs.identity(path).ok().map(FileId::of);
        Self::new(file, insensitive, Box::new(identity))
    }

    /// The names of `real` under each root it is in, by text (a spelling's names start its
    /// own) or, when `identity`, by identity (a folder above it, or it, is the root).
    fn under(&self, real: &Path, identity: bool) -> Vec<(&Root, Vec<String>)> {
        let names = key(real, self.insensitive, self.windows);
        let mut out = Vec::new();
        for root in &self.roots {
            for spelling in &root.keys {
                if let Some(rest) = names.strip_prefix(spelling.as_slice()) {
                    out.push((root, rest.to_vec()));
                }
            }
        }
        let ids: HashSet<&FileId> = self
            .roots
            .iter()
            .filter_map(|root| root.id.as_ref())
            .collect();
        if ids.is_empty() || !identity {
            return out;
        }
        for folder in real.ancestors() {
            let Some(id) = (self.identity)(folder) else {
                continue;
            };
            if !ids.contains(&id) {
                continue;
            }
            let rest = real.strip_prefix(folder).unwrap_or(Path::new(""));
            let parts: Vec<OsString> = rest.iter().map(ToOwned::to_owned).collect();
            let rest = folded(&parts, self.insensitive);
            for root in self
                .roots
                .iter()
                .filter(|root| root.id.as_ref() == Some(&id))
            {
                out.push((root, rest.clone()));
            }
        }
        out
    }

    /// The kind of fenced place the local path `real` lands in, if any: for a tool that writes
    /// (`write`), the read and edit patterns count; else the read patterns.
    pub(crate) fn landed(&self, real: &Path, write: bool) -> Option<RootKind> {
        self.landed_as(real, write, true)
    }

    /// [`Self::landed`] for a path judged by its text alone (a network path, never touched).
    pub(crate) fn landed_text(&self, path: &Path, write: bool) -> Option<RootKind> {
        self.landed_as(path, write, false)
    }

    fn landed_as(&self, real: &Path, write: bool, identity: bool) -> Option<RootKind> {
        self.under(real, identity)
            .into_iter()
            .find_map(|(root, names)| {
                let edit = root.edit.iter().filter(|_| write);
                root.read
                    .iter()
                    .chain(edit)
                    .any(|pattern| pattern.covers(&names))
                    .then_some(root.kind)
            })
    }

    /// The kind of fenced place a search from the local folder `real` may reach, if any: the
    /// folder is in one (a read pattern may cover something at or under it), or holds a root
    /// (by text, or by identity: it is the root or a folder above it).
    pub(crate) fn reached(&self, real: &Path) -> Option<RootKind> {
        self.reached_as(real, true)
    }

    /// [`Self::reached`] for a folder judged by its text alone (a network path).
    pub(crate) fn reached_text(&self, path: &Path) -> Option<RootKind> {
        self.reached_as(path, false)
    }

    fn reached_as(&self, real: &Path, identity: bool) -> Option<RootKind> {
        let inside = self
            .under(real, identity)
            .into_iter()
            .find_map(|(root, names)| {
                root.read
                    .iter()
                    .any(|pattern| pattern.may_hold(&names))
                    .then_some(root.kind)
            });
        if inside.is_some() {
            return inside;
        }
        let names = key(real, self.insensitive, self.windows);
        let readable = self.roots.iter().filter(|root| !root.read.is_empty());
        let by_text = readable.clone().find(|root| {
            root.keys
                .iter()
                .any(|spelling| spelling.starts_with(&names))
        });
        if let Some(root) = by_text {
            return Some(root.kind);
        }
        if !identity {
            return None;
        }
        let id = (self.identity)(real)?;
        readable
            .into_iter()
            .find(|root| root.line.contains(&id))
            .map(|root| root.kind)
    }
}

/// `texts` read as patterns.
fn patterns(texts: &[String], insensitive: bool) -> Vec<Pattern> {
    texts
        .iter()
        .map(|text| Pattern::new(text, insensitive))
        .collect()
}

/// `path`'s names for a prefix comparison, folded when `insensitive` ([`fold_name`]), a long
/// path's verbatim `\\?\C:\` read as `C:\` ([`comparable`]). With Windows' rules
/// (`windows`) the text is split at `\` and `/`, a server path's leading pair kept as one name,
/// so a UNC or drive path compares the same whatever OS reads it.
fn key(path: &Path, insensitive: bool, windows: bool) -> Vec<String> {
    let path = comparable(path);
    let parts: Vec<OsString> = if windows {
        let text = path.to_string_lossy();
        let separator = |c: char| c == '/' || c == '\\';
        let server = text.chars().take(2).filter(|c| separator(*c)).count() == 2;
        let names = text.split(separator).filter(|name| !name.is_empty());
        server
            .then(|| OsString::from("\\\\"))
            .into_iter()
            .chain(names.map(OsString::from))
            .collect()
    } else {
        path.components()
            .map(|part| part.as_os_str().to_owned())
            .collect()
    };
    if insensitive {
        parts
            .iter()
            .map(|part| fold_name(&part.to_string_lossy()))
            .collect()
    } else {
        folded(&parts, false)
    }
}

/// How a tool uses the path it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Reads the one file.
    Read,
    /// Writes the one file.
    Write,
    /// Searches the folder and all below it.
    Search,
}

/// The fence's decision for the tool `tool` with `input`, read against `ground`: `None` lets
/// the call go on to the other checks. A tool the fence does not check is `None`; a checked
/// one whose input lacks its path is [`Denial::Broken`].
pub(crate) fn decide(
    fence: &Fence,
    tool: &str,
    input: &Value,
    ground: &Ground<'_>,
) -> Option<Denial> {
    decide_as(fence, tool, input, ground, REFUSE_NETWORK_OUTSIDE_MATTERS)
}

/// [`decide`], refusing a network path outside every matter when `refuse_network`.
pub(crate) fn decide_as(
    fence: &Fence,
    tool: &str,
    input: &Value,
    ground: &Ground<'_>,
    refuse_network: bool,
) -> Option<Denial> {
    let judge = Judge {
        fence,
        ground,
        refuse_network,
    };
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    match tool {
        "Read" => judge.literal(text("file_path"), Use::Read),
        "Edit" | "Write" | "MultiEdit" => judge.literal(text("file_path"), Use::Write),
        "NotebookEdit" => {
            let path = text("notebook_path").or_else(|| text("file_path"));
            judge.literal(path, Use::Write)
        }
        "Grep" => judge.search(text("path").unwrap_or(""), ""),
        "Glob" => {
            let Some(pattern) = text("pattern") else {
                return Some(Denial::Broken);
            };
            let windows = ground.disk.net.windows;
            let Some((base, _rest)) = glob_base(pattern, windows) else {
                return Some(Denial::Unbounded);
            };
            judge.search(text("path").unwrap_or(""), &base)
        }
        _ => None,
    }
}

/// One call's decision: the fence, what paths are read against, and the network rule.
struct Judge<'a> {
    fence: &'a Fence,
    ground: &'a Ground<'a>,
    refuse_network: bool,
}

impl Judge<'_> {
    /// The decision for a tool that reads or writes the one file at `path` (text).
    fn literal(&self, path: Option<&str>, using: Use) -> Option<Denial> {
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            return Some(Denial::Broken);
        };
        self.first(&spots(path, self.ground), using)
    }

    /// The decision for a search of the folder `base` (text, maybe empty) taken from `dir`
    /// (the tool's `path`, text, maybe empty: the working folder).
    fn search(&self, dir: &str, base: &str) -> Option<Denial> {
        let ground = self.ground;
        let mut found = Vec::new();
        for folder in readings(dir, ground) {
            if base.is_empty() {
                found.push(super::real::locate(&folder, ground));
                continue;
            }
            let from = Ground {
                cwd: &folder,
                home: ground.home,
                shares: ground.shares,
                disk: ground.disk,
            };
            found.extend(spots(base, &from));
        }
        self.first(&found, Use::Search)
    }

    /// The first denial any of `found` earns, used as `using` says. A local search also walks
    /// its folder for links into a fenced place ([`super::links::below`]); a path on a share is
    /// judged by its text, never touched.
    fn first(&self, found: &[Spot], using: Use) -> Option<Denial> {
        let fence = self.fence;
        let by_text = |path: &Path| match using {
            Use::Read => fence.landed_text(path, false).map(Denial::Fenced),
            Use::Write => fence.landed_text(path, true).map(Denial::Fenced),
            Use::Search => fence.reached_text(path).map(Denial::Holds),
        };
        found.iter().find_map(|spot| match spot {
            Spot::Network(_) if self.refuse_network => Some(Denial::Network),
            Spot::Share(path) | Spot::Network(path) => by_text(path),
            Spot::Unvouched => Some(Denial::Unvouched),
            Spot::Local(real) => match using {
                Use::Read => fence.landed(real, false).map(Denial::Fenced),
                Use::Write => fence.landed(real, true).map(Denial::Fenced),
                Use::Search => fence
                    .reached(real)
                    .map(Denial::Holds)
                    .or_else(|| super::links::below(fence, real, self.ground, self.refuse_network)),
            },
        })
    }
}

/// The lawyer's home, as the agent's own tools read `~`: `USERPROFILE` on Windows, `HOME`
/// elsewhere (the hook runs with Claude Code's environment); none when unset or relative.
pub(crate) fn home() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
}
