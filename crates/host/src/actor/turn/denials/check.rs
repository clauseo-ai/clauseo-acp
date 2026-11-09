//! A denial's path checked off the actor, at most [`super::count::MAX_IN_FLIGHT`] at once per
//! thread: resolved without touching a network location ([`resolve`]), pinned by identity to
//! the session's known places ([`Places::pin`]), then decided by the offer rules. The tool-call
//! source first finds which named path lies outside the jail, in the same check.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_protocol::{DenialKind, DenialSource, ThreadId, ToolCallId, TurnId};
use clauseo_sandbox::PathClass;
use futures::task::LocalSpawnExt;

use super::count::InFlight;
use super::network::{NetEnv, is_network_path};
use super::offer::identity::{Captured, Identity, RealIdentity, Target};
use super::offer::{self, Facts, Jail, Offer, OfferRules, Refused};
use super::resolve::{self, Disk, Resolved};
use crate::actor::input::Input;
use crate::actor::turn::TurnInput;
use crate::actor::{Actor, turn::updates};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::config::SessionKey;
use crate::network_paths::under_share_matter;

/// A denial decided (off the actor for a path), on its way to be recorded.
#[derive(Debug)]
pub(crate) struct Checked {
    pub(crate) thread: ThreadId,
    pub(crate) turn: Option<TurnId>,
    pub(crate) source: DenialSource,
    pub(crate) decided: Decided,
}

/// What the rules made of one denial.
#[derive(Debug, Clone)]
pub(crate) struct Decided {
    /// The card, as an OS report would get it (a tool call's is cut down when recorded).
    pub(crate) offer: Offer,
    /// Why the path was not checked (a network location, or it could not be resolved): logged
    /// as such, never offered.
    pub(crate) unchecked: Option<Unchecked>,
    /// What the card is about, for its dedupe.
    pub(crate) card: Target,
    /// The path refused, resolved and pinned (as written when not checked); `None` for a site.
    pub(crate) asked: Option<Target>,
    /// What the tool-call ruling needs; `None` for a path not checked, or a site.
    pub(crate) facts: Option<Facts>,
}

impl Decided {
    /// A site's or an upstream's card: known by its text, without case.
    pub(crate) fn of_text(offer: Offer, text: &str) -> Self {
        Self {
            offer,
            unchecked: None,
            card: Target::text(text, true),
            asked: None,
            facts: None,
        }
    }
}

/// Why a path's card was decided without resolving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unchecked {
    /// It names a network location, or a link on the way does.
    Network,
    /// A loop, a link that can't be read, or an I/O error on the way.
    Unresolved,
}

impl Unchecked {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Unresolved => "unresolved",
        }
    }
}

/// One thread's session: its rules walked and its places captured once, again only for a new
/// launch, when the host's rules change, or when a place missing at capture has appeared since
/// (a new `.ssh`: [`Places::stale`]). Shared with the checks off the actor.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionCache(Arc<Mutex<Option<Held>>>);

/// The launch and the rules it was captured for, and what was captured.
#[derive(Debug)]
struct Held {
    launch: u64,
    rules: u64,
    captured: Arc<Captured>,
}

impl SessionCache {
    /// The session's capture for `launch` under `raw`: the one held, or a new one (off the
    /// actor: it reads `disk`).
    pub(crate) fn captured(
        &self,
        launch: u64,
        raw: OfferRules,
        disk: &Disk<'_>,
        ids: &dyn Identity,
    ) -> Arc<Captured> {
        let mut hasher = DefaultHasher::new();
        format!("{raw:?}").hash(&mut hasher);
        let rules = hasher.finish();
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = held.as_ref()
            && held.launch == launch
            && held.rules == rules
            && !held.captured.places.stale(ids)
        {
            return Arc::clone(&held.captured);
        }
        let captured = Arc::new(Captured::new(raw, disk, ids));
        *held = Some(Held {
            launch,
            rules,
            captured: Arc::clone(&captured),
        });
        captured
    }
}

/// A path's check from the sandbox or the proxy, holding `flight`, back on the actor as
/// `TurnInput::Denial`.
pub(crate) fn check_path(
    actor: &mut Actor,
    key: SessionKey,
    turn: Option<TurnId>,
    source: DenialSource,
    rules: OfferRules,
    (path, class): (PathBuf, PathClass),
    flight: InFlight,
) {
    let thread = key.thread;
    let job = Job::short("denial check", Some(rules.matter));
    let clock = Arc::clone(&actor.clock);
    let session = actor.denials.session(thread);
    spawn_maybe(actor, async move {
        let work = move || {
            let _flight = flight;
            let (net, ids) = (NetEnv::current(), RealIdentity);
            let disk = Disk::real(&net);
            let captured = session.captured(key.launch, rules, &disk, &ids);
            Ok(decide(&captured, &path, class, &disk, &ids))
        };
        let decided = off_actor(clock, job, work).await.ok()?;
        let checked = Checked {
            thread,
            turn,
            source,
            decided,
        };
        Some(Input::Turn(TurnInput::Denial(Box::new(checked))))
    });
}

/// A failed tool call's check, holding `flight`: the first path it named that lies outside the
/// jail, decided, back on the actor as `TurnInput::Denial`; nothing when none does.
pub(crate) fn check_tool_call(
    actor: &mut Actor,
    (thread, turn, call): (ThreadId, TurnId, ToolCallId),
    rules: OfferRules,
    named: Vec<String>,
    flight: InFlight,
) {
    let Some(agent) = updates::agent(actor, thread) else {
        return;
    };
    let (launch, folder) = (agent.id, agent.folder.clone());
    let job = Job::short("tool call denial check", Some(rules.matter));
    let clock = Arc::clone(&actor.clock);
    let session = actor.denials.session(thread);
    spawn_maybe(actor, async move {
        let work = move || {
            let _flight = flight;
            let (net, ids) = (NetEnv::current(), RealIdentity);
            let disk = Disk::real(&net);
            let captured = session.captured(launch, rules, &disk, &ids);
            let outside = outside_jail_walked(&folder, &captured, &named, &disk, &ids);
            Ok(outside.map(|(asked, walked, class)| {
                decide_walked(&captured, &asked, walked, class, &ids)
            }))
        };
        let decided = off_actor(clock, job, work).await.ok().flatten()?;
        let checked = Checked {
            thread,
            turn: Some(turn),
            source: DenialSource::ToolCall { tool_call: call },
            decided,
        };
        Some(Input::Turn(TurnInput::Denial(Box::new(checked))))
    });
}

/// Off the actor: where the path leads on `disk` ([`resolve::resolve`], the one call to the
/// walk), then [`decide_walked`].
pub(crate) fn decide(
    captured: &Captured,
    path: &Path,
    class: PathClass,
    disk: &Disk<'_>,
    ids: &dyn Identity,
) -> Decided {
    decide_walked(captured, path, resolve::resolve(path, disk), class, ids)
}

/// The card for `path` (as the agent wrote it), which the walk resolved to `walked`: only the
/// walked local path is used from here on, pinned by identity to the session's places, then the
/// card and the facts the tool-call ruling need. A path that names a network location or can't
/// be resolved is never touched further and never offered; the agent's text is only shown.
pub(crate) fn decide_walked(
    captured: &Captured,
    path: &Path,
    walked: Resolved,
    class: PathClass,
    ids: &dyn Identity,
) -> Decided {
    let rules = &captured.rules;
    let fold = rules.fold_case;
    let unchecked = |why| {
        let shown = path.display().to_string();
        let target = Target::text(&shown, fold);
        Decided {
            offer: offer::offer(rules, Refused::Unchecked { path: &shown }),
            unchecked: Some(why),
            card: target.clone(),
            asked: Some(target),
            facts: None,
        }
    };
    match walked {
        Resolved::Network => unchecked(Unchecked::Network),
        Resolved::Unresolved => unchecked(Unchecked::Unresolved),
        Resolved::Local { path, is_file } => {
            let Ok(pinned) = captured.places.pin(&path, ids) else {
                return unchecked(Unchecked::Unresolved);
            };
            let folder = if is_file {
                path.parent().unwrap_or(&path)
            } else {
                &path
            };
            let refused = Refused::Path {
                path: &pinned,
                is_file,
                class,
                holds_protected: captured.places.holds_protected(folder, ids),
            };
            let offer = offer::offer(rules, refused);
            let facts = offer::facts(rules, &pinned, is_file, class, &Jail(rules));
            let asked = Target::of(&pinned, Some(ids), fold);
            let shown = pinned.display().to_string();
            let card = match card_path(&offer.kind) {
                Some(card) if card != shown => Target::of(Path::new(card), Some(ids), fold),
                Some(_) | None => asked.clone(),
            };
            Decided {
                offer,
                unchecked: None,
                card,
                asked: Some(asked),
                facts: Some(facts),
            }
        }
    }
}

/// The first of `named` that leads outside the matter folder (`folder` as the agent has it, or
/// the session's canonical root) and Clauseo's data folder on `fs`, with its class: `LinkTarget`
/// for a matter path that leads out. Each resolved path is pinned by identity first, so a
/// respelling of the matter folder is inside. A network path (a UNC path, `/net/…`, a link to
/// one) is outside without being touched: taken as written, never joined to the folder. One
/// under the folder by its text alone (a matter on a share) is inside, also untouched.
#[cfg(test)]
pub(crate) fn outside_jail(
    folder: &Path,
    captured: &Captured,
    named: &[String],
    disk: &Disk<'_>,
    ids: &dyn Identity,
) -> Option<(PathBuf, PathClass)> {
    let found = outside_jail_walked(folder, captured, named, disk, ids)?;
    let (asked, walked, class) = found;
    match walked {
        Resolved::Local { path, .. } => {
            Some((captured.places.pin(&path, ids).unwrap_or(asked), class))
        }
        Resolved::Network | Resolved::Unresolved => Some((asked, class)),
    }
}

/// [`outside_jail`]'s path as the agent named it (joined to the folder), what the walk made
/// of it, and its class, for [`decide_walked`] to take without walking it again.
pub(crate) fn outside_jail_walked(
    folder: &Path,
    captured: &Captured,
    named: &[String],
    disk: &Disk<'_>,
    ids: &dyn Identity,
) -> Option<(PathBuf, Resolved, PathClass)> {
    let (rules, net) = (&captured.rules, disk.net);
    let (root, data) = (&rules.matter_root, &rules.data_dir);
    let under = |path: &Path, prefix: &Path| offer::under(path, prefix, net.fold_case);
    let has_up = |path: &Path| path.components().any(|c| matches!(c, Component::ParentDir));
    named.iter().find_map(|text| {
        let raw = Path::new(text);
        let asked = if is_network_path(raw, net) {
            raw.to_path_buf()
        } else {
            folder.join(raw)
        };
        let lexical = !has_up(&asked) && (under(&asked, folder) || under(&asked, root));
        let class = if lexical {
            PathClass::LinkTarget
        } else {
            PathClass::Outside
        };
        if net.is_network(&asked) {
            let share = |matter: &Path| under_share_matter(&asked, matter, net);
            let inside = share(folder) || share(root);
            return (!inside).then_some((asked, Resolved::Network, PathClass::Outside));
        }
        match resolve::resolve(&asked, disk) {
            // A network path, or one not known to be inside: refused, its card never offered.
            walked @ (Resolved::Network | Resolved::Unresolved) => Some((asked, walked, class)),
            Resolved::Local { path, is_file } => {
                let Ok(pinned) = captured.places.pin(&path, ids) else {
                    // An identity on the way can't be read: not known to be inside.
                    return Some((asked, Resolved::Unresolved, class));
                };
                let inside = under(&pinned, root) || under(&pinned, data);
                (!inside).then_some((asked, Resolved::Local { path, is_file }, class))
            }
        }
    })
}

/// The path a card shows, if it shows one.
fn card_path(kind: &DenialKind) -> Option<&str> {
    match kind {
        DenialKind::Folder { path, .. } | DenialKind::File { path, .. } => Some(path),
        DenialKind::Site { .. } | DenialKind::UpstreamAuth { .. } | DenialKind::Unknown { .. } => {
            None
        }
    }
}

/// Runs `task` on the actor's pool and posts what it gives, if anything.
fn spawn_maybe(actor: &Actor, task: impl Future<Output = Option<Input>> + 'static) {
    let post = actor.post.clone();
    let spawned = actor.spawner.spawn_local(async move {
        if let Some(input) = task.await {
            // The actor may be gone by then; nobody is left to tell.
            let _gone = post.try_send(input);
        }
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a task");
    }
}
