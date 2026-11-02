//! `pricing.json`: whether the connector is live, its prices and its packs. Read at run time,
//! never hard-coded: the figures are not final.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};

use crate::de;
use crate::error::{Error, parse_error};

/// Whether the slot shows, and what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Liveness {
    /// True only for a 200 whose JSON parses with `status: "live"`.
    pub live: bool,
    /// The parsed `pricing.json`, when one parsed (a `preview` one too).
    pub pricing: Option<Pricing>,
    /// When it was fetched; `None` before the first fetch.
    pub fetched_at: Option<SystemTime>,
}

impl Liveness {
    /// Not live, nothing parsed: a failed fetch at `at`.
    pub(crate) fn failed(at: SystemTime) -> Liveness {
        Liveness {
            live: false,
            pricing: None,
            fetched_at: Some(at),
        }
    }
}

/// `pricing.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Pricing {
    /// `live`, `preview` (staging before launch: hidden), or anything else (hidden).
    #[serde(default)]
    pub status: PricingStatus,
    /// `INR`.
    #[serde(default, deserialize_with = "de::text")]
    pub currency: String,
    /// Rupees per credit, as written (`"0.10"`).
    #[serde(default, deserialize_with = "de::text")]
    pub credit_inr: String,
    /// The GST rate, as written (`"0.18"`).
    #[serde(default, deserialize_with = "de::text")]
    pub gst_rate: String,
    /// Credits per action (`search_page`, `open`, ...).
    #[serde(default, deserialize_with = "de::prices")]
    pub prices: BTreeMap<String, i64>,
    /// The packs on sale.
    #[serde(default, deserialize_with = "de::list")]
    pub packs: Vec<Pack>,
    /// Credits granted when an account is claimed.
    #[serde(default, deserialize_with = "de::opt_int")]
    pub starter_credits_on_claim: Option<i64>,
}

/// `pricing.json`'s `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingStatus {
    /// `live`: the slot shows.
    Live,
    /// `preview`: hidden.
    Preview,
    /// Anything else, as written: hidden.
    Other(String),
}

impl Default for PricingStatus {
    fn default() -> PricingStatus {
        PricingStatus::Other(String::new())
    }
}

impl<'de> Deserialize<'de> for PricingStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PricingStatus, D::Error> {
        Ok(match de::text(deserializer)?.as_str() {
            "live" => PricingStatus::Live,
            "preview" => PricingStatus::Preview,
            other => PricingStatus::Other(other.to_owned()),
        })
    }
}

/// A pack of credits on sale.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "the connector's field names, fixed by the brief's API"
)]
pub struct Pack {
    /// The pack's name, sent as `{"pack": "..."}` to buy it (`"200"`).
    #[serde(default, deserialize_with = "de::text")]
    pub pack: String,
    /// Credits it adds.
    #[serde(default, deserialize_with = "de::int")]
    pub credits: i64,
    /// Rupees before GST, as written.
    #[serde(default, deserialize_with = "de::text")]
    pub net_inr: String,
    /// GST in rupees, as written.
    #[serde(default, deserialize_with = "de::text")]
    pub gst_inr: String,
    /// Rupees with GST, as written.
    #[serde(default, deserialize_with = "de::text")]
    pub gross_inr: String,
}

/// `pricing.json` with the one field kept outside [`Pricing`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PricingDoc {
    pub(crate) pricing: Pricing,
    /// `endpoints.mcp`, which must equal the compiled-in MCP URL (it is never used).
    pub(crate) endpoint_mcp: Option<String>,
}

#[derive(Deserialize)]
struct DocIn {
    #[serde(flatten)]
    pricing: Pricing,
    #[serde(default, deserialize_with = "de::opt_object")]
    endpoints: Option<EndpointsIn>,
}

#[derive(Deserialize)]
struct EndpointsIn {
    #[serde(default, deserialize_with = "de::opt_text")]
    mcp: Option<String>,
}

/// Parses `pricing.json`.
pub(crate) fn parse_pricing(bytes: &[u8]) -> Result<PricingDoc, Error> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| parse_error("pricing", &err))?;
    if !value.is_object() {
        return Err(Error::Parse("pricing: not an object".to_owned()));
    }
    let doc: DocIn = serde_json::from_value(value).map_err(|err| parse_error("pricing", &err))?;
    Ok(PricingDoc {
        pricing: doc.pricing,
        endpoint_mcp: doc.endpoints.and_then(|endpoints| endpoints.mcp),
    })
}

/// Liveness from a `pricing.json` answer: live only for a 200 whose JSON parses and whose
/// `status` is `live`. An `endpoints.mcp` other than `mcp_url` is logged and ignored.
pub(crate) fn liveness_from(status: u16, body: &[u8], mcp_url: &str, at: SystemTime) -> Liveness {
    if status != 200 {
        return Liveness::failed(at);
    }
    let Ok(doc) = parse_pricing(body) else {
        return Liveness::failed(at);
    };
    if let Some(endpoint) = &doc.endpoint_mcp
        && endpoint != mcp_url
    {
        tracing::warn!(%endpoint, "pricing.json names another MCP URL; the compiled-in one is kept");
    }
    Liveness {
        live: doc.pricing.status == PricingStatus::Live,
        pricing: Some(doc.pricing),
        fetched_at: Some(at),
    }
}

#[cfg(test)]
mod tests;
