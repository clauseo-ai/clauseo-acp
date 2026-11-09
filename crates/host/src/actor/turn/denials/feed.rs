//! The entry point for the sandbox's and the proxy's sinks (R17 PR-D wires them): a
//! [`DenialSink`] that posts each denial to the actor as `Input::Denial`.

use clauseo_protocol::DenialSource;
use clauseo_sandbox::{Denial, DenialSink};

use super::DenialInput;
use crate::actor::input::Input;
use crate::config::SessionKey;

/// One launch's denial sink: posts each denial to the actor, never waiting, with the source
/// it was made with (`Sandbox` for the jail's watcher, `Proxy` for the egress proxy). PR-D
/// wraps it in `clauseo_sandbox::CappedSink` and hands it to the launcher and the proxy where
/// `CountDenials` goes today.
///
/// The actor's internal `Input` channel is unbounded and posted with `try_send`, so the
/// watcher's and the proxy's threads never block on it; `CappedSink`'s budget is what bounds
/// it per session (20 a second, plus the block list's own 20 since #694, which go to
/// `HostHandle::site_blocked`, not here). A send fails only once the host has stopped, and the
/// denial is then dropped.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "R17 PR-D's entry point; the launch path builds it from PR-D on"
    )
)]
pub(crate) struct DenialFeed {
    post: async_channel::Sender<Input>,
    key: SessionKey,
    source: DenialSource,
}

impl DenialFeed {
    /// A sink for the launch `key`, posting on `post` (`Actor::post`), its denials from
    /// `source`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "R17 PR-D's entry point; the launch path builds it from PR-D on"
        )
    )]
    pub(crate) fn new(
        post: async_channel::Sender<Input>,
        key: SessionKey,
        source: DenialSource,
    ) -> Self {
        Self { post, key, source }
    }
}

impl DenialSink for DenialFeed {
    fn denied(&self, d: Denial) {
        let input = DenialInput {
            key: self.key,
            kind: d,
            source: self.source.clone(),
        };
        if self.post.try_send(Input::Denial(Box::new(input))).is_err() {
            tracing::debug!("a denial was dropped: the host has stopped");
        }
    }
}
