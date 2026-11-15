//! The Worker's two handlers, the only code that touches the request and the clock. A storing
//! request goes through the steps in this order, and stops at the first that refuses it: path
//! and method, the two body headers, a read capped at the gzip limit, the inflate, the crate's
//! parser, the daily count, then the rows. Nothing before the daily count touches D1, so a
//! refused request never does. A crash report has the hourly count instead, then its R2 object
//! and its row. A read route (`/dash`, `/v1/query`) is `404` unless its Access settings are
//! set, then needs a valid Access token before anything else (`read.rs`).
//!
//! This is the only file that touches the request; `tests/source.test.mjs` holds it to the one
//! `header` function below, with its three names.

use futures_util::StreamExt;
use worker::wasm_bindgen::JsValue;
use worker::{
    Conditional, Context, D1Database, D1PreparedStatement, Date, Env, Request, Response,
    ScheduleContext, ScheduledEvent, event,
};

use clauseo_counts::schema::Time;

use crate::body::{Limited, check_headers, check_plain, inflate};
use crate::crash::{self, BUCKET, BUMP_CRASH_RATE, DELETE_PAST_HOURS, HOURLY_CAP, INSERT_CRASH};
use crate::read;
use crate::retention::{RECORD_STATUS, Step, plan, status};
use crate::route::{Refusal, Route};
use crate::rows::{Accepted, BUMP_RATE, Param, accept};

/// The D1 binding in `wrangler.toml`.
const DB: &str = "DB";

/// The only request headers the Worker reads (the source test holds it to these three).
#[derive(Debug, Clone, Copy)]
enum RequestHeader {
    /// Must be `gzip` (counts), or absent (`/v1/query`).
    ContentEncoding,
    /// At most the route's body cap, when given.
    ContentLength,
    /// Cloudflare Access's JWT, on the read routes only.
    AccessJwt,
}

impl RequestHeader {
    const fn name(self) -> &'static str {
        match self {
            RequestHeader::ContentEncoding => "Content-Encoding",
            RequestHeader::ContentLength => "Content-Length",
            RequestHeader::AccessJwt => "Cf-Access-Jwt-Assertion",
        }
    }
}

/// The one place a request header is read.
fn header(request: &Request, which: RequestHeader) -> Result<Option<String>, Refusal> {
    Ok(request.headers().get(which.name())?)
}

/// The Worker's clock, in whole seconds.
fn now() -> Time {
    Time::from_unix_secs(Date::now().as_millis() / 1_000)
}

#[event(fetch)]
async fn fetch(mut request: Request, env: Env, _context: Context) -> worker::Result<Response> {
    let route = Route::at(&request.path());
    let outcome = match route {
        None => Err(Refusal::NotFound),
        Some(route) if route.reads() => serve(route, &mut request, &env).await,
        Some(route) => receive(route, &mut request, &env).await.map(|()| None),
    };
    let status = match outcome {
        Ok(Some(response)) => return Ok(response),
        Ok(None) => 204,
        Err(refusal) => refusal.status(),
    };
    let response = Response::empty()?.with_status(status);
    if let (Some(route), true) = (route, status == Refusal::Method.status()) {
        let allow = worker::Headers::new();
        allow.set("Allow", route.method())?;
        return Ok(response.with_headers(allow));
    }
    Ok(response)
}

/// A read route: `404` without its Access settings, then the method, the Access token, and
/// the dashboard or the query.
async fn serve(
    route: Route,
    request: &mut Request,
    env: &Env,
) -> Result<Option<Response>, Refusal> {
    let access = read::access(env, route).ok_or(Refusal::NotFound)?;
    route.allows(request.method().as_ref())?;
    let jwt = header(request, RequestHeader::AccessJwt)?;
    read::authorize(env, &access, jwt.as_deref(), now().unix_secs()).await?;
    let db = env.d1(DB)?;
    let response = if route == Route::Query {
        let encoding = header(request, RequestHeader::ContentEncoding)?;
        let length = header(request, RequestHeader::ContentLength)?;
        check_plain(encoding.as_deref(), length.as_deref(), route.gzip_cap())?;
        let body = read(request, route.gzip_cap()).await?;
        read::query(&db, &body).await?
    } else {
        read::dash(&db).await?
    };
    Ok(Some(response))
}

/// Every step of a counts request, in order; `Ok` once its rows are stored.
async fn receive(route: Route, request: &mut Request, env: &Env) -> Result<(), Refusal> {
    route.allows(request.method().as_ref())?;
    let encoding = header(request, RequestHeader::ContentEncoding)?;
    let length = header(request, RequestHeader::ContentLength)?;
    check_headers(encoding.as_deref(), length.as_deref(), route.gzip_cap())?;
    let gzip = read(request, route.gzip_cap()).await?;
    let json = inflate(&gzip, route.json_cap())?;
    let now = now();
    if route == Route::Crash {
        return store_crash(env, &json, now).await;
    }
    let accepted = accept(route, &json, now)?;
    let db = env.d1(DB)?;
    bump(&db, route, now).await?;
    store(&db, accepted).await
}

/// The body, refused as soon as it passes `cap`. A request with no body is refused like an
/// empty one.
async fn read(request: &mut Request, cap: usize) -> Result<Vec<u8>, Refusal> {
    let mut body = Limited::new(cap);
    let Ok(mut stream) = request.stream() else {
        return Err(Refusal::Bad);
    };
    while let Some(chunk) = stream.next().await {
        body.push(&chunk?)?;
    }
    Ok(body.into_bytes())
}

/// Adds this request to the route's count for the UTC day, or refuses it with
/// [`Refusal::Busy`] when the count is at the cap, which then stays as it was.
async fn bump(db: &D1Database, route: Route, now: Time) -> Result<(), Refusal> {
    let today = now.day().ok_or(Refusal::Failed)?;
    let cap = i32::try_from(route.daily_cap())?;
    let values = [
        Param::Text(route.name().to_owned()),
        Param::Text(today.to_string()),
        Param::Int(cap),
    ];
    let count: Option<i64> = statement(db, BUMP_RATE, &values)?.first(Some("n")).await?;
    count.map(drop).ok_or(Refusal::Busy)
}

/// A crash report past the parser: `503` while the bucket is unbound, then the hour's count
/// (`429` at the cap; the first report of an hour deletes the past hours' counts), then the
/// object, then the row. The object is put only if no object has its key, so a report sent
/// again leaves the first object, as `INSERT OR IGNORE` leaves the first row. The object goes
/// first: if the row fails, the sender retries, the put is skipped and the row is written.
async fn store_crash(env: &Env, json: &[u8], now: Time) -> Result<(), Refusal> {
    let upload = crash::accept(json, now)?;
    let Ok(bucket) = env.bucket(BUCKET) else {
        return Err(Refusal::Unavailable);
    };
    let db = env.d1(DB)?;
    let hour = crash::hour(now).ok_or(Refusal::Failed)?;
    let values = [Param::Text(hour.clone()), Param::Int(HOURLY_CAP)];
    let count: Option<i64> = statement(&db, BUMP_CRASH_RATE, &values)?
        .first(Some("n"))
        .await?;
    if count.ok_or(Refusal::Busy)? == 1 {
        statement(&db, DELETE_PAST_HOURS, &[Param::Text(hour)])?
            .run()
            .await?;
    }
    // R2 answers no object (not an error) when the condition fails, i.e. the key exists.
    let absent = Conditional {
        etag_does_not_match: Some("*".to_owned()),
        ..Conditional::default()
    };
    bucket
        .put(upload.key, upload.body)
        .only_if(absent)
        .execute()
        .await?;
    statement(&db, INSERT_CRASH, &upload.row)?.run().await?;
    Ok(())
}

/// Inserts every row in one batch, which D1 runs as one transaction.
async fn store(db: &D1Database, accepted: Accepted) -> Result<(), Refusal> {
    let statements = accepted
        .rows
        .iter()
        .map(|row| statement(db, accepted.insert, row))
        .collect::<Result<Vec<_>, _>>()?;
    db.batch(statements).await?;
    Ok(())
}

fn statement(db: &D1Database, sql: &str, values: &[Param]) -> Result<D1PreparedStatement, Refusal> {
    let values: Vec<JsValue> = values
        .iter()
        .map(|value| match value {
            Param::Text(text) => JsValue::from_str(text),
            Param::Int(number) => JsValue::from_f64(f64::from(*number)),
            Param::Null => JsValue::null(),
        })
        .collect();
    Ok(db.prepare(sql).bind(&values)?)
}

/// The daily run ([`plan`]): the deletes, then the fills, one statement at a time, then the
/// run's health row in `cron_status` (each step finished or failed), which `/dash` shows. A
/// statement that fails does not stop the ones after it, and each finished range stays
/// deleted. A failed run is not retried; the next day's run sends the same statements, and a
/// fill that already happened inserts nothing.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _context: ScheduleContext) {
    daily(&env).await.unwrap_or_default();
}

async fn daily(env: &Env) -> Result<(), Refusal> {
    let db = env.d1(DB)?;
    let now = now();
    let mut outcomes = Vec::new();
    for (sql, values) in plan(now).ok_or(Refusal::Failed)? {
        let ran = match statement(&db, sql, &values) {
            Ok(prepared) => prepared.run().await.is_ok(),
            Err(_) => false,
        };
        outcomes.push((Step::of(sql), ran));
    }
    let values = status(now, &outcomes).ok_or(Refusal::Failed)?;
    statement(&db, RECORD_STATUS, &values)?.run().await?;
    Ok(())
}
