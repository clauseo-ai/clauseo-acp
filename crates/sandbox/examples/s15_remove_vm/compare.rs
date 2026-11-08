//! `compare <before> <after> <mode>`: every difference between two listings `snapshot` wrote,
//! in four groups, judged against what the mode expects (the usage block at the top of
//! `s15_remove_vm.rs`). Pure: it reads the two files and nothing else, so it runs on any OS.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clauseo_sandbox::windows::pool::{POOL_GROUP, POOL_PREFIX};

use crate::{Out, ROOT};

/// What the run expects between the two listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// `--remove` by someone who isn't the record's owner: nothing changed.
    NonOwner,
    /// `--remove` by the owner: the pool gone, every entry taken back.
    Owner,
    /// `--remove --per-machine` as LOCAL SYSTEM: the pool gone, entries left as the
    /// administrator's path leaves them.
    Machine,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Option<Mode> {
        match text {
            "nonowner" => Some(Mode::NonOwner),
            "owner" => Some(Mode::Owner),
            "machine" => Some(Mode::Machine),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::NonOwner => "nonowner",
            Mode::Owner => "owner",
            Mode::Machine => "machine",
        }
    }
}

/// One listing: each line's kind and key, to the rest of the line.
type Listing = BTreeMap<(String, String), String>;

fn load(path: &Path) -> Result<Listing, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut listing = Listing::new();
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut parts = line.splitn(3, '\t');
        let kind = parts.next().unwrap_or_default().to_owned();
        let key = parts.next().unwrap_or_default().to_owned();
        let rest = parts.next().unwrap_or_default().to_owned();
        listing.insert((kind, key), rest);
    }
    Ok(listing)
}

/// The lines of `kind`, by key.
fn of_kind<'a>(listing: &'a Listing, kind: &str) -> BTreeMap<&'a str, &'a str> {
    listing
        .iter()
        .filter(|((k, _), _)| k == kind)
        .map(|((_, key), rest)| (key.as_str(), rest.as_str()))
        .collect()
}

/// A security descriptor's SDDL, split: owner, DACL and SACL flags, and their entries.
#[derive(Debug, Default, PartialEq, Eq)]
struct Descriptor {
    owner: String,
    dacl_flags: String,
    dacl: Vec<String>,
    sacl_flags: String,
    sacl: Vec<String>,
}

fn parse_sddl(text: &str) -> Descriptor {
    let mut d = Descriptor::default();
    let chars: Vec<char> = text.chars().collect();
    let mut section = ' ';
    let mut ace: Option<String> = None;
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        at += 1;
        if let Some(open) = ace.as_mut() {
            if c == ')' {
                let done = std::mem::take(open);
                ace = None;
                match section {
                    'D' => d.dacl.push(done),
                    'S' => d.sacl.push(done),
                    _ => {}
                }
            } else {
                open.push(c);
            }
            continue;
        }
        if "OGDS".contains(c) && chars.get(at) == Some(&':') {
            section = c;
            at += 1;
            continue;
        }
        match (c, section) {
            ('(', _) => ace = Some(String::new()),
            (_, 'O') => d.owner.push(c),
            (_, 'D') => d.dacl_flags.push(c),
            (_, 'S') => d.sacl_flags.push(c),
            _ => {}
        }
    }
    d
}

/// An entry's trustee (its SID field) and whether it is inherited.
fn trustee(ace: &str) -> &str {
    ace.split(';').nth(5).unwrap_or_default()
}

fn inherited(ace: &str) -> bool {
    ace.split(';')
        .nth(1)
        .is_some_and(|flags| flags.contains("ID"))
}

fn low_label(ace: &str) -> bool {
    ace.starts_with("ML;") && trustee(ace) == "LW"
}

/// What `a` holds that `b` doesn't, counting repeats.
fn minus<'a>(a: &'a [String], b: &[String]) -> Vec<&'a str> {
    let mut left: Vec<&str> = b.iter().map(String::as_str).collect();
    let mut extra = Vec::new();
    for item in a {
        if let Some(i) = left.iter().position(|l| *l == item.as_str()) {
            left.swap_remove(i);
        } else {
            extra.push(item.as_str());
        }
    }
    extra
}

/// The comparison under way: the mode, the SIDs set-up made, and what was unexpected.
struct Judge<'a> {
    out: &'a mut Out,
    mode: Mode,
    /// The pool users' SIDs and the group's, from the listing before.
    made: BTreeSet<String>,
    unexpected: usize,
    left: usize,
}

impl Judge<'_> {
    fn heading(&mut self, text: &str) {
        self.out.line(&format!("== {text}"));
    }

    fn expected(&mut self, text: &str) {
        self.out.line(&format!("  expected: {text}"));
    }

    fn unexpected(&mut self, text: &str) {
        self.unexpected += 1;
        self.out.line(&format!("  UNEXPECTED: {text}"));
    }

    fn removing(&self) -> bool {
        self.mode != Mode::NonOwner
    }

    /// Whether `ace` is one set-up or a launch wrote that `--remove` takes back: a pool user's
    /// or the group's, or (in a matter) a Low label.
    fn ours(&self, ace: &str, matter: bool) -> bool {
        self.made.contains(trustee(ace)) || (matter && low_label(ace))
    }

    fn content(&mut self, before: &Listing, after: &Listing) {
        self.heading("content changes (expected: none)");
        let (b, a) = (of_kind(before, "item"), of_kind(after, "item"));
        let keys: BTreeSet<&str> = b.keys().chain(a.keys()).copied().collect();
        for key in keys {
            match (b.get(key), a.get(key)) {
                (Some(x), Some(y)) if x == y => {}
                (x, y) => self.unexpected(&format!("{key}: {x:?} -> {y:?}")),
            }
        }
    }

    fn entries(&mut self, before: &Listing, after: &Listing) {
        self.heading("entry and label changes");
        for kind in ["acl.matter", "acl.shared", "acl.lawyer"] {
            let (b, a) = (of_kind(before, kind), of_kind(after, kind));
            let keys: BTreeSet<&str> = b.keys().chain(a.keys()).copied().collect();
            for key in keys {
                match (b.get(key), a.get(key)) {
                    (Some(x), Some(y)) => self.descriptor(kind, key, x, y),
                    (x, y) => self.unexpected(&format!("{kind} {key}: {x:?} -> {y:?}")),
                }
                if let Some(now) = a.get(key).filter(|_| self.removing()) {
                    self.leftovers(kind, key, now);
                }
            }
        }
    }

    /// One item's descriptor before (`was`) and after (`now`); each is `<owner>\t<sddl>`.
    fn descriptor(&mut self, kind: &str, key: &str, was: &str, now: &str) {
        if was == now {
            return;
        }
        let sddl = |s: &str| parse_sddl(s.rsplit('\t').next().unwrap_or_default());
        let (b, a) = (sddl(was), sddl(now));
        let lawyer = kind == "acl.lawyer";
        let matter = kind == "acl.matter";
        if !self.removing() || lawyer {
            return self.unexpected(&format!("{kind} {key}: {was} -> {now}"));
        }
        if b.owner != a.owner {
            self.unexpected(&format!("{key}: owner {} -> {}", b.owner, a.owner));
        }
        if b.dacl_flags != a.dacl_flags {
            let (f, t) = (&b.dacl_flags, &a.dacl_flags);
            self.unexpected(&format!("{key}: DACL flags {f} -> {t}"));
        }
        if b.sacl_flags != a.sacl_flags && !a.sacl.is_empty() {
            let (f, t) = (&b.sacl_flags, &a.sacl_flags);
            self.unexpected(&format!("{key}: SACL flags {f} -> {t}"));
        }
        let removed = minus(&b.dacl, &a.dacl).into_iter();
        for ace in removed.chain(minus(&b.sacl, &a.sacl)) {
            if self.ours(ace, matter) {
                self.expected(&format!("{key}: removed ({ace})"));
            } else {
                self.unexpected(&format!("{key}: removed ({ace})"));
            }
        }
        let added = minus(&a.dacl, &b.dacl).into_iter();
        for ace in added.chain(minus(&a.sacl, &b.sacl)) {
            self.unexpected(&format!("{key}: added ({ace})"));
        }
    }

    /// Entries naming the removed SIDs, or Low labels in a matter, still there after.
    fn leftovers(&mut self, kind: &str, key: &str, y: &str) {
        if kind == "acl.lawyer" {
            return;
        }
        let matter = kind == "acl.matter";
        let root = is_matter_root(key);
        let d = parse_sddl(y.rsplit('\t').next().unwrap_or_default());
        for ace in d.dacl.iter().chain(&d.sacl) {
            if !self.ours(ace, matter) {
                continue;
            }
            // The administrator's path takes off each matter folder's own pool allows and its
            // label, and nothing else (`ledger_work.rs`).
            let admin_takes = root && !inherited(ace) && (ace.starts_with("A;") || low_label(ace));
            if self.mode == Mode::Machine && !admin_takes {
                self.left += 1;
                self.out
                    .line(&format!("  left (as ledger_work.rs says): {key}: ({ace})"));
            } else {
                self.unexpected(&format!("{key}: still holds ({ace})"));
            }
        }
    }

    fn accounts(&mut self, before: &Listing, after: &Listing) {
        self.heading("account and group changes");
        let pool = |name: &str| name.to_lowercase().starts_with(&POOL_PREFIX.to_lowercase());
        let group = |name: &str| name.eq_ignore_ascii_case(POOL_GROUP);
        let (b, a) = (of_kind(before, "user"), of_kind(after, "user"));
        if self.removing() && !b.keys().any(|n| pool(n)) {
            self.unexpected("no ClauseoSb account before: run prepare first");
        }
        self.named(&b, &a, "user", &pool);
        let (b, a) = (of_kind(before, "group"), of_kind(after, "group"));
        self.named(&b, &a, "group", &group);
        let (b, a) = (of_kind(before, "profiledir"), of_kind(after, "profiledir"));
        self.named(&b, &a, "profile folder", &pool);
    }

    /// Users, groups or profile folders by name: those `ours` names may go (and must, when
    /// removing); a group's members may lose the removed SIDs; nothing else changes.
    fn named(
        &mut self,
        b: &BTreeMap<&str, &str>,
        a: &BTreeMap<&str, &str>,
        what: &str,
        ours: &dyn Fn(&str) -> bool,
    ) {
        let keys: BTreeSet<&str> = b.keys().chain(a.keys()).copied().collect();
        for key in keys {
            match (b.get(key), a.get(key)) {
                (Some(x), Some(y)) if x == y => {
                    if self.removing() && ours(key) {
                        self.unexpected(&format!("{what} {key} is still there"));
                    }
                }
                (Some(x), None) if self.removing() && ours(key) => {
                    self.expected(&format!("{what} {key} removed ({x})"));
                }
                (Some(x), Some(y)) if self.removing() && what == "group" => {
                    self.members(key, x, y, ours(key));
                }
                (x, y) => self.unexpected(&format!("{what} {key}: {x:?} -> {y:?}")),
            }
        }
    }

    /// A group's line before (`x`) and after (`y`): `<sid>\t<member,member,...>`.
    fn members(&mut self, key: &str, x: &str, y: &str, ours: bool) {
        let split = |s: &str| -> (String, BTreeSet<String>) {
            let (sid, members) = s.split_once('\t').unwrap_or((s, ""));
            let set = members.split(',').filter(|m| !m.is_empty());
            (sid.to_owned(), set.map(str::to_owned).collect())
        };
        let ((bs, bm), (as_, am)) = (split(x), split(y));
        let gone: Vec<&String> = bm.difference(&am).collect();
        let only_made = gone.iter().all(|m| self.made.contains(*m));
        if ours || bs != as_ || !am.is_subset(&bm) || !only_made {
            return self.unexpected(&format!("group {key}: {x} -> {y}"));
        }
        self.expected(&format!("group {key} lost the removed members {gone:?}"));
    }

    fn record(&mut self, before: &Listing, after: &Listing) {
        self.heading("record changes");
        let state = |l: &Listing| {
            let key = ("record".to_owned(), "state".to_owned());
            l.get(&key).cloned().unwrap_or_default()
        };
        let (b, a) = (state(before), state(after));
        let (bv, av) = (
            of_kind(before, "recordvalue"),
            of_kind(after, "recordvalue"),
        );
        if !self.removing() {
            if b != a || bv != av {
                self.unexpected(&format!("record {b} -> {a}"));
            }
            return;
        }
        if b == "present" && a == "absent" && av.is_empty() {
            self.expected(&format!("record removed ({} values)", bv.len()));
        } else {
            self.unexpected(&format!("record {b} -> {a}, {} values after", av.len()));
        }
    }
}

/// Whether `path` is a matter folder itself: `<ROOT>\matters\<name>`.
fn is_matter_root(path: &str) -> bool {
    let matters = format!("{ROOT}\\matters\\").to_lowercase();
    path.to_lowercase()
        .strip_prefix(&matters)
        .is_some_and(|name| !name.is_empty() && !name.contains('\\'))
}

/// The `compare` step: `Ok` once judged (the verdict is in `out`); `Err` when a listing can't
/// be read.
pub(crate) fn compare(
    out: &mut Out,
    before: &Path,
    after: &Path,
    mode: Mode,
) -> Result<(), String> {
    let (b, a) = (load(before)?, load(after)?);
    let me = |l: &Listing| {
        of_kind(l, "me")
            .into_iter()
            .next()
            .map(|(k, _)| k.to_owned())
    };
    let made: BTreeSet<String> = of_kind(&b, "user")
        .into_iter()
        .filter(|(name, _)| name.starts_with(POOL_PREFIX))
        .chain(
            of_kind(&b, "group")
                .into_iter()
                .filter(|(n, _)| *n == POOL_GROUP),
        )
        .filter_map(|(_, rest)| rest.split('\t').next().map(str::to_owned))
        .collect();
    let mut judge = Judge {
        out,
        mode,
        made,
        unexpected: 0,
        left: 0,
    };
    judge.heading("who listed");
    let (mb, ma) = (me(&b), me(&a));
    if mb.is_none() || mb != ma {
        judge.unexpected(&format!("the listings are by {mb:?} and {ma:?}"));
    } else {
        judge.expected(&format!("both by {}", mb.unwrap_or_default()));
    }
    judge.content(&b, &a);
    judge.entries(&b, &a);
    judge.accounts(&b, &a);
    judge.record(&b, &a);
    let (unexpected, left) = (judge.unexpected, judge.left);
    out.evidence("SIDs set-up made, from the listing before", &made_count(&b));
    if mode == Mode::Machine {
        out.evidence("entries left naming removed SIDs", &left.to_string());
    }
    out.check(
        unexpected == 0,
        &format!("{unexpected} unexpected differences for {}", mode.name()),
    );
    Ok(())
}

fn made_count(before: &Listing) -> String {
    let users = of_kind(before, "user");
    let pool = users.keys().filter(|n| n.starts_with(POOL_PREFIX)).count();
    let group = of_kind(before, "group").contains_key(POOL_GROUP);
    format!("{pool} pool users, group {group}")
}
