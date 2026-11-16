//! What a case-law tool result says it cost: `result._meta["chat.clauseo/credits"]`, the Indian
//! Kanoon server's `{"charged_credits": <int ≥ 0>, "balance_credits": <int>}` on the
//! `CallToolResult` itself (a sibling of `content`, `structuredContent` and `isError`). The
//! server puts it on every result of `search_law`, `read_document`, `citator`,
//! `check_citations` and `account` for a known account, an error included; never on an
//! unauthenticated or protocol error. An older server has no such key.
//!
//! Only those two integers are read, borrowed, and nothing else of the result; the reply is
//! forwarded as it came either way. Each is read on its own and must be a JSON integer
//! (`3`, never `3.0`, `"3"` or `3e0`): `charged_credits` from 0 to `u32::MAX`, `balance_credits`
//! any `i64`. Anything else counts as absent, so a changed shape never shows a wrong number.
//! Nothing here logs: a result holds matter content.

use serde::Deserialize;
use serde_json::value::RawValue;

use super::reply::{object, present};

/// The `_meta` key the credits sit under.
#[cfg(test)]
pub(super) const CREDITS_KEY: &str = "chat.clauseo/credits";

/// What a result's `_meta` credits say; [`credits`] returns `None` when the key is missing or
/// not an object that reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Credits {
    /// `charged_credits`, when a whole number 0 or more.
    pub(super) charged: Option<u32>,
    /// `balance_credits`, when a whole number.
    pub(super) balance: Option<i64>,
}

#[derive(Deserialize)]
struct WithMeta<'a> {
    #[serde(rename = "_meta", borrow, default, deserialize_with = "present")]
    meta: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Meta<'a> {
    #[serde(
        rename = "chat.clauseo/credits",
        borrow,
        default,
        deserialize_with = "present"
    )]
    credits: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Raw<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    charged_credits: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    balance_credits: Option<&'a RawValue>,
}

/// The raw `tools/call` result's credits; `None` when `_meta` or its credits key is missing or
/// does not read as an object (a duplicate key, say).
pub(super) fn credits(result: &RawValue) -> Option<Credits> {
    let meta = object::<WithMeta<'_>>(result)?.meta?;
    let raw = object::<Meta<'_>>(meta)?.credits?;
    let read = object::<Raw<'_>>(raw)?;
    Some(Credits {
        charged: read.charged_credits.and_then(whole::<u32>),
        balance: read.balance_credits.and_then(whole::<i64>),
    })
}

/// A raw JSON integer as `T`; `None` for a float, a string, `null` or one out of range. The
/// check on the text keeps `1.0` and `1e3` out, which `serde_json` may read as integers.
fn whole<T: for<'de> Deserialize<'de>>(raw: &RawValue) -> Option<T> {
    let text = raw.get();
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests;
