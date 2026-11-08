//! The browser pins' part of each round (`browser::pins`): fetch the signature, then the
//! manifest, by the block list's fetch rules (redirects by hand to the allowed hosts, `https`
//! only in the release, routed through `network.upstream_proxy` else the seams' route), verify
//! them against the pins' keys, set aside one dated over a day ahead, apply the no-rollback rule
//! against the newest verified copy on disk (stored or bundled), and store the pair. It runs
//! whether block list updates are on or off. Whatever fails keeps the copies there are, and the
//! next round tries again.

use clauseo_sandbox::Upstream;

use super::super::fetch::{self, Failure, fetch};
use super::{SIGNATURE_TIMEOUT, Updater, named};
use crate::browser::pins::{self, MAX_MANIFEST, Pins};

/// How one browser pins round ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinsRound {
    /// A newer verified manifest is stored.
    Stored,
    /// A verified manifest that the no-rollback rule did not take: not newer, or a lower
    /// version.
    NotNewer,
    /// A 404 on either file: nothing published yet.
    NotPublished,
    /// The pair was refused: the signature, the comment, the body, or a date over a day ahead.
    Refused,
    /// The fetch or the write failed.
    Failed,
    /// The host stopped during the round: nothing written.
    Stopped,
}

/// [`Updater::refresh_pins`]'s work.
pub(super) fn refresh(this: &Updater, stopped: &dyn Fn() -> bool) -> PinsRound {
    let fetching = this.live.get();
    let fetched = fetch::upstream(fetching.upstream_proxy.as_deref(), &this.update.upstream)
        .and_then(|upstream| fetch_pins(this, &upstream));
    let (body, signature) = match fetched {
        Ok(pair) => pair,
        Err(Failure::NotPublished) => {
            tracing::info!("no browser pins published yet");
            return PinsRound::NotPublished;
        }
        Err(Failure::Refused(why)) => {
            tracing::info!(%why, "the browser pins fetch failed; the copies there are stay");
            return PinsRound::Failed;
        }
    };
    if stopped() {
        return PinsRound::Stopped;
    }
    let keys: Vec<&str> = this.pins.keys.iter().map(String::as_str).collect();
    let fetched = match Pins::verify(&body, &signature, &keys) {
        Ok(fetched) => fetched,
        Err(error) => {
            tracing::warn!(%error, "the fetched browser pins were refused");
            return PinsRound::Refused;
        }
    };
    let now = this.clock.now().wall;
    if fetched.is_ahead_of(now) {
        tracing::warn!(
            published = fetched.published().as_str(),
            "the fetched browser pins are dated over a day ahead of this computer's clock; \
             not stored"
        );
        return PinsRound::Refused;
    }
    let best = this.pins_source.newest(now);
    if !pins::accept(&fetched, best.as_ref()) {
        return PinsRound::NotNewer;
    }
    if let Err(error) = pins::store(&this.pins_source.stored, &body, &signature) {
        tracing::warn!(kind = ?error.kind(), "the new browser pins could not be stored");
        return PinsRound::Failed;
    }
    tracing::info!(
        published = fetched.published().as_str(),
        version = fetched.version(),
        run = fetched.run(),
        "the browser pins are updated"
    );
    PinsRound::Stored
}

/// The signature, then the manifest, through `upstream`, under the block list's rules.
fn fetch_pins(this: &Updater, upstream: &Upstream) -> Result<(Vec<u8>, Vec<u8>), Failure> {
    let clients = (&this.clients, upstream);
    let signature = (this.update.max_signature, SIGNATURE_TIMEOUT);
    let signature = fetch(clients, &this.pins.signature_url, &this.update, signature)
        .map_err(|failure| named(failure, "browser pins signature"))?;
    let body = fetch(
        clients,
        &this.pins.manifest_url,
        &this.update,
        (MAX_MANIFEST, SIGNATURE_TIMEOUT),
    )
    .map_err(|failure| named(failure, "browser pins"))?;
    Ok((body, signature))
}
