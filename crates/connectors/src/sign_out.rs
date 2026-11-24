//! Sign-out and remove: the lawyer's way to end case law's credential, and to forget the
//! connector on this install. What a keychain refusal does to each is in [`SignOutFailed`].

use std::sync::atomic::Ordering;

use crate::connectors::{ConnectorHandle, Slot, lock};
use crate::credential::{Credential, RemoveReport, SignOutReport};
use crate::error::{Error, StoreError};
use crate::secret::Secret;
use crate::state::Ended;

/// Why a sign-out stopped short.
enum SignOutFailed {
    /// The keychain refused to read the item (macOS), or its delete when no revoke was asked
    /// for: nothing was revoked, deleted or recorded. `StoreError::Refused`.
    Unreadable(StoreError),
    /// The keychain refused the delete, after a revoke was asked for: the item and the state
    /// file stay. The keychain's own error.
    NotDeleted(StoreError),
    /// The state file could not be written after the delete.
    Other(Error),
}

/// [`ConnectorHandle::sign_out`].
pub(crate) fn sign_out(handle: &ConnectorHandle) -> Result<SignOutReport, Error> {
    sign_out_keeping(handle).map_err(|failed| match failed {
        SignOutFailed::Unreadable(err) | SignOutFailed::NotDeleted(err) => Error::Store(err),
        SignOutFailed::Other(err) => err,
    })
}

/// [`ConnectorHandle::sign_out`], saying whether the item stayed in the keychain.
fn sign_out_keeping(handle: &ConnectorHandle) -> Result<SignOutReport, SignOutFailed> {
    if let Some((_, pending)) = lock(&handle.inner.pending).take() {
        pending.cancel();
    }
    // Asked before the lock: a slow proxy lookup never holds the credential.
    let route = handle.inner.shared.agent.route();
    let mut slot = lock(&handle.inner.slot);
    let refuses = handle.refuses_unreadable();
    let credential = match handle.load(&mut slot) {
        Ok(credential) => credential.cloned(),
        Err(err) if refuses => return Err(SignOutFailed::Unreadable(handle.unreadable(&err))),
        Err(err) => {
            tracing::warn!(connector = handle.id(), error = %err, "the keychain item could not be read; deleting it");
            None
        }
    };
    // A later read of the item was refused (before a refresh, say): it is read again first,
    // and its own refresh token is revoked too when it holds another one.
    let stored = if handle.inner.read_refused.load(Ordering::SeqCst) {
        match handle.read_stored() {
            Ok(stored) => stored,
            Err(err) if refuses => {
                return Err(SignOutFailed::Unreadable(handle.unreadable(&err)));
            }
            Err(err) => {
                tracing::warn!(connector = handle.id(), error = %err, "the keychain item could not be read again; deleting it");
                None
            }
        }
    } else {
        None
    };
    let mut tokens: Vec<&Secret> = Vec::new();
    for held in [&credential, &stored].into_iter().flatten() {
        if let Credential::OAuth(oauth) = held
            && !tokens.contains(&&oauth.refresh_token)
        {
            tokens.push(&oauth.refresh_token);
        }
    }
    let asked_revoke = !tokens.is_empty();
    let mut revoked = false;
    for token in tokens {
        revoked |= crate::refresh::revoke_on(handle, token, &route);
    }
    if let Err(err) = handle.inner.shared.store.delete(handle.id()) {
        // The item stays and the state file says what it said: nothing records a sign-out
        // that did not happen. The next look reads the item again. `Refused` ("nothing was
        // changed") only when no revoke was asked for (an API key): once one was, the
        // connector may have ended the sign-in, so the keychain's own error.
        tracing::warn!(connector = handle.id(), error = %err, asked_revoke, "the keychain refused the delete; the item and the state file stay");
        if !handle.persist_failed() {
            handle.forget_unread(&mut slot);
        }
        return Err(if asked_revoke {
            SignOutFailed::NotDeleted(err)
        } else {
            SignOutFailed::Unreadable(handle.unreadable(&err))
        });
    }
    *slot = Slot::Read(None);
    handle.inner.generation.fetch_add(1, Ordering::SeqCst);
    handle.inner.persist_failed.store(false, Ordering::Relaxed);
    handle
        .save_state(|state| state.end(handle.id(), Ended::SignedOut))
        .map_err(|err| SignOutFailed::Other(err.into()))?;
    Ok(SignOutReport { revoked })
}

/// [`ConnectorHandle::remove`].
pub(crate) fn remove(handle: &ConnectorHandle) -> Result<RemoveReport, Error> {
    let signed_out = sign_out_keeping(handle);
    let mut slot = lock(&handle.inner.slot);
    let left_in_keychain = match signed_out {
        Ok(_) => false,
        Err(SignOutFailed::Unreadable(_) | SignOutFailed::NotDeleted(_)) => {
            tracing::warn!(
                connector = handle.id(),
                "removed on this install; the saved sign-in stays in the keychain"
            );
            handle.forget_unread(&mut slot);
            handle.inner.persist_failed.store(false, Ordering::Relaxed);
            true
        }
        Err(SignOutFailed::Other(err)) => {
            // The entry goes even when the state file refused the sign-out.
            handle.save_state(|state| state.remove(handle.id()))?;
            return Err(err);
        }
    };
    handle.save_state(|state| state.remove(handle.id()))?;
    Ok(RemoveReport { left_in_keychain })
}
