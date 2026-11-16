//! The `network` object of Claude Code's sandbox (`super::sandbox`, macOS and Linux): which hosts
//! a sandboxed Bash command may reach (decision record 0065, "Claude's shell network").
//!
//! - `allowedDomains` ([`allowed_domains`]): the seeded legal sites Claude may also fetch unasked
//!   ([`WEB_FETCH_SITES`]) plus [`SHELL_NEEDED_HOSTS`], and in `auto` the matter's kept sites
//!   at launch ([`fed_kept_hosts`]); sorted, each once. It was `["*"]`.
//! - `deniedDomains`: loopback and the any-address ([`DENIED_DOMAINS`]), unchanged; a deny wins
//!   over an allow in Claude Code's proxy.
//! - `strictAllowlist` ([`Strict`]): on when the session launches in `auto`, off in forced ask
//!   (`default`). In `auto`, Claude Code 2.1.293 sends an unlisted host's
//!   `SandboxNetworkAccess` request to its classifier, never to the host, so no card would reach
//!   the lawyer and the matter's kept sites could not answer it; strict makes the proxy refuse it
//!   instead. Strict gates sandboxed commands only: an unlisted `WebFetch` still goes to the
//!   classifier (which fails closed when it is unavailable). Off, in `default` (and in
//!   `acceptEdits`), the request is a card, and one Yes covers that host, on every port and
//!   path, for the rest of the session. A session that falls back to `acceptEdits` after a
//!   launch in `auto`, or is switched to forced ask, keeps strict until the next launch:
//!   stricter, not weaker.
//! - `allowLocalBinding` and `allowUnixSockets`: unset.
//!
//! Matching (Claude Code 2.1.293, read from its binary): the proxy compares a host with an entry
//! lower-cased and exactly; `*.x` covers subdomains of `x` at any depth but never `x` itself; an
//! entry without a port covers every port. A `WebFetch(domain:x)` rule without `*` matches the
//! URL's host exactly too (`*.x` the same way), so the plain hosts here reach exactly what the
//! seeded `WebFetch` rules reach, no wider and no narrower; no `*.` form is added.
//!
//! Claude Code also merges `WebFetch(domain:…)` allow rules from the settings files it loads
//! into this list. The seeds are not among them: they go in as `allowedTools` (a `cliArg` rule,
//! `profile::claude_options`), not as a settings file's `permissions.allow`, so the list here
//! names them itself. A matter's kept sites go in only in `auto`, at launch, so there the
//! lawyer's Remove acts from the next conversation; in the ask modes the host answers their
//! cards and a Remove acts at once.
//!
//! Windows has no Claude sandbox, so none of this applies there: a shell command's network is
//! judged by Anthropic's classifier alone (docs/threat-model.md, "Claude's shell network").

use serde_json::{Map, Value, json};

use super::sandbox::DENIED_DOMAINS;
use crate::browser::seed_sites::WEB_FETCH_SITES;

/// The hosts a sandboxed command needs beyond the seeded sites. Empty until stage C's census of
/// what Claude's shell reaches in real legal work fills it. A court site's CDN host goes in only
/// where the court site itself is seeded ([`WEB_FETCH_SITES`]); package indexes (pypi.org,
/// registry.npmjs.org) and code hosts (github.com) stay out unless the census shows a need.
pub(crate) const SHELL_NEEDED_HOSTS: &[&str] = &[];

/// Whether Claude Code's proxy refuses an unlisted host instead of sending it to the mode
/// (`network.strictAllowlist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strict {
    /// An unlisted host goes to the permission mode: a card in `default` and `acceptEdits`.
    Off,
    /// An unlisted host is refused without asking: `auto`, where the classifier would otherwise
    /// judge it with no card.
    On,
}

impl Strict {
    /// The launch's choice ([`super::claude_mode`]): on when the session starts in
    /// [`super::CLAUDE_MODE`] (`auto`), off in forced ask (`default`). A switch to forced ask
    /// later in the conversation keeps it on until the next launch: stricter, not weaker.
    pub(crate) fn at_launch(forced_ask: bool) -> Self {
        if super::claude_mode(forced_ask) == super::CLAUDE_MODE {
            Self::On
        } else {
            Self::Off
        }
    }
}

/// Whether `host` may go in the list as a matter's kept site: a plain lower-case DNS name of two
/// or more labels (letters, digits, inner hyphens; at most 63 bytes a label, 253 in all) whose
/// last label is not all digits, and not loopback or another denied host. So no wildcard, IP
/// address, port, scheme, path or trailing dot: an entry reaches exactly its host.
pub(crate) fn is_plain_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    !host.is_empty()
        && host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| !tld.bytes().all(|b| b.is_ascii_digit()))
        && labels.last() != Some(&"localhost")
        && !DENIED_DOMAINS.contains(&host)
}

/// The matter's kept sites the launch feeds in: under [`Strict::On`] (`auto`) each of
/// `kept_hosts` that [`is_plain_host`], else none. In the ask modes the card answers a kept
/// site (stage B's arm), so the lawyer's Remove acts at once there; in `auto` it acts from the
/// next conversation.
pub(crate) fn fed_kept_hosts(strict: Strict, kept_hosts: &[String]) -> Vec<&str> {
    match strict {
        Strict::Off => Vec::new(),
        Strict::On => kept_hosts
            .iter()
            .map(String::as_str)
            .filter(|host| is_plain_host(host))
            .collect(),
    }
}

/// `network.allowedDomains`: [`WEB_FETCH_SITES`], [`SHELL_NEEDED_HOSTS`] and `kept_hosts`,
/// sorted, each once. `kept_hosts` is what [`fed_kept_hosts`] gave.
pub(crate) fn allowed_domains<'a>(kept_hosts: &[&'a str]) -> Vec<&'a str> {
    let mut hosts: Vec<&'a str> = WEB_FETCH_SITES
        .iter()
        .chain(SHELL_NEEDED_HOSTS)
        .chain(kept_hosts)
        .copied()
        .collect();
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

/// The sandbox's `network` object (see the module doc).
pub(crate) fn network(strict: Strict, kept_hosts: &[&str]) -> Value {
    let mut network = json!({
        "allowedDomains": allowed_domains(kept_hosts),
        "deniedDomains": DENIED_DOMAINS,
    });
    if strict == Strict::On
        && let Some(object) = network.as_object_mut()
    {
        object.insert("strictAllowlist".to_owned(), Value::Bool(true));
    }
    network
}

/// Puts the launch's `network` object in the flag settings' `sandbox`, when there is one (a
/// session with Claude's sandbox): strict in `auto` with the fed kept sites
/// ([`Strict::at_launch`], [`fed_kept_hosts`]), the plain list otherwise.
pub(crate) fn launch_network(
    settings: &mut Map<String, Value>,
    forced_ask: bool,
    kept_hosts: &[String],
) {
    let strict = Strict::at_launch(forced_ask);
    let fed = fed_kept_hosts(strict, kept_hosts);
    if let Some(sandbox) = settings.get_mut("sandbox").and_then(Value::as_object_mut) {
        sandbox.insert("network".to_owned(), network(strict, &fed));
    }
}
