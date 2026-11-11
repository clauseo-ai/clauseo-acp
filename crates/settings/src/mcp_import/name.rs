//! Names: the MCP name rules, the reserved names, and the merge of entries into candidates.

use super::{CandidateKind, Entry, ImportCandidate, Problem, Source};
use std::collections::HashSet;

/// The longest MCP server name (leaf/mcp.rs).
const NAME_MAX: usize = 64;

/// The name used when an app's entry has none.
const EMPTY_NAME: &str = "server";

/// One distinct config under a base name: its sources and problems, and the name it was first
/// read with.
struct Variant {
    from: String,
    kind: CandidateKind,
    sources: Vec<Source>,
    problems: Vec<Problem>,
}

/// The entries that share a base name, one [`Variant`] per distinct config.
struct Group {
    base: String,
    variants: Vec<Variant>,
}

/// Merges the entries into candidates. The first config of a base name keeps the name; each
/// further distinct config gets ` 2`, ` 3`, … as `-2`, `-3`, skipping names already taken.
pub(crate) fn assemble(entries: Vec<Entry>, existing: &[String]) -> Vec<ImportCandidate> {
    let mut groups: Vec<Group> = Vec::new();
    for entry in entries {
        let (base, name_problem) = clean(&entry.raw_name);
        let mut problems = entry.problems;
        problems.extend(name_problem);
        let kind = normalized(entry.kind);
        let gi = groups
            .iter()
            .position(|group| group.base == base)
            .unwrap_or_else(|| {
                groups.push(Group {
                    base: base.clone(),
                    variants: Vec::new(),
                });
                groups.len() - 1
            });
        let group = &mut groups[gi];
        let vi = group
            .variants
            .iter()
            .position(|variant| variant.kind == kind)
            .unwrap_or_else(|| {
                group.variants.push(Variant {
                    from: entry.raw_name.clone(),
                    kind: kind.clone(),
                    sources: Vec::new(),
                    problems: Vec::new(),
                });
                group.variants.len() - 1
            });
        let variant = &mut group.variants[vi];
        push_unique(&mut variant.sources, entry.source);
        for problem in problems {
            push_unique(&mut variant.problems, problem);
        }
    }

    // Names taken before any suffix is chosen: the base names and the names Clauseo has.
    let mut taken: HashSet<String> = existing.iter().cloned().collect();
    taken.extend(groups.iter().map(|group| group.base.clone()));

    let mut out = Vec::new();
    for group in groups {
        for (index, variant) in group.variants.into_iter().enumerate() {
            let mut problems = variant.problems;
            let name = if index == 0 {
                group.base.clone()
            } else {
                let name = next_free(&group.base, &mut taken);
                push_unique(
                    &mut problems,
                    Problem::NameChanged {
                        from: variant.from.clone(),
                    },
                );
                name
            };
            if existing.contains(&name) {
                push_unique(&mut problems, Problem::AlreadyAdded);
            }
            out.push(ImportCandidate {
                name,
                sources: variant.sources,
                kind: variant.kind,
                problems,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The name an app's entry gets in Clauseo, and the [`Problem::NameChanged`] when it differs:
/// bad characters become `-`, `__` becomes `_`, trailing `_` is dropped, the name is cut to
/// [`NAME_MAX`], and a reserved name gets `-imported`.
fn clean(raw: &str) -> (String, Option<Problem>) {
    let mut name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    name = trim_underscores(&name);
    if name.len() > NAME_MAX {
        name.truncate(NAME_MAX);
        name = trim_underscores(&name);
    }
    if name.is_empty() {
        EMPTY_NAME.clone_into(&mut name);
    }
    if crate::reserved_mcp_server(&name).is_some() {
        name = format!("{name}-imported");
    }
    let problem = (name != raw).then(|| Problem::NameChanged {
        from: raw.to_owned(),
    });
    (name, problem)
}

/// `name` without trailing `_`, which the name rules forbid.
fn trim_underscores(name: &str) -> String {
    name.trim_end_matches('_').to_owned()
}

/// The first `base-2`, `base-3`, … not taken; it is then taken. The base is cut so the whole
/// name stays within [`NAME_MAX`].
fn next_free(base: &str, taken: &mut HashSet<String>) -> String {
    let name = (2..=taken.len() + 2)
        .map(|n| {
            let suffix = format!("-{n}");
            let stem_len = base.len().min(NAME_MAX - suffix.len());
            format!("{}{suffix}", &base[..stem_len])
        })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| base.to_owned());
    taken.insert(name.clone());
    name
}

/// The kind with its env and header pairs sorted by name, so two configs that differ only in
/// order are one config.
fn normalized(kind: CandidateKind) -> CandidateKind {
    match kind {
        CandidateKind::Local {
            command,
            args,
            mut env,
        } => {
            env.sort();
            CandidateKind::Local { command, args, env }
        }
        CandidateKind::Remote { url, mut headers } => {
            headers.sort();
            CandidateKind::Remote { url, headers }
        }
    }
}

/// Pushes `item` unless an equal one is there already.
fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}
