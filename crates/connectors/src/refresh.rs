//! `bearer` and the refresh: one refresh at a time per connector (the credential slot's lock is
//! held while it runs), 5 minutes before expiry or after a 401, so rotation never forks a chain.
//! A rotated refresh token is written to the keychain before the new access token is used.
//!
//! Another `Connectors` (another process, or a second instance in this one) may have rotated
//! the chain already, so the keychain item is read again under the lock just before each
//! refresh, and its newer tokens are adopted instead of refreshing with a spent token.

use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::connectors::{ConnectorHandle, Slot, lock};
use crate::credential::{Credential, OAuthTokens};
use crate::error::AuthError;
use crate::http::{self, SendBody};
use crate::oauth::{encode_form, oauth_error_code, parse_token_answer};
use crate::secret::{Bearer, Secret};
use crate::state::Ended;

/// How long before expiry an access token is refreshed.
pub(crate) const REFRESH_MARGIN: Duration = Duration::from_mins(5);

/// The bearer token for the connector: the API key, or an access token with more than 5
/// minutes left, refreshed first when it has less. No I/O when no refresh is due. Callers on
/// other threads wait for a running refresh and then get its token.
pub(crate) fn bearer(handle: &ConnectorHandle) -> Result<Bearer, AuthError> {
    let mut slot = lock(&handle.inner.slot);
    fresh_token(handle, &mut slot).map(Bearer)
}

/// The current token, refreshed when due; the slot is locked by the caller.
pub(crate) fn fresh_token(handle: &ConnectorHandle, slot: &mut Slot) -> Result<Secret, AuthError> {
    let horizon = handle
        .inner
        .shared
        .unix_now()
        .saturating_add(REFRESH_MARGIN.as_secs());
    match handle.current(slot)? {
        Credential::ApiKey(key) => Ok(key.clone()),
        Credential::OAuth(tokens) if tokens.access_expires_at > horizon => {
            Ok(tokens.access_token.clone())
        }
        Credential::OAuth(_) => refresh_locked(handle, slot, true),
    }
}

/// After a 401 with `used`: an API key is revoked (the item goes, no refresh); an access
/// token that is still the current one is refreshed, else the newer one is taken.
pub(crate) fn after_401(handle: &ConnectorHandle, used: &Secret) -> Result<Secret, AuthError> {
    let mut slot = lock(&handle.inner.slot);
    match handle.current(&mut slot)? {
        Credential::ApiKey(_) => {
            handle.end_credential(&mut slot, Ended::KeyRevoked);
            Err(AuthError::KeyRevoked)
        }
        Credential::OAuth(tokens) if tokens.access_token != *used => {
            Ok(tokens.access_token.clone())
        }
        Credential::OAuth(_) => refresh_locked(handle, &mut slot, false),
    }
}

/// A second 401, after the refresh or the newer token: signed out.
pub(crate) fn after_second_401(handle: &ConnectorHandle, used: &Secret) -> AuthError {
    let mut slot = lock(&handle.inner.slot);
    let ended = match handle.current(&mut slot) {
        Ok(Credential::OAuth(tokens)) if tokens.access_token == *used => Ended::SignedOut,
        Ok(Credential::ApiKey(key)) if key == used => Ended::KeyRevoked,
        // Another thread changed the credential meanwhile; leave it.
        Ok(_) => return AuthError::SignedOut,
        Err(err) => return err,
    };
    handle.end_credential(&mut slot, ended);
    match ended {
        Ended::KeyRevoked => AuthError::KeyRevoked,
        Ended::SignedOut => AuthError::SignedOut,
    }
}

/// Refreshes the access token; the slot is locked by the caller. First the keychain item is
/// read again: gone is signed out; a different refresh token (another instance rotated) is
/// adopted, and its access token returned when it has more than 5 minutes left. `invalid_grant`
/// ends the credential; any other answer leaves it as it is. With `keep_on_network_error` (a
/// refresh ahead of expiry, not after a 401), a network failure or a 5xx while the access
/// token has not expired yet returns that token.
fn refresh_locked(
    handle: &ConnectorHandle,
    slot: &mut Slot,
    keep_on_network_error: bool,
) -> Result<Secret, AuthError> {
    let current = match handle.current(slot)? {
        Credential::OAuth(tokens) => tokens.clone(),
        Credential::ApiKey(key) => return Ok(key.clone()),
    };
    let current = match adopt_stored(handle, slot, current)? {
        Adopted::Fresh(token) => return Ok(token),
        Adopted::Refresh(tokens) => tokens,
    };
    let old_refresh = current.refresh_token.clone();
    let keep_current = |why: &str| -> Option<Secret> {
        let unexpired = current.access_expires_at > handle.inner.shared.unix_now();
        (keep_on_network_error && unexpired).then(|| {
            tracing::warn!(
                connector = handle.id(),
                error = why,
                "the refresh failed (network or 5xx); the current access token is still valid"
            );
            current.access_token.clone()
        })
    };
    let metadata = match crate::oauth::metadata(handle) {
        Ok(metadata) => metadata,
        Err(err) => {
            let why = format!("discovery: {err}");
            return keep_current(&why).ok_or(AuthError::Network(why));
        }
    };
    let urls = &handle.inner.urls;
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", old_refresh.expose()),
        ("client_id", urls.client_id.as_str()),
    ];
    pairs.extend(
        urls.resources
            .iter()
            .map(|resource| ("resource", resource.as_str())),
    );
    let form = encode_form(&pairs);
    let shared = &handle.inner.shared;
    let reply = match http::send(
        &shared.agent,
        "POST",
        metadata.token.as_str(),
        &[("Accept", "application/json")],
        SendBody::Form(&form),
        shared.timeout(),
    ) {
        Ok(reply) => reply,
        Err(err) => return keep_current(&err).ok_or(AuthError::Network(err)),
    };
    if reply.status != 200 {
        let code = oauth_error_code(&reply.body);
        if code == "invalid_grant" {
            tracing::info!(
                connector = handle.id(),
                "the refresh token was refused; signed out"
            );
            handle.end_credential(slot, Ended::SignedOut);
            return Err(AuthError::SignedOut);
        }
        let why = format!("the token endpoint answered HTTP {} ({code})", reply.status);
        // A 5xx is the connector failing, not an answer about the token: like the network.
        if reply.status >= 500
            && let Some(token) = keep_current(&why)
        {
            return Ok(token);
        }
        return Err(AuthError::Network(why));
    }
    let tokens = parse_token_answer(&reply.body, shared.unix_now(), Some(&old_refresh))
        .map_err(AuthError::Network)?;
    let access = tokens.access_token.clone();
    persist(handle, &tokens);
    *slot = Slot::Read(Some(Credential::OAuth(tokens)));
    Ok(access)
}

/// What the keychain item says before a refresh.
enum Adopted {
    /// Another instance's access token, with more than 5 minutes left: no refresh.
    Fresh(Secret),
    /// The tokens to refresh with.
    Refresh(OAuthTokens),
}

/// Reads the keychain item again before a refresh. Gone: signed out. A different refresh token
/// (another instance rotated the chain): adopted into memory. While a rotated token is held only
/// in memory (`persist_failed`), memory is the newer copy and is kept. A keychain that cannot
/// be read leaves memory as it is.
fn adopt_stored(
    handle: &ConnectorHandle,
    slot: &mut Slot,
    current: OAuthTokens,
) -> Result<Adopted, AuthError> {
    if handle.inner.persist_failed.load(Ordering::Relaxed) {
        return Ok(Adopted::Refresh(current));
    }
    let stored = match handle.read_stored() {
        Ok(stored) => stored,
        Err(err) => {
            tracing::warn!(connector = handle.id(), error = %err, "the keychain item could not be read again before the refresh");
            return Ok(Adopted::Refresh(current));
        }
    };
    match stored {
        None => {
            tracing::info!(
                connector = handle.id(),
                "the keychain item is gone; signed out"
            );
            handle.end_credential(slot, Ended::SignedOut);
            Err(AuthError::SignedOut)
        }
        Some(Credential::OAuth(stored)) if stored.refresh_token != current.refresh_token => {
            tracing::info!(
                connector = handle.id(),
                "another instance refreshed; its tokens are adopted"
            );
            *slot = Slot::Read(Some(Credential::OAuth(stored.clone())));
            let horizon = handle
                .inner
                .shared
                .unix_now()
                .saturating_add(REFRESH_MARGIN.as_secs());
            if stored.access_expires_at > horizon {
                Ok(Adopted::Fresh(stored.access_token))
            } else {
                Ok(Adopted::Refresh(stored))
            }
        }
        Some(Credential::ApiKey(key)) => {
            tracing::info!(
                connector = handle.id(),
                "another instance stored an API key; adopted"
            );
            *slot = Slot::Read(Some(Credential::ApiKey(key.clone())));
            handle.inner.generation.fetch_add(1, Ordering::SeqCst);
            Ok(Adopted::Fresh(key))
        }
        Some(Credential::OAuth(_)) => Ok(Adopted::Refresh(current)),
    }
}

/// Writes refreshed tokens to the keychain, retrying once; a second failure sets
/// `persist_failed` and the tokens stay in memory only.
fn persist(handle: &ConnectorHandle, tokens: &OAuthTokens) {
    let credential = Credential::OAuth(tokens.clone());
    let store = &handle.inner.shared.store;
    let mut last = None;
    for _ in 0..2 {
        let written = credential
            .encode()
            .and_then(|bytes| store.set(handle.id(), &bytes));
        match written {
            Ok(()) => {
                handle.inner.persist_failed.store(false, Ordering::Relaxed);
                return;
            }
            Err(err) => last = Some(err),
        }
    }
    if let Some(err) = last {
        tracing::warn!(connector = handle.id(), error = %err, "the refreshed tokens could not be written to the keychain; held in memory");
    }
    handle.inner.persist_failed.store(true, Ordering::Relaxed);
}

/// `POST /revoke` for the refresh token; whether the connector answered 200.
pub(crate) fn revoke(handle: &ConnectorHandle, refresh_token: &Secret) -> bool {
    let metadata = match crate::oauth::metadata(handle) {
        Ok(metadata) => metadata,
        Err(err) => {
            tracing::info!(connector = handle.id(), error = %err, "no revocation endpoint");
            return false;
        }
    };
    let form = encode_form(&[
        ("token", refresh_token.expose()),
        ("token_type_hint", "refresh_token"),
        ("client_id", &handle.inner.urls.client_id),
    ]);
    let shared = &handle.inner.shared;
    match http::send(
        &shared.agent,
        "POST",
        metadata.revocation.as_str(),
        &[],
        SendBody::Form(&form),
        shared.timeout(),
    ) {
        Ok(reply) if reply.status == 200 => true,
        Ok(reply) => {
            tracing::info!(
                connector = handle.id(),
                status = reply.status,
                "the revoke was refused"
            );
            false
        }
        Err(err) => {
            tracing::info!(connector = handle.id(), error = %err, "the revoke failed");
            false
        }
    }
}
