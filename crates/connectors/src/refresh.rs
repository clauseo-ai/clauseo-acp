//! `bearer` and the refresh: one refresh at a time per connector (the credential slot's lock is
//! held while it runs), 5 minutes before expiry or after a 401, so rotation never forks a chain.
//! A rotated refresh token is written to the keychain before the new access token is used.

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
        Credential::OAuth(_) => refresh_locked(handle, slot),
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
        Credential::OAuth(_) => refresh_locked(handle, &mut slot),
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

/// Refreshes the access token; the slot is locked by the caller. `invalid_grant` ends the
/// credential; a network error or any other answer leaves it as it is.
fn refresh_locked(handle: &ConnectorHandle, slot: &mut Slot) -> Result<Secret, AuthError> {
    let old_refresh = match handle.current(slot)? {
        Credential::OAuth(tokens) => tokens.refresh_token.clone(),
        Credential::ApiKey(key) => return Ok(key.clone()),
    };
    let metadata = crate::oauth::metadata(handle)
        .map_err(|err| AuthError::Network(format!("discovery: {err}")))?;
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
    let reply = http::send(
        &shared.agent,
        "POST",
        metadata.token.as_str(),
        &[("Accept", "application/json")],
        SendBody::Form(&form),
        shared.timeout(),
    )
    .map_err(AuthError::Network)?;
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
        return Err(AuthError::Network(format!(
            "the token endpoint answered HTTP {} ({code})",
            reply.status
        )));
    }
    let tokens = parse_token_answer(&reply.body, shared.unix_now(), Some(&old_refresh))
        .map_err(AuthError::Network)?;
    let access = tokens.access_token.clone();
    persist(handle, &tokens);
    *slot = Slot::Read(Some(Credential::OAuth(tokens)));
    Ok(access)
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
