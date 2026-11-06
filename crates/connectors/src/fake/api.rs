//! The fake's MCP endpoint, JSON API and `pricing.json`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::PoisonError;
use std::time::Duration;

use serde_json::{Value, json};

use super::server::Response;
use super::{Fixture, RecordedRequest, Shared, fixtures};

/// The paid tools and the `prices` entry each spends.
const PAID_TOOLS: [(&str, &str); 4] = [
    ("search_law", "search_page"),
    ("read_document", "open"),
    ("citator", "details"),
    ("check_citations", "citation_check"),
];

/// The balance, the pricing answer, queued tool results and the recharges.
pub(super) struct ApiState {
    pub(super) balance: i64,
    pub(super) pricing: Fixture,
    pub(super) tool_results: HashMap<String, VecDeque<Fixture>>,
    pub(super) sse: bool,
    pub(super) wait_cap: Duration,
    /// A pay link to answer instead of the fake's own, for the client's refusals.
    pub(super) pay_url: Option<String>,
    /// An out-of-credit answer opens a new recharge instead of naming the open one.
    pub(super) fresh_recharge: bool,
    recharges: BTreeMap<String, FakeRecharge>,
    next_recharge: u64,
}

struct FakeRecharge {
    /// Opening order.
    n: u64,
    credits: i64,
    amount: Value,
    paid: bool,
    /// Ended unpaid, with this status (`expired`, `cancelled`).
    ended: Option<&'static str>,
}

impl FakeRecharge {
    /// Paid or ended: a `wait` poll answers at once.
    fn settled(&self) -> bool {
        self.paid || self.ended.is_some()
    }
}

impl ApiState {
    pub(super) fn new() -> ApiState {
        ApiState {
            balance: 1000,
            pricing: Fixture::File("pricing-live.json"),
            tool_results: HashMap::new(),
            sse: false,
            wait_cap: Duration::from_secs(2),
            pay_url: None,
            fresh_recharge: false,
            recharges: BTreeMap::new(),
            next_recharge: 1,
        }
    }

    pub(super) fn mark_paid(&mut self, id: &str) -> bool {
        let Some(recharge) = self.recharges.get_mut(id) else {
            return false;
        };
        if !recharge.paid {
            recharge.paid = true;
            self.balance = self.balance.saturating_add(recharge.credits);
        }
        true
    }

    /// Ends an unpaid recharge with `status`; false for an unknown or settled one.
    pub(super) fn end_recharge(&mut self, id: &str, status: &'static str) -> bool {
        match self.recharges.get_mut(id) {
            Some(recharge) if !recharge.settled() => {
                recharge.ended = Some(status);
                true
            }
            _ => false,
        }
    }

    pub(super) fn recharge_ids(&self) -> Vec<String> {
        let mut ids: Vec<(u64, String)> = self
            .recharges
            .iter()
            .map(|(id, recharge)| (recharge.n, id.clone()))
            .collect();
        ids.sort_unstable();
        ids.into_iter().map(|(_, id)| id).collect()
    }

    /// The packs of the pricing answer now (`pricing-live.json`'s when it has none).
    fn packs(&self, base: &str) -> Vec<Value> {
        let pricing = match &self.pricing {
            Fixture::File(name) => fixtures::value(name, base),
            Fixture::Json(value) => value.clone(),
            Fixture::Raw { .. } => Value::Null,
        };
        let packs = pricing.get("packs").and_then(Value::as_array).cloned();
        packs.unwrap_or_else(|| {
            fixtures::value("pricing-live.json", base)
                .get("packs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
    }

    /// Opens a recharge for `pack`; `None` for a pack not on sale.
    fn open(&mut self, base: &str, pack: &str) -> Option<String> {
        let found = self
            .packs(base)
            .into_iter()
            .find(|candidate| pack_name(candidate).as_deref() == Some(pack))?;
        let id = format!("rch_fake{:06}", self.next_recharge);
        let n = self.next_recharge;
        self.next_recharge = self.next_recharge.saturating_add(1);
        self.recharges.insert(
            id.clone(),
            FakeRecharge {
                n,
                credits: found.get("credits").and_then(Value::as_i64).unwrap_or(0),
                amount: json!({
                    "net_inr": found.get("net_inr").cloned().unwrap_or(Value::Null),
                    "gst_inr": found.get("gst_inr").cloned().unwrap_or(Value::Null),
                    "gross_inr": found.get("gross_inr").cloned().unwrap_or(Value::Null),
                }),
                paid: false,
                ended: None,
            },
        );
        Some(id)
    }

    /// The newest recharge awaiting payment.
    fn open_recharge(&self) -> Option<String> {
        self.recharge_ids().into_iter().rev().find(|id| {
            self.recharges
                .get(id)
                .is_some_and(|recharge| !recharge.settled())
        })
    }

    /// The open recharge (unless `fresh_recharge`), or a new one for the smallest pack.
    fn open_or_new(&mut self, base: &str) -> Option<String> {
        if let Some(id) = self.open_recharge().filter(|_| !self.fresh_recharge) {
            return Some(id);
        }
        let smallest = self.packs(base).first().and_then(pack_name)?;
        self.open(base, &smallest)
    }

    /// A recharge as the payment object of `recharge-created.json` (or `-paid`).
    fn recharge_json(&self, base: &str, id: &str) -> Value {
        let Some(recharge) = self.recharges.get(id) else {
            return Value::Null;
        };
        let file = if recharge.paid {
            "recharge-paid.json"
        } else {
            "recharge-created.json"
        };
        let mut value = fixtures::value(file, base);
        if let Some(status) = recharge.ended {
            value["status"] = json!(status);
        }
        value["recharge_id"] = json!(id);
        value["url"] = json!(
            self.pay_url
                .clone()
                .unwrap_or_else(|| format!("{base}/pay/{id}"))
        );
        value["amount"] = recharge.amount.clone();
        value["credits"] = json!(recharge.credits);
        value
    }

    /// A recharge as `GET /recharges/{id}` answers it: the `recharge` tool's `structuredContent`,
    /// in the connector's shapes (`recharge-get-*.json`). Paid has no `recharge_id` and no
    /// `payment`; expired's `payment` is the open (or a new) recharge, a fresh link.
    fn poll_json(&mut self, base: &str, id: &str) -> Value {
        let Some((paid, ended)) = self.recharges.get(id).map(|r| (r.paid, r.ended)) else {
            return Value::Null;
        };
        match (paid, ended) {
            (true, _) => {
                let mut value = fixtures::value("recharge-get-paid.json", base);
                value["balance_credits"] = json!(self.balance);
                value["balance_inr"] = json!(inr(self.balance));
                value
            }
            (false, None) => {
                let mut value = fixtures::value("recharge-get-awaiting.json", base);
                self.payment_now(base, id, &mut value);
                value
            }
            (false, Some("expired")) => {
                let mut value = fixtures::value("recharge-get-expired.json", base);
                match self.open_or_new(base) {
                    Some(fresh) => self.payment_now(base, &fresh, &mut value),
                    None => {
                        if let Some(object) = value.as_object_mut() {
                            object.remove("payment");
                            object.remove("next");
                        }
                    }
                }
                value
            }
            (false, Some(status)) => {
                let mut value = fixtures::value("recharge-get-held.json", base);
                value["status"] = json!(status);
                value
            }
        }
    }

    /// Fills a poll answer's `payment` and `next` with recharge `id`, the one to pay now.
    fn payment_now(&self, base: &str, id: &str, value: &mut Value) {
        let Some(recharge) = self.recharges.get(id) else {
            return;
        };
        let payment = &mut value["payment"];
        payment["recharge_id"] = json!(id);
        payment["url"] = json!(
            self.pay_url
                .clone()
                .unwrap_or_else(|| format!("{base}/pay/{id}"))
        );
        payment["amount"] = recharge.amount.clone();
        payment["credits"] = json!(recharge.credits);
        value["next"][0]["arguments"]["recharge_id"] = json!(id);
    }

    /// `account.json` with the balance and the open payment now.
    fn account_json(&self, base: &str) -> Value {
        let mut value = fixtures::value("account.json", base);
        value["balance_credits"] = json!(self.balance);
        value["balance_inr"] = json!(inr(self.balance));
        value["open_payment"] = self
            .open_recharge()
            .map_or(Value::Null, |id| self.recharge_json(base, &id));
        value
    }

    /// `api-402.json` with the open (or a new) recharge as its payment.
    fn out_of_credit(&mut self, base: &str) -> Value {
        let mut body = fixtures::value("api-402.json", base);
        if let Some(id) = self.open_or_new(base) {
            body["payment"] = self.recharge_json(base, &id);
            body["next"] =
                json!([{"tool": "recharge", "arguments": {"recharge_id": id, "wait": true}}]);
            body["message_for_human"] = json!(format!(
                "The case-law connector is out of credit. Recharge to continue: {base}/pay/{id}"
            ));
        }
        body
    }
}

/// Credits as rupees at 0.10 INR each, as the connector writes `balance_inr`.
fn inr(credits: i64) -> String {
    let paise = credits.saturating_mul(10);
    format!(
        "{}{}.{:02}",
        if paise < 0 { "-" } else { "" },
        (paise / 100).abs(),
        (paise % 100).abs()
    )
}

fn pack_name(pack: &Value) -> Option<String> {
    match pack.get("pack")? {
        Value::String(name) => Some(name.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// A `Fixture` as an answer.
fn fixture_response(fixture: &Fixture, base: &str) -> Response {
    match fixture {
        Fixture::File(name) => match fixtures::text(name, base) {
            Some(text) => Response {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: text.into_bytes(),
            },
            None => Response::text(500, "unknown fixture"),
        },
        Fixture::Json(value) => Response::json(200, value),
        Fixture::Raw {
            status,
            content_type,
            body,
        } => Response {
            status: *status,
            headers: vec![("Content-Type".to_owned(), content_type.clone())],
            body: body.clone(),
        },
    }
}

/// A 401 with the protected resource metadata's location (RFC 9728 section 5.1).
fn unauthorized(shared: &Shared) -> Response {
    Response::json(401, &json!({"error": "invalid_token"})).with(
        "WWW-Authenticate",
        &format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp/case-law\"",
            shared.base
        ),
    )
}

fn authorized(shared: &Shared, request: &RecordedRequest) -> bool {
    shared
        .lock()
        .oauth
        .bearer_ok(request.header("authorization"), shared.now())
}

/// `GET /pricing.json`.
pub(super) fn pricing(shared: &Shared) -> Response {
    let fixture = shared.lock().api.pricing.clone();
    fixture_response(&fixture, &shared.base)
}

/// `GET /v1/case-law/account`.
pub(super) fn account(shared: &Shared, request: &RecordedRequest) -> Response {
    if !authorized(shared, request) {
        return unauthorized(shared);
    }
    let mut state = shared.lock();
    if state.api.balance <= 0 {
        return Response::json(402, &state.api.out_of_credit(&shared.base));
    }
    Response::json(200, &state.api.account_json(&shared.base))
        .with("Cache-Control", "private, max-age=30")
}

/// `POST /v1/case-law/recharges` with `{"pack": "..."}`.
pub(super) fn create_recharge(shared: &Shared, request: &RecordedRequest) -> Response {
    if !authorized(shared, request) {
        return unauthorized(shared);
    }
    let mut state = shared.lock();
    if state.api.balance <= 0 {
        return Response::json(402, &state.api.out_of_credit(&shared.base));
    }
    let pack = request.json().and_then(|body| pack_name(&body));
    let Some(id) = pack.and_then(|pack| state.api.open(&shared.base, &pack)) else {
        return Response::json(400, &json!({"error": {"code": "unknown_pack"}}));
    };
    Response::json(200, &state.api.recharge_json(&shared.base, &id))
}

/// `GET /v1/case-law/recharges/{id}?wait=N`: holds the answer up to `N` seconds (capped) while
/// the recharge awaits payment (neither paid nor ended).
pub(super) fn get_recharge(shared: &Shared, request: &RecordedRequest, id: &str) -> Response {
    if !authorized(shared, request) {
        return unauthorized(shared);
    }
    let wait = request
        .query_pairs()
        .into_iter()
        .find(|(name, _)| name == "wait")
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .unwrap_or(0);
    let mut state = shared.lock();
    let Some(settled) = state.api.recharges.get(id).map(FakeRecharge::settled) else {
        return Response::json(404, &json!({"error": {"code": "not_found"}}));
    };
    if !settled && wait > 0 {
        let hold = Duration::from_secs(wait).min(state.api.wait_cap);
        state = shared
            .paid
            .wait_timeout_while(state, hold, |state| {
                !shared.stop.is_cancelled()
                    && !state
                        .api
                        .recharges
                        .get(id)
                        .is_some_and(FakeRecharge::settled)
            })
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    let body = state.api.poll_json(&shared.base, id);
    Response::json(200, &body)
}

/// `POST /mcp/case-law`: JSON-RPC, one message per request.
pub(super) fn mcp(shared: &Shared, request: &RecordedRequest) -> Response {
    let Some(message) = request.json().filter(Value::is_object) else {
        return Response::json(
            400,
            &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
        );
    };
    let Some(id) = message.get("id").cloned() else {
        // A notification or a response: accepted, no body.
        return Response::empty(202);
    };
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" | "server/discover" => {
            Ok(fixtures::value("mcp/initialize.json", &shared.base))
        }
        "tools/list" => Ok(fixtures::value("mcp/tools-list.json", &shared.base)),
        "ping" => Ok(json!({})),
        "tools/call" => {
            if !authorized(shared, request) {
                return unauthorized(shared);
            }
            match tool_call(shared, message.get("params").unwrap_or(&Value::Null)) {
                ToolAnswer::Result(result) => Ok(result),
                ToolAnswer::Http(response) => return response,
            }
        }
        _ => Err(json!({"code": -32601, "message": "method not found"})),
    };
    let reply = match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    };
    let sse = shared.lock().api.sse;
    let response = if sse {
        Response {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "text/event-stream".to_owned())],
            body: format!("event: message\ndata: {reply}\n\n").into_bytes(),
        }
    } else {
        Response::json(200, &reply)
    };
    if method == "initialize" {
        response.with("Mcp-Session-Id", "fake-session-0001")
    } else {
        response
    }
}

enum ToolAnswer {
    Result(Value),
    Http(Response),
}

/// A `tools/call`: a queued result first, then out of credit at a zero balance for a paid tool,
/// then the tool's fixture (spending its price).
fn tool_call(shared: &Shared, params: &Value) -> ToolAnswer {
    let base = &shared.base;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut state = shared.lock();
    let queued = state
        .api
        .tool_results
        .get_mut(name)
        .and_then(VecDeque::pop_front);
    if let Some(fixture) = queued {
        return match fixture {
            Fixture::File(file) => ToolAnswer::Result(fixtures::value(file, base)),
            Fixture::Json(value) => ToolAnswer::Result(value),
            raw @ Fixture::Raw { .. } => ToolAnswer::Http(fixture_response(&raw, base)),
        };
    }
    let price_key = PAID_TOOLS
        .iter()
        .find(|(tool, _)| *tool == name)
        .map(|(_, key)| *key);
    if let Some(key) = price_key {
        if state.api.balance <= 0 {
            let body = state.api.out_of_credit(base);
            let mut result = fixtures::value("mcp/insufficient-credits-result.json", base);
            result["content"] =
                json!([{"type": "text", "text": body["message_for_human"].clone()}]);
            result["structuredContent"] = body;
            return ToolAnswer::Result(result);
        }
        let price = fixtures::value("pricing-live.json", base)
            .pointer(&format!("/prices/{key}"))
            .and_then(Value::as_i64)
            .unwrap_or(1);
        state.api.balance = state.api.balance.saturating_sub(price).max(0);
    }
    let result = match name {
        "search_law" => fixtures::value("mcp/search-law-result.json", base),
        "read_document" => fixtures::value("mcp/read-document-result.json", base),
        "account" => {
            let account = state.api.account_json(base);
            json!({"content": [{"type": "text", "text": "The account."}], "structuredContent": account, "isError": false})
        }
        "recharge" => recharge_tool(&mut state.api, base, params.get("arguments")),
        "citator" | "check_citations" => {
            json!({"content": [{"type": "text", "text": "No findings."}], "structuredContent": {}, "isError": false})
        }
        _ => {
            json!({"content": [{"type": "text", "text": format!("Unknown tool: {name}")}], "isError": true})
        }
    };
    ToolAnswer::Result(result)
}

/// The `recharge` tool: `recharge_id` reads a recharge, `pack` opens one. (No `resume`.)
fn recharge_tool(api: &mut ApiState, base: &str, arguments: Option<&Value>) -> Value {
    let arguments = arguments.cloned().unwrap_or(Value::Null);
    let id = match arguments.get("recharge_id").and_then(Value::as_str) {
        Some(id) => Some(id.to_owned()),
        None => pack_name(&arguments).and_then(|pack| api.open(base, &pack)),
    };
    match id.filter(|id| api.recharges.contains_key(id)) {
        Some(id) => {
            // The connector's shape: `structuredContent` is the GET's answer, and the first
            // text block is that JSON.
            let status = api.poll_json(base, &id);
            json!({"content": [{"type": "text", "text": status.to_string()}], "structuredContent": status, "isError": false})
        }
        None => {
            json!({"content": [{"type": "text", "text": "No such recharge or pack."}], "isError": true})
        }
    }
}
