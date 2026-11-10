//! Windows has no folder watch (decision record 0055): the `notify` crate is not built there, and
//! [`super::follow::opened`] follows nothing, since `papers_live` is off. This stands in for
//! `live.rs` so the follow code is the same on every OS; each call does nothing.

use std::path::Path;

use clauseo_protocol::MatterId;

use crate::actor::Actor;

/// No matter is followed on Windows.
pub(crate) enum Follow {}

pub(super) fn follow(_actor: &mut Actor, _matter: MatterId, _folder: &Path) {}

pub(super) fn relocated(_actor: &mut Actor, _matter: MatterId, _folder: &Path) {}

pub(super) fn forgotten(actor: &mut Actor, _matter: MatterId) {
    if let Some(follow) = actor.papers_follow.take() {
        match *follow {}
    }
}

pub(super) fn found(_actor: &mut Actor, _matter: MatterId, _folder: &Path) {}
