//! The fake's held calls, in the connector's shapes: an out-of-credit tool answer holds the call
//! (`pending_call`, `pc_…`), says what to do once paid (`then`) and names it in `next`'s
//! `resume`; a `recharge` call on that recharge, paid, with `resume` set to that id runs it
//! once, spending its price, and answers with `structuredContent.resumed`: `{tool,
//! in_content_blocks: true, result, text, charge_lines, balance_credits, charged_credits}`, the
//! call's own blocks after the status. The top-level `balance_credits` stays the balance right
//! after the payment, so `resumed.balance_credits` is it less `resumed.charged_credits`.

use serde_json::{Value, json};

use super::{ApiState, paid_result, price};

/// `then` of an out-of-credit answer that holds its call.
const THEN: &str = "Once the human has paid, call recharge as next says: it runs this call and returns its result.";

/// A resumed call's answer, to go with the recharge's status.
pub(super) struct Resumed {
    /// `structuredContent.resumed`.
    resumed: Value,
    /// The call's own `content` blocks.
    blocks: Vec<Value>,
}

impl Resumed {
    /// The `recharge` tool's answer: `status` with `resumed`, as JSON text and then the
    /// resumed call's blocks.
    pub(super) fn answer(self, mut status: Value) -> Value {
        status["resumed"] = self.resumed;
        let mut content = vec![json!({"type": "text", "text": status.to_string()})];
        content.extend(self.blocks);
        json!({"content": content, "structuredContent": status, "isError": false})
    }
}

/// Holds `tool`'s call on the recharge an out-of-credit `body` names: a new `pc_…` id in
/// `pending_call` and in `next`'s `resume`, and `then`.
pub(super) fn hold(api: &mut ApiState, mut body: Value, tool: &str) -> Value {
    let Some(id) = body
        .pointer("/payment/recharge_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return body;
    };
    let Some(recharge) = api.recharges.get_mut(&id) else {
        return body;
    };
    let pending = format!("pc_fake{:06}", api.next_pending);
    api.next_pending = api.next_pending.saturating_add(1);
    recharge.pending = Some((pending.clone(), tool.to_owned()));
    body["pending_call"] = json!(pending);
    body["then"] = json!(THEN);
    body["next"] = json!([{"tool": "recharge",
        "arguments": {"recharge_id": id, "resume": pending, "wait": true}}]);
    body
}

/// Runs the call recharge `id` holds when it is paid and `pending` names it: once, spending its
/// price. `None` otherwise (unpaid, another id, or already run).
pub(super) fn run(api: &mut ApiState, base: &str, id: &str, pending: &str) -> Option<Resumed> {
    let recharge = api.recharges.get_mut(id).filter(|recharge| recharge.paid)?;
    if recharge.pending.as_ref().map(|(held, _)| held.as_str()) != Some(pending) {
        return None;
    }
    let (_, tool) = recharge.pending.take()?;
    let price = price(base, &tool).unwrap_or(0);
    api.balance = api.balance.saturating_sub(price).max(0);
    let answer = paid_result(base, &tool);
    let blocks = answer
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let line = format!(
        "{tool}: Charged {price} credits · balance {}",
        thousands(api.balance)
    );
    let resumed = json!({"tool": tool, "in_content_blocks": true,
        "result": answer.get("structuredContent").cloned().unwrap_or(Value::Null),
        "text": text, "charge_lines": [line], "balance_credits": api.balance,
        "charged_credits": price});
    Some(Resumed { resumed, blocks })
}

/// `n` with thousands commas, as the connector writes a balance: `1,960`.
fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if n < 0 { format!("-{out}") } else { out }
}
