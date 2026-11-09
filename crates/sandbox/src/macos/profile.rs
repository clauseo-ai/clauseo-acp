//! The Seatbelt profile of one launch: the text `sandbox-exec -p` takes and the `-D NAME=path`
//! parameters it reads. Compiles on every target, so its snapshots run on every CI target.
//!
//! The rule order follows Codex's `codex-rs/sandboxing/src/seatbelt.rs` (commit
//! 1a604d58da344c84528fecd5b5154105f597d70b, Copyright `OpenAI`, Apache License 2.0): paths
//! reach the profile only as parameters, numbered for lists, and the ancestors of a read-only
//! path cannot be renamed. Seatbelt applies the last rule that matches, so each later block
//! overrides the ones above it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub use bridge::INSTALLED_BROWSERS;
use filters::{ancestors, ancestors_of, directory_ancestors, literal, literals, subpath, subpaths};
use names::{GIT_NAME_REGEX, HEAD_REGEX, INSTRUCTIONS_REGEX};
use params::Params;

use crate::{Error, Loopback, PathRule, Policy};

/// The base allowances (system reads, devices, sysctls, Mach services), from Codex's base.
const BASE: &str = include_str!("base.sb");

/// Every tag starts with this, so one log predicate finds every launch's denials.
pub(crate) const TAG_PREFIX: &str = "clauseo-";

/// The parameter naming the browser socket folder.
const BROWSER_SOCKETS: &str = "BROWSER_SOCKETS";

/// The parameter naming the launch's session mark.
pub(crate) const SESSION_MARK: &str = "SESSION_MARK";

/// The longest tag the profile takes.
const TAG_MAX: usize = 64;

/// One launch's Seatbelt profile: the text and the parameters it names. Every path of the
/// policy is a parameter; the text holds only rules, port numbers and the launch tag, so a path
/// holding `"`, `(` or a newline cannot change a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatbeltProfile {
    text: String,
    params: Vec<(String, PathBuf)>,
    /// The paths its allow rules with a `file-write` operation name, by parameter
    /// ([`SeatbeltProfile::writable`]).
    writable: Vec<PathBuf>,
}

impl SeatbeltProfile {
    /// The profile for `policy`, every deny rule carrying `(with message "<tag>")` so the
    /// watcher can tell this launch's denials apart. `tag` must start with `clauseo-` and hold
    /// only ASCII letters, digits and `-` (at most 64 bytes); anything else is
    /// [`Error::Policy`].
    pub fn new(policy: &Policy, tag: &str) -> Result<SeatbeltProfile, Error> {
        check_tag(tag)?;
        Ok(build(policy, tag))
    }

    /// The profile text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parameters, in the order the text first names them: each name and its path.
    pub fn params(&self) -> &[(String, PathBuf)] {
        &self.params
    }

    /// Every path an allow rule with a `file-write` operation names through a parameter, as a
    /// `literal` or a `subpath` (the session mark's own rule left out): the folders and files
    /// the agent may write, read from the rules themselves, so the launcher's check of its marks
    /// folder cannot drift from them. `base.sb`'s fixed device paths are not parameters.
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS launcher and the tests read it")
    )]
    pub(crate) fn writable(&self) -> &[PathBuf] {
        &self.writable
    }

    /// `sandbox-exec`'s arguments before the program: `-p <text>`, then `-D NAME=path` for
    /// each parameter.
    pub fn args(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from("-p"), OsString::from(&self.text)];
        for (name, path) in &self.params {
            let mut pair = OsString::from(name);
            pair.push("=");
            pair.push(path);
            args.push(OsString::from("-D"));
            args.push(pair);
        }
        args
    }
}

/// The profile for `policy` with a tag the caller made and knows to pass [`check_tag`].
pub(crate) fn build(policy: &Policy, tag: &str) -> SeatbeltProfile {
    build_marked(policy, tag, None)
}

/// [`build`], plus the launch's one rule on its session mark when `mark` is set (the
/// launcher's, `macos/sweep.rs`): `file-write-data` on that file and nothing else, after every
/// other file rule, so the mark's folder stays under whatever denies it (the data dir's).
pub(crate) fn build_marked(policy: &Policy, tag: &str, mark: Option<&Path>) -> SeatbeltProfile {
    debug_assert!(check_tag(tag).is_ok(), "{tag}");
    let mut w = Writer::new(tag);
    w.mark = mark.map(Path::to_owned);
    w.write(policy);
    SeatbeltProfile {
        text: w.text,
        params: w.params,
        writable: w.writable,
    }
}

/// Refuses a tag that could break out of its string or miss the watcher's predicate.
pub(crate) fn check_tag(tag: &str) -> Result<(), Error> {
    let rest = tag.strip_prefix(TAG_PREFIX).unwrap_or_default();
    let ok = !rest.is_empty()
        && tag.len() <= TAG_MAX
        && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Error::Policy(format!(
            "launch tag {tag:?} must be `clauseo-` and then ASCII letters, digits or `-`"
        )))
    }
}

/// Builds the text and the parameter list together.
struct Writer {
    text: String,
    params: Vec<(String, PathBuf)>,
    /// `(with message "<tag>")`, on every deny rule.
    message: String,
    /// A comment written before the next rule, and dropped if a new comment comes first.
    comment: Option<String>,
    /// The launch's session mark, if any ([`build_marked`]).
    mark: Option<PathBuf>,
    /// What the allow rules with a `file-write` operation name ([`SeatbeltProfile::writable`]).
    writable: Vec<PathBuf>,
}

impl Writer {
    fn new(tag: &str) -> Writer {
        Writer {
            text: String::new(),
            params: Vec::new(),
            message: format!("(with message \"{tag}\")"),
            comment: None,
            mark: None,
            writable: Vec::new(),
        }
    }

    /// `(param "NAME")` for `path`, adding the parameter the first time. The path is the
    /// policy's canonical one, in the Unicode form the volume stored the name in; Seatbelt matches
    /// `literal` and `subpath` rules across NFC, NFD and mixed forms itself (measured on APFS,
    /// 2026-11-03), so no variants are written. Windows must use the path its own API returns.
    fn param(&mut self, name: &str, path: &Path) -> String {
        if !self.params.iter().any(|(n, _)| n == name) {
            self.params.push((name.to_owned(), path.to_owned()));
        }
        format!("(param \"{name}\")")
    }

    /// A comment for the next rule: a new section when it starts with a newline, else added
    /// to the section's comment.
    fn note(&mut self, comment: &str) {
        let mut text = if comment.starts_with('\n') {
            String::new()
        } else {
            self.comment.take().unwrap_or_default()
        };
        text.push_str(comment);
        text.push('\n');
        self.comment = Some(text);
    }

    /// Records the paths `filters` name ([`filters::named_paths`]), but the session mark's.
    fn record_writable(&mut self, filters: &[String]) {
        let named: Vec<PathBuf> = filters::named_paths(filters, &self.params, SESSION_MARK);
        for path in named {
            if !self.writable.contains(&path) {
                self.writable.push(path);
            }
        }
    }

    fn line(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }

    /// One rule `(<action> <ops> <filters...> [message])` with one filter per line; nothing
    /// when there are no filters, since a rule with none would match everything.
    fn rule(&mut self, action: &str, ops: &str, filters: &[String]) {
        if filters.is_empty() {
            return;
        }
        if let Some(comment) = self.comment.take() {
            self.text.push_str(&comment);
        }
        if action == "allow" && ops.contains("file-write") {
            self.record_writable(filters);
        }
        let mut rule = format!("({action} {ops}");
        let mut seen: Vec<&String> = Vec::new();
        for filter in filters {
            if seen.contains(&filter) {
                continue;
            }
            seen.push(filter);
            rule.push_str("\n  ");
            rule.push_str(filter);
        }
        if action == "deny" {
            rule.push_str("\n  ");
            rule.push_str(&self.message);
        }
        rule.push(')');
        self.line(&rule);
    }

    fn write(&mut self, policy: &Policy) {
        let p = Params::new(policy);
        self.line("(version 1)");
        let deny_default = format!("(deny default {})", self.message);
        self.line(&deny_default);
        self.line("");
        self.text.push_str(BASE);
        self.allows(policy, &p);
        self.keychain(policy, &p);
        self.denies(policy, &p);
        self.allow_again(&p);
        self.fixed_folders(&p);
        self.credential_link_metadata(&p);
        self.read_only_state(&p);
        self.session_mark();
        self.browser_sockets(policy);
        self.network(policy);
        bridge::browser_allows(self, policy);
        bridge::browser_denies(self, policy);
    }

    /// The policy's allows, before any of its denies.
    fn allows(&mut self, policy: &Policy, p: &Params) {
        self.note("\n; The matter and the state folder: read and write.");
        let matter = self.param("MATTER", policy.matter());
        let state = self.param("STATE", policy.state());
        self.rule(
            "allow",
            "file-read* file-write*",
            &[subpath(&matter), subpath(&state)],
        );
        self.tmpdir(p);

        self.note("\n; Runtime: read, run and map as code.");
        let runtime = subpaths(self, &p.runtime);
        self.rule(
            "allow",
            "file-read* process-exec file-map-executable",
            &runtime,
        );

        if let Some(home) = policy.home() {
            self.note(
                "\n; The user's text-encoding preference, which CoreFoundation reads at start.",
            );
            let encoding = self.param("CF_USER_TEXT_ENCODING", &home.join(".CFUserTextEncoding"));
            self.rule("allow", "file-read*", &[literal(&encoding)]);
        }

        self.note("\n; Read paths and allowed folders: read only.");
        let mut read = subpaths(self, &p.read);
        read.extend(subpaths(self, &p.allowed));
        self.rule("allow", "file-read*", &read);

        self.note("\n; Notes and the Claude store's names: read and write.");
        let shared = self.shared(p);
        self.rule("allow", "file-read* file-write*", &shared);
        self.credential_link(p);

        self.note("\n; The folders above every allowed path: metadata only, so paths resolve.");
        let mut above: Vec<String> = vec![ancestors(&matter), ancestors(&state)];
        above.extend(self.reallowed_ancestors(p));
        above.extend(ancestors_of(self, &p.allowed));
        if let Some((name, path)) = &p.keychains {
            let keychains = self.param(name, path);
            above.push(ancestors(&keychains));
        }
        self.rule("allow", "file-read-metadata", &above);
    }

    /// The session's temp folder (`TMPDIR`), read and write. Only this folder: the run folder
    /// that holds it, and the token there, stay under the hidden data dir's deny.
    fn tmpdir(&mut self, p: &Params) {
        let Some(tmpdir) = &p.tmpdir else {
            return;
        };
        self.note(
            "; The session's temp folder (TMPDIR): read and write. Not the run folder above.",
        );
        let tmpdir = subpath(&self.param("TMPDIR", tmpdir));
        self.rule("allow", "file-read* file-write*", &[tmpdir]);
    }

    /// Notes and the Claude store's names, as filters.
    fn shared(&mut self, p: &Params) -> Vec<String> {
        let mut filters = Vec::new();
        if let Some(notes) = &p.notes {
            filters.push(subpath(&self.param("NOTES", notes)));
        }
        filters.extend(literals(self, &p.claude_files));
        filters.extend(subpaths(self, &p.claude_locks));
        filters
    }

    /// The credential link: read, and its data written in place (Codex's token refresh). Its
    /// metadata is refused after every allow that covers it, in
    /// [`Writer::credential_link_metadata`].
    fn credential_link(&mut self, p: &Params) {
        let Some(link) = &p.credential_link else {
            return;
        };
        self.note("; The credential link: read, and its data written in place (a token refresh).");
        let link = literal(&self.param("CREDENTIAL_LINK", link));
        self.rule("allow", "file-read* file-write-data", &[link]);
    }

    /// The ancestors of the paths allowed again after the hidden paths.
    fn reallowed_ancestors(&mut self, p: &Params) -> Vec<String> {
        let mut out = vec![ancestors("(param \"STATE\")")];
        if let Some(tmpdir) = &p.tmpdir {
            out.push(ancestors(&self.param("TMPDIR", tmpdir)));
        }
        out.extend(ancestors_of(self, &p.runtime));
        out.extend(ancestors_of(self, &p.read));
        if let Some(notes) = &p.notes {
            out.push(ancestors(&self.param("NOTES", notes)));
        }
        if let Some(link) = &p.credential_link {
            out.push(ancestors(&self.param("CREDENTIAL_LINK", link)));
        }
        out.extend(ancestors_of(self, &p.claude_files));
        out.extend(ancestors_of(self, &p.claude_locks));
        out
    }

    /// Read-only paths in the matter, link findings and hidden paths.
    fn denies(&mut self, policy: &Policy, p: &Params) {
        self.note("\n; Read-only in the matter: protected names, every .git, over-cap files.");
        let mut read_only = Vec::new();
        let mut parents = Vec::new();
        for (name, rule) in &p.read_only {
            let param = self.param(name, rule.path());
            read_only.push(match rule {
                PathRule::Literal(_) => literal(&param),
                PathRule::Subpath(_) => subpath(&param),
            });
            parents.push(directory_ancestors(&param));
        }
        self.rule("deny", "file-write*", &read_only);
        self.note("; Their folders cannot be renamed, which would carry them out of their rules.");
        self.rule("deny", "file-write-unlink", &parents);
        self.note("; Claude's instruction file at any depth, in any letter case.");
        self.rule(
            "deny",
            "file-write*",
            &[format!(
                "(require-all (subpath (param \"MATTER\")) (regex #\"{INSTRUCTIONS_REGEX}\"))"
            )],
        );
        self.note("; No new .git, folder or file, at any depth, in any letter case: not made, not");
        self.note("; renamed onto (Seatbelt checks a rename's destination path).");
        self.matter_regex(GIT_NAME_REGEX);
        self.note("; No HEAD, the file that makes a folder with objects/ and refs/ a bare repo.");
        self.matter_regex(HEAD_REGEX);

        self.note("\n; Links that lead outside the matter, and where they lead.");
        let mut links = Vec::new();
        for (i, finding) in policy.links_outside_link_folders().enumerate() {
            links.push(literal(&self.param(&format!("LINK_{i}"), &finding.path)));
        }
        for (i, folder) in policy.link_folders().iter().enumerate() {
            links.push(subpath(&self.param(&format!("LINK_FOLDER_{i}"), folder)));
        }
        for (name, target) in &p.link_targets {
            links.push(subpath(&self.param(name, target)));
        }
        self.rule("deny", "file-read* file-write*", &links);

        self.note(
            "\n; Hidden paths: the data dir, other matters and state folders, refused folders.",
        );
        let hidden = subpaths(self, &p.hidden);
        self.rule("deny", "file-read* file-write*", &hidden);
    }

    /// A write deny on the matter's paths that `regex` matches.
    fn matter_regex(&mut self, regex: &str) {
        let filter = format!("(require-all (subpath (param \"MATTER\")) (regex #\"{regex}\"))");
        self.rule("deny", "file-write*", &[filter]);
    }

    /// The paths some hidden paths hold, allowed again after them.
    fn allow_again(&mut self, p: &Params) {
        self.note(
            "\n; Allowed again after the hidden paths: the state folder, notes, the credential",
        );
        self.note("; link, the Claude store's names, the runtime and read paths.");
        let state = subpath("(param \"STATE\")");
        let mut shared = vec![state];
        shared.extend(self.shared(p));
        self.rule("allow", "file-read* file-write*", &shared);
        self.tmpdir(p);
        self.credential_link(p);
        let runtime = subpaths(self, &p.runtime);
        self.rule(
            "allow",
            "file-read* process-exec file-map-executable",
            &runtime,
        );
        let read = subpaths(self, &p.read);
        self.rule("allow", "file-read*", &read);
        let above = self.reallowed_ancestors(p);
        self.rule("allow", "file-read-metadata", &above);
    }

    /// The matter, state, notes and temp folders themselves cannot be removed or made again: a
    /// subpath rule matches the folder too, so without this an agent could put a symbolic link
    /// in its place for the next launch to follow.
    fn fixed_folders(&mut self, p: &Params) {
        self.note("\n; The matter, state and notes folders themselves: not removed, not replaced.");
        let mut filters = vec![literal("(param \"MATTER\")"), literal("(param \"STATE\")")];
        if p.notes.is_some() {
            filters.push(literal("(param \"NOTES\")"));
        }
        if p.tmpdir.is_some() {
            self.note("; The session's temp folder too: the launcher makes it, outside the jail.");
            filters.push(literal("(param \"TMPDIR\")"));
        }
        self.rule("deny", "file-write-unlink file-write-create", &filters);
    }

    /// The credential link's metadata, refused after every allow that covers it (the state
    /// folder's among them) and before the state folder's read-only denies, which still win:
    /// the link is a hard link to the lawyer's login file.
    fn credential_link_metadata(&mut self, p: &Params) {
        let Some(link) = &p.credential_link else {
            return;
        };
        self.note("\n; The credential link, a hard link to the lawyer's login file: no chmod,");
        self.note("; chflags, xattr, chown, setugid, times or ACL. After every allow that covers");
        self.note("; it, so it wins over the state folder's (Seatbelt applies the last match).");
        self.note("; Seatbelt checks utimes as file-write* itself, not file-write-times, so all");
        self.note("; writes are denied and data, unlink and create allowed back. Unlink and");
        self.note("; create stay: the host finds a split link and re-links or refuses the launch.");
        let link = literal(&self.param("CREDENTIAL_LINK", link));
        self.rule("deny", "file-write*", std::slice::from_ref(&link));
        self.rule(
            "allow",
            "file-write-data file-write-unlink file-write-create",
            &[link],
        );
    }

    /// Read-only paths in the state folder, last of the file rules.
    fn read_only_state(&mut self, p: &Params) {
        self.note(
            "\n; Read-only in the state folder: config the host rewrites before each launch.",
        );
        let mut filters = Vec::new();
        let mut parents = Vec::new();
        for (name, path) in &p.read_only_state {
            let param = self.param(name, path);
            filters.push(subpath(&param));
            parents.push(directory_ancestors(&param));
        }
        self.rule("deny", "file-write*", &filters);
        self.rule("deny", "file-write-unlink", &parents);
    }

    /// The launch's session mark: `file-write-data` on that one file, which the host's sweep
    /// asks the kernel about (`sandbox_check`) to find this launch's processes wherever they
    /// went. Its canary beside it is named by no rule. Last of the file rules, so it wins over
    /// the hidden data dir's deny around it; nothing else in its folder is allowed.
    fn session_mark(&mut self) {
        let Some(mark) = self.mark.clone() else {
            return;
        };
        self.note("\n; The session mark: data writes to this one file, so the host can find every");
        self.note("; process of this launch, whatever its parent, session or group (0038).");
        let mark = literal(&self.param(SESSION_MARK, &mark));
        self.rule("allow", "file-write-data", &[mark]);
    }

    /// The browser socket folder the host made: regular files and sockets in it may be made,
    /// and files in it written, read and removed; the folder itself is neither made nor
    /// removed, and no folder, link or pipe is made in it. Nothing else in `/tmp`.
    fn browser_sockets(&mut self, policy: &Policy) {
        let Some(dir) = policy.browser_socket_dir() else {
            return;
        };
        self.note("\n; The browser socket folder: agent-browser's socket and the files beside it.");
        let dir = self.param(BROWSER_SOCKETS, dir);
        self.rule(
            "allow",
            "file-read* file-write-data file-write-unlink",
            &[subpath(&dir)],
        );
        self.note("; Only regular files and sockets are made there: no folder, link or pipe.");
        // A swap (renamex_np, RENAME_SWAP) with a matter path can still bring a folder in;
        // Seatbelt cannot refuse it without refusing the matter's renames (0038).
        let made = ["REGULAR-FILE", "SOCKET"]
            .map(|kind| format!("(require-all {} (vnode-type {kind}))", subpath(&dir)));
        self.rule("allow", "file-write-create", &made);
        self.note("; The folder itself: not removed, not replaced (the host owns it).");
        self.rule(
            "deny",
            "file-write-create file-write-unlink",
            &[literal(&dir)],
        );
        self.note("; The folders above it: metadata only, so its path resolves.");
        self.rule("allow", "file-read-metadata", &[ancestors(&dir)]);
    }

    /// TCP out only: no inet bind or listener, whatever the policy. Seatbelt's `localhost`
    /// means any address of this Mac (measured), so each rule holds by its port and its
    /// family: `tcp4` and `tcp6` leave out UDP, which the `ip` forms would let through.
    fn network(&mut self, policy: &Policy) {
        self.note(
            "\n; Network: TCP out to the proxy's ports on 127.0.0.1 and to the browser socket",
        );
        self.note("; folder only; no DNS, no UDP, and no inet bind or listener at all.");
        let mut out: Vec<String> = policy
            .proxy_ports()
            .iter()
            .map(|port| format!("(remote tcp4 \"localhost:{port}\")"))
            .collect();
        let sockets = policy
            .browser_socket_dir()
            .map(|dir| self.param(BROWSER_SOCKETS, dir));
        if let Some(dir) = &sockets {
            out.push(format!("(remote unix-socket (subpath {dir}))"));
        }
        let reach: Vec<String> = match policy.loopback() {
            Loopback::None => Vec::new(),
            Loopback::Ports(ports) => ports
                .iter()
                .map(|port| format!("localhost:{port}"))
                .collect(),
            Loopback::Any => vec!["localhost:*".to_owned()],
        };
        for at in &reach {
            out.push(format!("(remote tcp4 \"{at}\")"));
            out.push(format!("(remote tcp6 \"{at}\")"));
        }
        self.rule("allow", "network-outbound", &out);
        if let Some(dir) = &sockets {
            self.note("; agent-browser's daemon, in the jail, makes its socket in its folder.");
            self.rule(
                "allow",
                "network-bind",
                &[format!("(local unix-socket (subpath {dir}))")],
            );
        }
    }

    fn keychain(&mut self, policy: &Policy, p: &Params) {
        if !policy.keychain() {
            return;
        }
        self.note("\n; Claude's Keychain. Broad: narrow it after G7 row 17.");
        self.rule(
            "allow",
            "mach-lookup",
            &[
                "(global-name \"com.apple.SecurityServer\")".to_owned(),
                "(global-name \"com.apple.securityd.xpc\")".to_owned(),
                "(global-name \"com.apple.trustd.agent\")".to_owned(),
            ],
        );
        if let Some((name, path)) = &p.keychains {
            let keychains = self.param(name, path);
            self.rule("allow", "file-read*", &[subpath(&keychains)]);
        }
    }
}

mod bridge;
mod filters;
mod names;
#[cfg(test)]
mod network_tests;
mod params;
#[cfg(test)]
mod tests;
