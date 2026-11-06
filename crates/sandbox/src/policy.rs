//! The policy: what one launch's jail lets the agent read, write, run and reach. Fields are
//! private; [`Policy::builder`] makes one, and every path in it is canonical.

use std::path::{Path, PathBuf};

use crate::links::{LinkFinding, MatterScan};
use crate::{Access, Error, PathClass};

mod build;
mod links;
mod names;

pub use links::LINK_RULES_MAX;

/// Names write-denied at the matter root whether or not they exist, so the agent cannot plant
/// config or instructions that load next session (shared ruling 12, private issue #184).
/// `.clauseo/matter.json` is written by the host. `CLAUDE.local.md` is Claude's untracked
/// instruction file, loaded like `CLAUDE.md`.
pub const PROTECTED_NAMES: [&str; 8] = [
    ".codex",
    ".claude",
    ".agents",
    ".clauseo",
    ".mcp.json",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
];

/// The names write-denied at any depth of the matter, in any letter case, wherever one exists
/// at launch (S15 `PR4a2`): git finds a `.git` at any depth, Claude loads `.claude`,
/// `.mcp.json` and `CLAUDE.md` from the folder it works in, and Codex `AGENTS.md`. The walk
/// ([`scan_matter`](crate::scan_matter)) finds them by name in the listings it reads, and
/// [`Policy::read_only_in_matter`] lists each. On Windows the set-aside probes the same names in
/// every folder below the root, for the ones made or replaced while a session runs.
pub const PROTECTED_AT_ANY_DEPTH: [&str; 5] =
    [".git", ".claude", ".mcp.json", "CLAUDE.md", "AGENTS.md"];

/// The instruction file write-denied at any depth of the matter, in any letter case: Claude
/// loads a nested `CLAUDE.md` when it works in that folder. Stricter than the brief's "at the
/// matter root" (decision 0038, for Rohan to confirm).
pub const INSTRUCTIONS_AT_ANY_DEPTH: &str = "CLAUDE.md";

/// Whether `path` is `prefix` or under it. On macOS names compare one by one by their
/// [`names::name_key`], Unicode canonical caseless matching, the folding APFS documents for its
/// case-insensitive volumes (`DATA/x` is under `data`, an NFD name matches its NFC form; the
/// pairs checked against this Mac's volume are in the tests). Elsewhere the comparison is exact:
/// NTFS and Linux file systems keep NFC and NFD names as two files.
pub(crate) fn under(path: &Path, prefix: &Path) -> bool {
    if path.starts_with(prefix) {
        true
    } else if cfg!(target_os = "macos") {
        let mut names = path.components();
        prefix.components().all(|want| {
            names
                .next()
                .is_some_and(|got| names::same_name(got.as_os_str(), want.as_os_str()))
        })
    } else {
        false
    }
}

/// Whether `a` and `b` are the same path, by [`under`]'s comparison.
pub(crate) fn same(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && under(a, b)
}

/// Whether `path` is a [`INSTRUCTIONS_AT_ANY_DEPTH`] file in `matter`, in any letter case.
pub(crate) fn instruction_file(matter: &Path, path: &Path) -> bool {
    under(path, matter)
        && path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(INSTRUCTIONS_AT_ANY_DEPTH))
}

/// The files of Claude's login store the jail lets the agent read, write and create, in the
/// folder given to [`PolicyBuilder::claude_store`]: the store itself (the Keychain's fallback
/// on macOS) and the secure-storage write lock's name.
pub const CLAUDE_STORE_FILES: [&str; 2] = [".credentials.json", ".storage-write"];

/// The lock folders Claude makes in that folder (proper-lockfile makes a lock as a folder):
/// the write lock's (`.storage-write` plus `.lock`) and the OAuth refresh lock. Unverified:
/// read from Claude Code 2.1.316's bundled code (`lockfilePath: <dir>/.oauth_refresh.lock`),
/// not seen live; G7 row 17 records the names. See also [`CLAUDE_STORE_BESIDE_LOCK_SUFFIX`].
pub const CLAUDE_STORE_LOCK_FOLDERS: [&str; 2] = [".storage-write.lock", ".oauth_refresh.lock"];

/// The suffix of Claude's legacy OAuth refresh lock, a folder that sits beside the store
/// folder, not in it: `<dir>.lock` (for `~/.claude`, `~/.claude.lock`). Without it a token
/// refresh can fail mid-session. Unverified like the others: read from Claude Code 2.1.316's
/// bundled code (`${realpath(dir)}.lock`), not seen live; G7 row 17 records it.
pub const CLAUDE_STORE_BESIDE_LOCK_SUFFIX: &str = ".lock";

/// The browser's loopback reach (agent-browser's and Chrome's CDP ports).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Loopback {
    /// No loopback beyond the proxy ports.
    #[default]
    None,
    /// These loopback ports, with bind and inbound on localhost. Sorted and de-duplicated.
    Ports(Vec<u16>),
    /// Any loopback port. Also reaches other loopback services and other launches' proxies,
    /// which refuse a request without their token (S16).
    Any,
}

/// A path rule: the path alone, or the path and everything under it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathRule {
    /// Exactly this path.
    Literal(PathBuf),
    /// This path and everything under it.
    Subpath(PathBuf),
}

impl PathRule {
    /// The rule's path.
    pub fn path(&self) -> &Path {
        match self {
            PathRule::Literal(path) | PathRule::Subpath(path) => path,
        }
    }

    /// Whether the rule covers `path`, by [`under`]: on macOS without case and in any Unicode
    /// normalisation form, as the kernel resolves names on a case-insensitive volume.
    pub fn covers(&self, path: &Path) -> bool {
        match self {
            PathRule::Literal(rule) => same(path, rule),
            PathRule::Subpath(rule) => under(path, rule),
        }
    }
}

/// The folder holding Claude's login store, and the names the jail allows in it and nothing
/// else (the agent cannot list the folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeStore {
    dir: PathBuf,
}

impl ClaudeStore {
    /// The folder, canonical.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// [`CLAUDE_STORE_FILES`] in the folder: read-write literal rules.
    pub fn files(&self) -> Vec<PathBuf> {
        CLAUDE_STORE_FILES
            .iter()
            .map(|name| self.dir.join(name))
            .collect()
    }

    /// The lock folders, read-write subpath rules, each creatable: [`CLAUDE_STORE_LOCK_FOLDERS`]
    /// in the folder, then the legacy lock beside it (`<dir>.lock`,
    /// [`CLAUDE_STORE_BESIDE_LOCK_SUFFIX`]), which sits next to the store folder, not in it.
    pub fn lock_folders(&self) -> Vec<PathBuf> {
        let mut locks: Vec<PathBuf> = CLAUDE_STORE_LOCK_FOLDERS
            .iter()
            .map(|name| self.dir.join(name))
            .collect();
        let mut beside = self.dir.clone().into_os_string();
        beside.push(CLAUDE_STORE_BESIDE_LOCK_SUFFIX);
        locks.push(PathBuf::from(beside));
        locks
    }

    /// Whether `path` is one of the allowed names or under a lock folder.
    fn covers(&self, path: &Path) -> bool {
        self.files().iter().any(|file| same(path, file))
            || self.lock_folders().iter().any(|lock| under(path, lock))
    }
}

/// What one launch's jail allows. Built by [`Policy::builder`]; every path is canonical
/// (`/var` is `/private/var` on macOS), and every rule of [`PolicyBuilder::build`] holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    matter: PathBuf,
    state: PathBuf,
    home: Option<PathBuf>,
    runtime: Vec<PathBuf>,
    read: Vec<PathBuf>,
    allowed_folders: Vec<PathBuf>,
    read_only_in_state: Vec<PathBuf>,
    notes: Option<PathBuf>,
    credential_link: Option<PathBuf>,
    claude_store: Option<ClaudeStore>,
    hidden: Vec<PathBuf>,
    proxy_ports: Vec<u16>,
    loopback: Loopback,
    browser_socket_dir: Option<PathBuf>,
    keychain: bool,
    over_cap: Vec<PathBuf>,
    links: Vec<LinkFinding>,
    link_targets: Vec<PathBuf>,
    link_folders: Vec<PathBuf>,
    read_only_in_matter: Vec<PathRule>,
}

/// The policy's inputs, as given. [`PolicyBuilder::build`] checks and canonicalises them.
#[derive(Debug, Clone, Default)]
pub struct PolicyBuilder {
    matter: PathBuf,
    state: PathBuf,
    home: Option<PathBuf>,
    runtime: Vec<PathBuf>,
    read: Vec<PathBuf>,
    allowed_folders: Vec<PathBuf>,
    read_only_in_state: Vec<PathBuf>,
    notes: Option<PathBuf>,
    credential_link: Option<PathBuf>,
    claude_store: Option<PathBuf>,
    hidden: Vec<PathBuf>,
    proxy_ports: Vec<u16>,
    loopback: Loopback,
    browser_socket_dir: Option<PathBuf>,
    keychain: bool,
    over_cap: Vec<PathBuf>,
    links: Vec<LinkFinding>,
    scan: Option<MatterScan>,
}

impl Policy {
    /// A builder for the jail of `matter` with the agent's `state` folder, both read-write.
    /// `TMPDIR` is `<state>/tmp` ([`Policy::tmpdir`]).
    pub fn builder(matter: impl Into<PathBuf>, state: impl Into<PathBuf>) -> PolicyBuilder {
        PolicyBuilder {
            matter: matter.into(),
            state: state.into(),
            home: std::env::home_dir(),
            ..PolicyBuilder::default()
        }
    }

    /// The matter folder, read-write.
    pub fn matter(&self) -> &Path {
        &self.matter
    }

    /// The agent's state folder, read-write.
    pub fn state(&self) -> &Path {
        &self.state
    }

    /// `<state>/tmp`, the agent's `TMPDIR`.
    pub fn tmpdir(&self) -> PathBuf {
        self.state.join("tmp")
    }

    /// The home folder the refusals compare with ([`PolicyBuilder::home`]); never allowed.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Paths to read and run: Node, the adapter, the agent binary, the sidecar folder, Chrome.
    pub fn runtime(&self) -> &[PathBuf] {
        &self.runtime
    }

    /// Paths to read only: CA files, `agent-browser.json`, skills folders.
    pub fn read(&self) -> &[PathBuf] {
        &self.read
    }

    /// Folders the lawyer allowed, read-only, each passing [`Policy::check_allowed_folder`].
    pub fn allowed_folders(&self) -> &[PathBuf] {
        &self.allowed_folders
    }

    /// Paths in the state folder the agent may read but not write (config the host rewrites
    /// before each launch).
    pub fn read_only_in_state(&self) -> &[PathBuf] {
        &self.read_only_in_state
    }

    /// The matter's notes folder, read-write.
    pub fn notes(&self) -> Option<&Path> {
        self.notes.as_deref()
    }

    /// The credential link inside the state folder, unresolved: only this path enters the
    /// profile, never the file it links to.
    pub fn credential_link(&self) -> Option<&Path> {
        self.credential_link.as_deref()
    }

    /// Claude's login store, if Claude runs in this jail.
    pub fn claude_store(&self) -> Option<&ClaudeStore> {
        self.claude_store.as_ref()
    }

    /// Paths denied after the allows: the data dir, other matters and state folders, refused
    /// folders, stetdoc's config. The state folder, notes, credential link, Claude store,
    /// runtime and read paths are allowed again after them; allowed folders are not.
    pub fn hidden(&self) -> &[PathBuf] {
        &self.hidden
    }

    /// The loopback ports the agent may connect out to (the proxy's listeners, the connector
    /// relay), sorted and de-duplicated. Empty means no network at all.
    pub fn proxy_ports(&self) -> &[u16] {
        &self.proxy_ports
    }

    /// The browser's loopback reach.
    pub fn loopback(&self) -> &Loopback {
        &self.loopback
    }

    /// The browser socket folder, canonical (`BrowserSocketDir::canonical`, Unix): the one
    /// place the jail lets the agent make, bind and reach Unix sockets, and make, write, read
    /// and remove files. `None`: no Unix socket anywhere.
    pub fn browser_socket_dir(&self) -> Option<&Path> {
        self.browser_socket_dir.as_deref()
    }

    /// Whether Claude's Keychain rules apply (macOS).
    pub fn keychain(&self) -> bool {
        self.keychain
    }

    /// Files in the matter over the checkpoint size cap, read-only (literal rules).
    pub fn over_cap(&self) -> &[PathBuf] {
        &self.over_cap
    }

    /// Links in the matter that lead outside it, canonical; read and write are denied on each
    /// path (or the link folder that holds it) and on the targets of [`Policy::link_targets`].
    pub fn links(&self) -> &[LinkFinding] {
        &self.links
    }

    /// The link targets the profile denies read and write on (subpath), distinct: those some
    /// allow rule of the policy covers (runtime, read, allowed folders, state, notes, the Claude
    /// store, the credential link). A target nothing allows is refused by the default deny
    /// already, and one that holds an allowed path gets no rule (it would deny that path).
    /// Only the policy's own allows count: a target under the profile's base system reads,
    /// the text-encoding preference or the Keychain folders gets no rule, since the assistant
    /// may read those files by their own path anyway; the link itself stays denied.
    pub fn link_targets(&self) -> &[PathBuf] {
        &self.link_targets
    }

    /// Folders in the matter denied read and write in place of the findings under them, so the
    /// profile holds at most [`LINK_RULES_MAX`] link rules; empty unless the findings would
    /// pass it. See [`PolicyBuilder::build`].
    pub fn link_folders(&self) -> &[PathBuf] {
        &self.link_folders
    }

    /// The findings no link folder holds: the ones that get a rule of their own.
    pub fn links_outside_link_folders(&self) -> impl Iterator<Item = &LinkFinding> {
        self.links.iter().filter(|finding| {
            !self
                .link_folders
                .iter()
                .any(|folder| under(&finding.path, folder))
        })
    }

    /// Everything write-denied in the matter, sorted: each of [`PROTECTED_NAMES`] at the root
    /// (a subpath rule, so a protected folder that already exists is read-only inside too),
    /// each `.git` found at any depth (subpath), each folder a `.git` file names inside the
    /// matter (subpath), each other [`PROTECTED_AT_ANY_DEPTH`] name found at any depth, in any
    /// letter case, unless a folder listed here holds it (subpath), and each over-cap file
    /// (literal). A [`INSTRUCTIONS_AT_ANY_DEPTH`] file at any depth is write-denied on macOS
    /// by one pattern rule in the profile too, so one made after the walk is covered there.
    pub fn read_only_in_matter(&self) -> &[PathRule] {
        &self.read_only_in_matter
    }

    /// Why `path` (canonical, as the policy's paths are) was refused `access`, for the denial
    /// watcher: [`PathClass::LinkTarget`] on or under a link finding's path or target, or under
    /// a link folder;
    /// [`PathClass::Hidden`] under a hidden path that nothing allows again;
    /// [`PathClass::ReadOnly`] for a write to a read-only path (protected names, `.git`,
    /// over-cap files, nested `CLAUDE.md`, read-only paths in the state folder, allowed
    /// folders, runtime, read); otherwise [`PathClass::Outside`]. On macOS names compare
    /// without case and Unicode normalisation ([`under`]), as the kernel's checks do on a
    /// case-insensitive volume, so `.GIT/config` is read-only and `DATA/x` hidden.
    pub fn classify(&self, path: &Path, access: Access) -> PathClass {
        let on_link = self.links.iter().any(|finding| {
            under(path, &finding.path)
                || finding
                    .target
                    .as_ref()
                    .is_some_and(|target| under(path, target))
        }) || self.link_folders.iter().any(|folder| under(path, folder));
        if on_link {
            return PathClass::LinkTarget;
        }
        if !self.allowed_again(path) && self.hidden.iter().any(|h| under(path, h)) {
            return PathClass::Hidden;
        }
        if access == Access::Write && self.read_only(path) {
            return PathClass::ReadOnly;
        }
        PathClass::Outside
    }

    /// Refuses `folder` as an allowed folder unless it passes every rule, after canonicalising
    /// it (it must exist): it may not equal, hold or sit inside a hidden path; equal, hold or
    /// sit inside the matter; equal or hold the home folder; be `/`; or have a `.git`
    /// component. "Holds the matter" is the rule that refuses `Documents/Clauseo`, the parent of
    /// new matters, which cannot be a hidden path because the matter sits inside it. R18 runs
    /// this before it shows a card; [`PolicyBuilder::build`] runs it on each allowed folder.
    pub fn check_allowed_folder(&self, folder: &Path) -> Result<(), Error> {
        let folder = build::existing(folder, "allowed folder")?;
        build::check_allowed(&folder, &self.matter, self.home.as_deref(), &self.hidden)
    }

    /// Whether a rule after the hidden paths allows `path` again.
    fn allowed_again(&self, path: &Path) -> bool {
        let inside = |p: &PathBuf| under(path, p);
        under(path, &self.state)
            || self.notes.as_ref().is_some_and(inside)
            || self
                .credential_link
                .as_deref()
                .is_some_and(|link| same(path, link))
            || self
                .claude_store
                .as_ref()
                .is_some_and(|store| store.covers(path))
            || self.runtime.iter().any(inside)
            || self.read.iter().any(inside)
    }

    /// Whether `path` may be read but not written.
    fn read_only(&self, path: &Path) -> bool {
        let inside = |p: &PathBuf| under(path, p);
        self.read_only_in_matter
            .iter()
            .any(|rule| rule.covers(path))
            || instruction_file(&self.matter, path)
            || self.read_only_in_state.iter().any(inside)
            || self.allowed_folders.iter().any(inside)
            || self.runtime.iter().any(inside)
            || self.read.iter().any(inside)
    }
}

impl PolicyBuilder {
    /// A path to read and run.
    pub fn runtime(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.runtime.push(p.into());
        self
    }

    /// A path to read only.
    pub fn read(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.read.push(p.into());
        self
    }

    /// A folder the lawyer allowed, read-only; `build` runs
    /// [`Policy::check_allowed_folder`] on it.
    pub fn allowed_folder(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.allowed_folders.push(p.into());
        self
    }

    /// A path inside the state folder that the agent may read but not write.
    pub fn read_only_in_state(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.read_only_in_state.push(p.into());
        self
    }

    /// The matter's notes folder, `<data_dir>/agent-state/<matter_id>/notes/`, read-write.
    pub fn notes(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.notes = Some(p.into());
        self
    }

    /// The credential link inside the state folder; only this path enters the profile.
    pub fn credential_link(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.credential_link = Some(p.into());
        self
    }

    /// The folder holding Claude's login store (the lawyer's `~/.claude`, or Clauseo's login
    /// home). Only [`CLAUDE_STORE_FILES`] and [`CLAUDE_STORE_LOCK_FOLDERS`] in it are allowed.
    pub fn claude_store(mut self, dir: impl Into<PathBuf>) -> PolicyBuilder {
        self.claude_store = Some(dir.into());
        self
    }

    /// A path to deny after the allows. It need not exist.
    pub fn hidden(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.hidden.push(p.into());
        self
    }

    /// A loopback port the agent may connect out to. Call once per listener (the proxy's
    /// agent and browser listeners, the connector relay); none means no network at all.
    pub fn proxy_port(mut self, port: u16) -> PolicyBuilder {
        self.proxy_ports.push(port);
        self
    }

    /// The browser's loopback reach; [`Loopback::None`] unless set.
    pub fn loopback(mut self, loopback: Loopback) -> PolicyBuilder {
        self.loopback = loopback;
        self
    }

    /// The browser socket folder the host made for this launch. The policy keeps its canonical
    /// path; the jail then allows Unix-socket bind and connect, and file create, write, read
    /// and unlink, under that one folder, but not making or removing the folder itself.
    /// Without it the jail allows no Unix socket anywhere.
    #[cfg(unix)]
    pub fn browser_socket_dir(mut self, dir: &crate::BrowserSocketDir) -> PolicyBuilder {
        self.browser_socket_dir = Some(dir.canonical().to_owned());
        self
    }

    /// [`PolicyBuilder::browser_socket_dir`] by path, for the profile's snapshots on every
    /// target.
    #[cfg(test)]
    pub(crate) fn browser_socket_path(mut self, path: PathBuf) -> PolicyBuilder {
        self.browser_socket_dir = Some(path);
        self
    }

    /// Whether Claude's Keychain rules apply (macOS); off unless set.
    pub fn keychain(mut self, on: bool) -> PolicyBuilder {
        self.keychain = on;
        self
    }

    /// Files in the matter the last checkpoint walk skipped for size, made read-only.
    pub fn over_cap(mut self, files: impl IntoIterator<Item = PathBuf>) -> PolicyBuilder {
        self.over_cap.extend(files);
        self
    }

    /// Link findings to deny, added to the scan's.
    pub fn links(mut self, findings: impl IntoIterator<Item = LinkFinding>) -> PolicyBuilder {
        self.links.extend(findings);
        self
    }

    /// The matter's walk, if the caller already ran [`scan_matter`](crate::scan_matter).
    /// Without it, `build` walks the matter itself, so there is one walk per launch either
    /// way.
    pub fn scan(mut self, scan: MatterScan) -> PolicyBuilder {
        self.scan = Some(scan);
        self
    }

    /// The home folder the refusals compare with; `std::env::home_dir()` unless set. Tests
    /// set it so the "home folder" refusal runs without the real one.
    pub fn home(mut self, p: impl Into<PathBuf>) -> PolicyBuilder {
        self.home = Some(p.into());
        self
    }

    /// Checks and canonicalises the inputs. Each refusal is an [`Error::Policy`] naming the
    /// rule and the path.
    ///
    /// - Every path must be absolute. The matter, state folder, runtime and read paths,
    ///   allowed folders, notes folder and Claude store folder must exist and are
    ///   canonicalised. The credential link, read-only paths in the state folder and over-cap
    ///   files keep their own name: the folder they are in is canonicalised and the name
    ///   appended, so a link's target never enters the policy. Hidden paths need not exist:
    ///   the longest existing ancestor is canonicalised and the rest appended.
    /// - The matter, state folder and notes folder may not themselves be symbolic links (an
    ///   agent could have put one in place of its own folder last session).
    /// - The matter may not be `/` or equal or hold the home folder, nor equal, hold or sit
    ///   inside a hidden path. The state folder and the matter may not be one inside the other
    ///   (or the same); the state folder may not equal or hold a hidden path. The notes folder
    ///   may not be inside the matter, hold it, or equal or hold a hidden path. The credential
    ///   link must be inside the state folder. Runtime and read paths may not be `/` or equal
    ///   or hold the home folder, the matter or a hidden path (inside one is fine: that is why
    ///   they are allowed again). Read-only paths in the state folder must be inside it;
    ///   over-cap files inside the matter.
    /// - Each allowed folder passes [`Policy::check_allowed_folder`].
    /// - Without [`PolicyBuilder::scan`], the matter is walked here
    ///   ([`scan_matter`](crate::scan_matter)). Every finding (the walk's and
    ///   [`PolicyBuilder::links`]') is canonicalised, and one outside the matter is refused. A
    ///   file whose names are all in the matter is not a finding unless it joins a write-denied
    ///   name to a writable one; then the writable names are.
    /// - At most [`LINK_RULES_MAX`] link rules (finding paths and distinct targets): past it,
    ///   findings give way to [`Policy::link_folders`], nearest folder first, never the matter
    ///   root or a folder holding a `.git`; if that cannot fit, the build is refused with the
    ///   count. Seatbelt fails to compile a profile with a few thousand such rules.
    /// - Proxy ports and [`Loopback::Ports`] are sorted and de-duplicated.
    pub fn build(self) -> Result<Policy, Error> {
        build::build(self)
    }
}

#[cfg(test)]
mod tests;
