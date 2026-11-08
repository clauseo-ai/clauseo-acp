//! Rupee amounts as the app sees them: whole paise, GST included. The connector writes rupees
//! as decimal text, so they are read here with integer arithmetic only, never floats.
//!
//! - [`paise`] reads an amount the connector already wrote in rupees (a pack's `gross_inr`).
//! - [`GrossRate`] prices credits, which is how the account gives its spend and balance: the
//!   price list's `credit_inr` (before GST) and `gst_rate`, held exactly. A credit's gross worth
//!   is not whole paise (₹0.10 at 18% is 11.8 paise), so each amount is rounded half-up to the
//!   paise once, from its exact value.

use clauseo_connectors::Pricing;

/// The fraction digits [`GrossRate`] keeps of `credit_inr` and `gst_rate`.
const RATE_DIGITS: u32 = 4;

/// `10^RATE_DIGITS`.
const RATE_ONE: u128 = 10_u128.pow(RATE_DIGITS);

/// The gross (GST-included) worth of one credit, exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GrossRate {
    /// `credit_inr` in ten-thousandths of a rupee, above nothing.
    credit: u128,
    /// `gst_rate` in ten-thousandths.
    gst: u128,
}

impl GrossRate {
    /// The rate from the price list; `None` when `credit_inr` or `gst_rate` is missing, is not
    /// plain decimal text with at most four fraction digits, or `credit_inr` is zero.
    pub(crate) fn from_pricing(pricing: &Pricing) -> Option<Self> {
        let credit = fixed(&pricing.credit_inr, RATE_DIGITS).filter(|credit| *credit > 0)?;
        let gst = fixed(&pricing.gst_rate, RATE_DIGITS)?;
        Some(Self {
            credit: u128::from(credit),
            gst: u128::from(gst),
        })
    }

    /// `credits` at this rate in paise, rounded half-up; `None` for a negative count or more
    /// than a `u32` holds. Every step is checked: the rates come from the server, and an
    /// overflow must give `None`, not a panic or a wrapped amount.
    pub(crate) fn paise(self, credits: i64) -> Option<u32> {
        let credits = u128::from(u64::try_from(credits).ok()?);
        // rupees = credits * credit / ONE * (ONE + gst) / ONE; paise = rupees * 100.
        let scale = RATE_ONE * RATE_ONE / 100;
        let exact = credits
            .checked_mul(self.credit)?
            .checked_mul(RATE_ONE.checked_add(self.gst)?)?;
        u32::try_from(exact.checked_add(scale / 2)? / scale).ok()
    }
}

/// Rupees written as decimal text (`"23.60"`, `"236"`, `"0.1"`) in paise; `None` for anything
/// else: a sign, a grouping comma, a currency mark, more than two paise digits, or more than a
/// `u32` holds.
pub(crate) fn paise(text: &str) -> Option<u32> {
    u32::try_from(fixed(text, 2)?).ok()
}

/// Decimal text in units of `10^-digits`: one to ten whole digits, then optionally a point and
/// one to `digits` fraction digits. No sign, grouping, spaces or currency mark.
fn fixed(text: &str, digits: u32) -> Option<u64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (text, ""),
    };
    let all_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let places = u32::try_from(fraction.len()).ok()?;
    if whole.is_empty()
        || whole.len() > 10
        || !all_digits(whole)
        || places > digits
        || !all_digits(fraction)
    {
        return None;
    }
    let whole: u64 = whole.parse().ok()?;
    let part: u64 = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .ok()?
            .checked_mul(10_u64.checked_pow(digits.checked_sub(places)?)?)?
    };
    whole
        .checked_mul(10_u64.checked_pow(digits)?)?
        .checked_add(part)
}
