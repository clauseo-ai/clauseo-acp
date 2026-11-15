//! The glue of the two read routes, `/dash` and `/v1/query`: the Access check with the team's
//! keys (fetched, and kept an hour in the isolate), the dashboard's D1 batch, and a guarded
//! query raced against its 10 seconds. `glue.rs` reads the request and calls these; nothing
//! here sees the request.

use std::cell::RefCell;
use std::pin::pin;
use std::time::Duration;

use futures_util::future::{Either, select};
use serde::Deserialize;
use serde_json::Value;
use worker::{D1Database, Delay, Env, Fetch, Headers, Response, Url};

use crate::access::{Access, DEV_CERTS_VAR, KeyCache, Token, check, dev_certs_url, parse_keys};
use crate::dash::{CSP, Data, page, statements};
use crate::query::guard;
use crate::route::{Refusal, Route};
use crate::webcrypto::WebCrypto;

/// How long a query may run before the Worker answers `504`.
pub(crate) const QUERY_SECS: u64 = 10;

thread_local! {
    /// The team's keys, shared by the requests one isolate serves.
    static KEYS: RefCell<KeyCache> = RefCell::new(KeyCache::default());
}

fn var(env: &Env, name: &str) -> String {
    env.var(name)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

/// The route's Access settings, or `None` (and then `404`) when `ACCESS_TEAM` or its AUD is
/// unset or empty.
pub(crate) fn access(env: &Env, route: Route) -> Option<Access> {
    let aud = match route {
        Route::Dash => "DASH_AUD",
        Route::Query => "QUERY_AUD",
        Route::Counts | Route::Tally | Route::Crash => return None,
    };
    Access::new(&var(env, "ACCESS_TEAM"), &var(env, aud))
}

/// Refuses with [`Refusal::Forbidden`] unless `jwt` passes the route's Access check at `now`.
pub(crate) async fn authorize(
    env: &Env,
    access: &Access,
    jwt: Option<&str>,
    now: u64,
) -> Result<(), Refusal> {
    let jwt = jwt.ok_or(Refusal::Forbidden)?;
    let token = Token::parse(jwt).or(Err(Refusal::Forbidden))?;
    let dev = var(env, DEV_CERTS_VAR);
    let url = dev_certs_url(&dev).map_or_else(|| access.certs_url(), str::to_owned);
    if KEYS.with(|keys| keys.borrow().stale(&url, token.kid(), now)) {
        let keys = fetch_keys(&url).await?;
        KEYS.with(|cache| {
            *cache.borrow_mut() = KeyCache {
                url,
                fetched: now,
                keys,
            };
        });
    }
    let keys = KEYS.with(|cache| cache.borrow().keys.clone());
    check(Some(jwt), access, &keys, now, &WebCrypto)
        .await
        .or(Err(Refusal::Forbidden))
}

async fn fetch_keys(url: &str) -> Result<Vec<crate::access::Key>, Refusal> {
    let url = Url::parse(url).or(Err(Refusal::Failed))?;
    let mut response = Fetch::Url(url).send().await?;
    if response.status_code() != 200 {
        return Err(Refusal::Failed);
    }
    parse_keys(&response.bytes().await?).ok_or(Refusal::Failed)
}

/// The headers of every read answer: no caching, no sniffing, no referrer, no framing.
fn answer_fields(content_type: &str) -> Result<Headers, Refusal> {
    let fields = Headers::new();
    fields.set("Content-Type", content_type)?;
    fields.set("Cache-Control", "no-store")?;
    fields.set("X-Content-Type-Options", "nosniff")?;
    fields.set("Referrer-Policy", "no-referrer")?;
    fields.set("X-Frame-Options", "DENY")?;
    Ok(fields)
}

/// The dashboard: the six statements of `dash.sql` in one batch, rendered.
pub(crate) async fn dash(db: &D1Database) -> Result<Response, Refusal> {
    let prepared = statements()
        .iter()
        .map(|sql| db.prepare(sql))
        .collect::<Vec<_>>();
    let mut results = db.batch(prepared).await?.into_iter();
    let mut next = || -> Result<Vec<Value>, Refusal> {
        Ok(results.next().ok_or(Refusal::Failed)?.results::<Value>()?)
    };
    let data = Data {
        active: next()?,
        cohorts: next()?,
        daily: next()?,
        tally: next()?,
        unfilled: next()?,
        cron: next()?,
    };
    let fields = answer_fields("text/html; charset=utf-8")?;
    fields.set("Content-Security-Policy", CSP)?;
    Ok(Response::ok(page(&data))?.with_headers(fields))
}

/// `/v1/query`'s body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    sql: String,
}

/// One guarded query: its rows as a JSON array, at most 1,000. A body or query the guard
/// refuses, or one D1 cannot run, is `400`; past 10 seconds, `504`.
pub(crate) async fn query(db: &D1Database, body: &[u8]) -> Result<Response, Refusal> {
    let ask: Ask = serde_json::from_slice(body).or(Err(Refusal::Bad))?;
    let sql = guard(&ask.sql).or(Err(Refusal::Bad))?;
    let run = pin!(async { db.prepare(&sql).all().await });
    let timer = pin!(Delay::from(Duration::from_secs(QUERY_SECS)));
    let rows: Vec<Value> = match select(run, timer).await {
        Either::Left((result, _)) => result
            .and_then(|result| result.results::<Value>())
            .or(Err(Refusal::Bad))?,
        Either::Right(_) => return Err(Refusal::Timeout),
    };
    let body = serde_json::to_string(&rows)?;
    Ok(Response::ok(body)?.with_headers(answer_fields("application/json")?))
}
