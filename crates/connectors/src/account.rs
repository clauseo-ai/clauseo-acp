//! The connector account: `GET /v1/case-law/account?recent_days=30`.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::Value;

use crate::config::Origin;
use crate::de;
use crate::error::{Error, parse_error};
use crate::known::KnownConnector;
use crate::recharge::{Recharge, recharge_from_value};

/// The account's balance, recent spend and open payment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "the connector's field names, fixed by the brief's API"
)]
pub struct Account {
    /// The connector's account id.
    pub account_id: String,
    /// The account's state, as the connector writes it (`active`, ...).
    pub state: String,
    /// Credits left.
    pub balance_credits: i64,
    /// Rupees left, as written.
    pub balance_inr: String,
    /// Credits spent today.
    pub spent_today_credits: i64,
    /// Credits spent per day, newest as the connector orders them.
    pub recent: Vec<DaySpend>,
    /// A recharge awaiting payment, if any. Its pay link is `None` when it is not ours.
    pub open_payment: Option<Recharge>,
    /// Whether the account is claimed.
    pub claimed: bool,
    /// Credits per action.
    pub prices: BTreeMap<String, i64>,
    /// When this copy was fetched.
    pub fetched_at: SystemTime,
}

/// Credits spent on one day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct DaySpend {
    /// `YYYY-MM-DD`.
    #[serde(default, deserialize_with = "de::text")]
    pub day: String,
    /// Credits spent.
    #[serde(default, deserialize_with = "de::int")]
    pub credits: i64,
}

#[derive(Deserialize)]
struct AccountIn {
    #[serde(default, deserialize_with = "de::text")]
    account_id: String,
    #[serde(default, deserialize_with = "de::text")]
    state: String,
    #[serde(default, deserialize_with = "de::int")]
    balance_credits: i64,
    #[serde(default, deserialize_with = "de::text")]
    balance_inr: String,
    #[serde(default, deserialize_with = "de::int")]
    spent_today_credits: i64,
    #[serde(default, deserialize_with = "de::list")]
    recent: Vec<DaySpend>,
    #[serde(default)]
    open_payment: Value,
    #[serde(default, deserialize_with = "de::flag")]
    claimed: bool,
    #[serde(default, deserialize_with = "de::prices")]
    prices: BTreeMap<String, i64>,
}

/// Parses the account answer, fetched at `fetched_at`.
pub(crate) fn parse_account(
    bytes: &[u8],
    connector: &KnownConnector,
    origin: &Origin,
    fetched_at: SystemTime,
) -> Result<Account, Error> {
    let value: Value = serde_json::from_slice(bytes).map_err(|err| parse_error("account", &err))?;
    if !value.is_object() {
        return Err(Error::Parse("account: not an object".to_owned()));
    }
    let wire: AccountIn =
        serde_json::from_value(value).map_err(|err| parse_error("account", &err))?;
    let open_payment = if wire.open_payment.is_object() {
        recharge_from_value(&wire.open_payment, connector, origin, false).ok()
    } else {
        None
    };
    Ok(Account {
        account_id: wire.account_id,
        state: wire.state,
        balance_credits: wire.balance_credits,
        balance_inr: wire.balance_inr,
        spent_today_credits: wire.spent_today_credits,
        recent: wire.recent,
        open_payment,
        claimed: wire.claimed,
        prices: wire.prices,
        fetched_at,
    })
}

#[cfg(test)]
mod tests;
