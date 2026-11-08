//! The orchestrator's ruling on a card only a tool call reported (#764 review, host MED 2): a
//! tool call's path and its "access denied" text are both the agent's own words, not evidence,
//! so such a card may never offer more than the launch itself would refuse. One function,
//! [`tool_call_ruling`], so the ruling can be adjusted in one place.
//!
//! - Credential places (`.ssh`, keychains, credential stores), Clauseo's data folder, refused
//!   trees and every other never-offered place: never offered, from any source.
//! - Another matter's path: offered (read-only, as an OS report would be) only when an OS report
//!   (the sandbox, the proxy, Windows' `SandboxWatch` once it lands) of the same file by identity
//!   came in the same turn. A tool-call-only claim is logged, not offered. On Windows that makes
//!   cross-matter access picker-only until `SandboxWatch`.
//! - Anything else: offered only when the launch would deny that target
//!   ([`LaunchPolicy::would_deny`]), and only as a one-file card, never a whole folder.

use std::path::Path;

use clauseo_protocol::DenialKind;
use clauseo_sandbox::PathClass;

use super::{Offer, OfferRules};

#[cfg(test)]
mod tests;

/// What the ruling needs to know of a resolved, pinned path; worked out off the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// It lies in another matter.
    pub(crate) other_matter: bool,
    /// It is never offered, from any source (a credential place, the data folder, a refused
    /// tree, a dot name, home itself, a root).
    pub(crate) never: bool,
    /// The launch would deny it.
    pub(crate) would_deny: bool,
    /// Its one-file card, when the file alone may be offered.
    pub(crate) one_file: Option<DenialKind>,
}

/// Whether a launch refuses a path: `Launchers::would_deny` once it is on this base.
pub(crate) trait LaunchPolicy {
    /// Whether the launch would deny the resolved, pinned `path`.
    fn would_deny(&self, path: &Path) -> bool;
}

/// The launch's jail as the host builds it until `Launchers::would_deny` is here: the matter
/// folder and Clauseo's data folder (which holds the run folders) are open, all else is not.
/// Compared on a pinned path, so a respelling of either folder is still inside.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Jail<'a>(pub(crate) &'a OfferRules);

impl LaunchPolicy for Jail<'_> {
    fn would_deny(&self, path: &Path) -> bool {
        let rules = self.0;
        !rules.under(path, &rules.matter_root) && !rules.under(path, &rules.data_dir)
    }
}

/// The facts of the resolved, pinned `path` under `rules`.
pub(crate) fn facts(
    rules: &OfferRules,
    path: &Path,
    is_file: bool,
    class: PathClass,
    policy: &dyn LaunchPolicy,
) -> Facts {
    let other_matter = rules.other_matter(path).is_some();
    let never = rules.never(path);
    let alone = is_file
        && class == PathClass::Outside
        && !other_matter
        && !never
        && !rules.overlaps_this_matter(path);
    Facts {
        other_matter,
        never,
        would_deny: policy.would_deny(path),
        one_file: alone.then(|| DenialKind::File {
            path: path.display().to_string(),
            other_matter: None,
        }),
    }
}

/// A tool-call-only card: `base` (what an OS report of the same path would get) cut down by the
/// ruling. `confirmed`: an OS report of the same file by identity came in this turn.
pub(crate) fn tool_call_ruling(base: Offer, facts: &Facts, confirmed: bool) -> Offer {
    let refused = |base: Offer| Offer {
        offered: false,
        ..base
    };
    if facts.never {
        return refused(base);
    }
    if facts.other_matter {
        return if confirmed { base } else { refused(base) };
    }
    match &facts.one_file {
        Some(kind) if facts.would_deny && base.offered => Offer {
            kind: kind.clone(),
            offered: true,
        },
        Some(_) | None => refused(base),
    }
}
