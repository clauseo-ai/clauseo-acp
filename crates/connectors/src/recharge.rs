//! Recharges and pay links: [`Recharge`], [`PaymentRef`] and [`PayUrl`], the one way a pay link
//! leaves the crate.

use serde::{Deserialize, Deserializer};
use serde_json::Value;
#[cfg(feature = "test-support")]
use url::Position;
use url::Url;

use crate::config::Origin;
use crate::de;
use crate::error::{Error, parse_error};
use crate::known::KnownConnector;
#[cfg(feature = "test-support")]
use crate::known::PRODUCTION_ORIGIN;

/// A recharge: a payment the connector opened for a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "the connector's field names, fixed by the brief's API"
)]
pub struct Recharge {
    /// `rch_...`.
    pub recharge_id: String,
    /// Whether it is paid.
    pub status: RechargeStatus,
    /// Where the lawyer pays; `None` when the connector sent none, or (outside
    /// `create_recharge`) one that is not ours.
    pub url: Option<PayUrl>,
    /// What it costs.
    pub amount: Option<Amount>,
    /// Credits it adds.
    pub credits: Option<i64>,
    /// When the payment link expires, as the connector writes it.
    pub expires_at: Option<String>,
}

/// A recharge's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RechargeStatus {
    /// `awaiting_payment`.
    AwaitingPayment,
    /// `paid`.
    Paid,
    /// Anything else, as written (expired, cancelled, ...).
    Other(String),
}

impl Default for RechargeStatus {
    fn default() -> RechargeStatus {
        RechargeStatus::Other(String::new())
    }
}

impl<'de> Deserialize<'de> for RechargeStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RechargeStatus, D::Error> {
        Ok(match de::text(deserializer)?.as_str() {
            "awaiting_payment" => RechargeStatus::AwaitingPayment,
            "paid" => RechargeStatus::Paid,
            other => RechargeStatus::Other(other.to_owned()),
        })
    }
}

/// Rupee amounts, as the connector writes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "the connector's field names, fixed by the brief's API"
)]
pub struct Amount {
    /// Before GST.
    #[serde(default, deserialize_with = "de::text")]
    pub net_inr: String,
    /// GST.
    #[serde(default, deserialize_with = "de::text")]
    pub gst_inr: String,
    /// With GST.
    #[serde(default, deserialize_with = "de::text")]
    pub gross_inr: String,
}

/// The payment an out-of-credit answer points to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaymentRef {
    /// `payment.recharge_id`.
    pub recharge_id: Option<String>,
    /// `payment.url`, when it is ours.
    pub url: Option<PayUrl>,
}

/// A pay link that is the connector's own: `https://data.clauseo.chat/pay/<id>`, with no port,
/// user info, query or fragment. Built only by [`PayUrl::parse`] and [`PayUrl::parse_for`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PayUrl(String);

impl PayUrl {
    /// A production pay link, or `None`: `https`, host exactly `data.clauseo.chat` (no port,
    /// not even 443, and no user info), path exactly `/pay/<id>` with `<id>` matching
    /// `[A-Za-z0-9_-]{1,128}`, no query, no fragment. A backslash or a percent escape anywhere
    /// is refused, so nothing the URL parser would rewrite gets through.
    pub fn parse(connector: &KnownConnector, text: &str) -> Option<PayUrl> {
        PayUrl::parse_for(connector, &Origin::Production, text)
    }

    /// [`PayUrl::parse`] on `origin`'s host. Under `Staging`, the staging host's `/pay/`, kept as
    /// is. Under `Fake`, the fake's own `http://127.0.0.1:<port>/pay/`, returned rewritten to
    /// `https://data.clauseo.chat/pay/...`, so R24's tests see the production form.
    pub fn parse_for(connector: &KnownConnector, origin: &Origin, text: &str) -> Option<PayUrl> {
        if text.contains(['\\', '%']) {
            return None;
        }
        let url = Url::parse(text).ok()?;
        let prefix = Url::parse(&origin.map_url(connector.pay_prefix)).ok()?;
        let prefix_host = prefix.host_str()?;
        let authority = match prefix.port() {
            Some(port) => format!("{prefix_host}:{port}"),
            None => prefix_host.to_owned(),
        };
        // The authority as written: the parser drops a default port such as `:443`.
        let written = text
            .split_once("://")
            .and_then(|(_, rest)| rest.split(['/', '?', '#']).next());
        let ours = written.is_some_and(|written| written.eq_ignore_ascii_case(&authority))
            && url.scheme() == prefix.scheme()
            && url.host_str().is_some()
            && url.host_str() == prefix.host_str()
            && url.port() == prefix.port()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url
                .path()
                .strip_prefix(prefix.path())
                .is_some_and(is_pay_id);
        if !ours {
            return None;
        }
        match origin {
            Origin::Production => Some(PayUrl(url.into())),
            #[cfg(feature = "staging")]
            Origin::Staging => Some(PayUrl(url.into())),
            #[cfg(feature = "local")]
            Origin::Local => Some(PayUrl(url.into())),
            #[cfg(feature = "test-support")]
            Origin::Fake(_) => Some(PayUrl(format!(
                "{PRODUCTION_ORIGIN}{}",
                &url[Position::BeforePath..]
            ))),
        }
    }

    /// Whether this link still reads as a pay link of `origin`, as [`PayUrl::parse_for`] would
    /// return it: under `Production` only `https://data.clauseo.chat/pay/<id>`, under `Staging`
    /// only `https://data-staging.clauseo.chat/pay/<id>`, under `Local` only
    /// `http://127.0.0.1:8793/pay/<id>` (that exact origin). Under `Fake` the link was rewritten
    /// to the production form, so it is checked as production. `OpenPayLink`'s check before it
    /// opens.
    pub fn is_for(&self, connector: &KnownConnector, origin: &Origin) -> bool {
        let reparsed = match origin {
            Origin::Production => PayUrl::parse_for(connector, origin, &self.0),
            #[cfg(feature = "staging")]
            Origin::Staging => PayUrl::parse_for(connector, origin, &self.0),
            #[cfg(feature = "local")]
            Origin::Local => PayUrl::parse_for(connector, origin, &self.0),
            #[cfg(feature = "test-support")]
            Origin::Fake(_) => PayUrl::parse(connector, &self.0),
        };
        reparsed.as_ref() == Some(self)
    }

    /// The link.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `[A-Za-z0-9_-]{1,128}`: the id part of a pay link.
fn is_pay_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The wire form of a payment object.
#[derive(Deserialize)]
struct RechargeIn {
    #[serde(default, deserialize_with = "de::text")]
    recharge_id: String,
    #[serde(default)]
    status: RechargeStatus,
    #[serde(default, deserialize_with = "de::opt_text")]
    url: Option<String>,
    #[serde(default, deserialize_with = "de::opt_object")]
    amount: Option<Amount>,
    #[serde(default, deserialize_with = "de::opt_int")]
    credits: Option<i64>,
    #[serde(default, deserialize_with = "de::opt_text")]
    expires_at: Option<String>,
}

/// The payment object of a recharge answer: the answer itself, or its `payment` key.
fn payment_object(value: &Value) -> &Value {
    match value.get("payment") {
        Some(payment) if payment.is_object() => payment,
        _ => value,
    }
}

/// A recharge from a payment object. A pay link that is not ours is `Err(NotOurPayLink)` when
/// `strict`, else dropped. A missing `recharge_id` is a parse error.
pub(crate) fn recharge_from_value(
    value: &Value,
    connector: &KnownConnector,
    origin: &Origin,
    strict: bool,
) -> Result<Recharge, Error> {
    let wire: RechargeIn = serde_json::from_value(payment_object(value).clone())
        .map_err(|err| parse_error("recharge", &err))?;
    if wire.recharge_id.is_empty() {
        return Err(Error::Parse("recharge: no recharge_id".to_owned()));
    }
    let url = match wire.url {
        None => None,
        Some(text) => match PayUrl::parse_for(connector, origin, &text) {
            Some(url) => Some(url),
            None if strict => return Err(Error::NotOurPayLink),
            None => None,
        },
    };
    Ok(Recharge {
        recharge_id: wire.recharge_id,
        status: wire.status,
        url,
        amount: wire.amount,
        credits: wire.credits,
        expires_at: wire.expires_at,
    })
}

/// A recharge answer (`POST /recharges`, `GET /recharges/{id}`): `NotOurPayLink` for a pay link
/// that is not ours, so no such link is returned.
pub(crate) fn parse_recharge(
    bytes: &[u8],
    connector: &KnownConnector,
    origin: &Origin,
) -> Result<Recharge, Error> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| parse_error("recharge", &err))?;
    if !value.is_object() {
        return Err(Error::Parse("recharge: not an object".to_owned()));
    }
    recharge_from_value(&value, connector, origin, true)
}

/// The payment an out-of-credit body points to (a tool result's `structuredContent`, or the
/// JSON API's 402 body): `payment.recharge_id`, and `payment.url` when it is ours.
pub(crate) fn payment_ref(body: &Value, connector: &KnownConnector, origin: &Origin) -> PaymentRef {
    let payment = body.get("payment");
    let field = |name: &str| {
        payment
            .and_then(|payment| payment.get(name))
            .and_then(Value::as_str)
    };
    PaymentRef {
        recharge_id: field("recharge_id").map(str::to_owned),
        url: field("url").and_then(|url| PayUrl::parse_for(connector, origin, url)),
    }
}

/// Whether a body is out of credit: `error.code == "insufficient_credits"`.
pub(crate) fn is_insufficient_credits(body: &Value) -> bool {
    body.pointer("/error/code").and_then(Value::as_str) == Some("insufficient_credits")
}

/// The JSON API's 402 as `Error::InsufficientCredits`, with whatever payment the body holds.
pub(crate) fn insufficient_credits_402(
    bytes: &[u8],
    connector: &KnownConnector,
    origin: &Origin,
) -> Error {
    let body = serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null);
    Error::InsufficientCredits(payment_ref(&body, connector, origin))
}

#[cfg(test)]
mod tests;
