//! The Worker's two handlers, the only code that touches the request, D1 and the clock. A request
//! goes through the steps in this order, and stops at the first that refuses it: method and
//! path, the two headers, a read capped at the gzip limit, the inflate, the crate's parser, the
//! daily count, then the rows. Nothing before the daily count touches D1, so a refused request
//! never does.
//!
//! This is the only file that touches the request; `tests/source.test.mjs` holds it to the one
//! `header` function below, with its two names.

use futures_util::StreamExt;
use worker::wasm_bindgen::JsValue;
use worker::{
    Context, D1Database, D1PreparedStatement, Date, Env, Method, Request, Response,
    ScheduleContext, ScheduledEvent, event,
};

use clauseo_counts::schema::Time;

use crate::body::{Limited, check_headers, inflate};
use crate::retention::Pruner;
use crate::route::{Refusal, Route};
use crate::rows::{Accepted, BUMP_RATE, Param, accept};

/// The D1 binding in `wrangler.toml`.
const DB: &str = "DB";

/// The only request headers the Worker reads (the source test holds it to these two).
#[derive(Debug, Clone, Copy)]
enum RequestHeader {
    /// Must be `gzip`.
    ContentEncoding,
    /// At most the route's gzip cap, when given.
    ContentLength,
}

impl RequestHeader {
    const fn name(self) -> &'static str {
        match self {
            RequestHeader::ContentEncoding => "Content-Encoding",
            RequestHeader::ContentLength => "Content-Length",
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
    let status = match receive(&mut request, &env).await {
        Ok(()) => 204,
        Err(refusal) => refusal.status(),
    };
    let response = Response::empty()?.with_status(status);
    if status == Refusal::Method.status() {
        let allow = worker::Headers::new();
        allow.set("Allow", "POST")?;
        return Ok(response.with_headers(allow));
    }
    Ok(response)
}

/// Every step of a request, in order; `Ok` once its rows are stored.
async fn receive(request: &mut Request, env: &Env) -> Result<(), Refusal> {
    let route = Route::find(request.method() == Method::Post, &request.path())?;
    let encoding = header(request, RequestHeader::ContentEncoding)?;
    let length = header(request, RequestHeader::ContentLength)?;
    check_headers(encoding.as_deref(), length.as_deref(), route.gzip_cap())?;
    let gzip = read(request, route.gzip_cap()).await?;
    let json = inflate(&gzip, route.json_cap())?;
    let now = now();
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

/// The daily deletes ([`Pruner`]). A failed run is not retried or recorded (the Worker keeps
/// no logs); the next day's run deletes the same rows and more.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _context: ScheduleContext) {
    prune(&env).await.unwrap_or_default();
}

/// A deleted row's key, the one column each chunked delete returns.
#[derive(serde::Deserialize)]
struct Key {
    #[serde(alias = "install", alias = "tally")]
    key: String,
}

async fn prune(env: &Env) -> Result<(), Refusal> {
    let db = env.d1(DB)?;
    let mut pruner = Pruner::new(now()).ok_or(Refusal::Failed)?;
    while let Some((sql, values)) = pruner.next() {
        let keys: Vec<Key> = statement(&db, sql, &values)?.all().await?.results()?;
        let keys: Vec<String> = keys.into_iter().map(|row| row.key).collect();
        pruner.record(&keys);
    }
    Ok(())
}
