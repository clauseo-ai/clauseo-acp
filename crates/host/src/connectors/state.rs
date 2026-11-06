//! The connector state the host keeps, and the [`ConnectorsState`] the app sees of it (R24,
//! PR5). Plain data: the actor (`actor/connectors.rs`) changes it from the commands' work and
//! the relays' events, and [`CaseLaw::view`] draws the app's view.
//!
//! - Money reaches the app in paise, as integers, GST included (`money.rs`): what the lawyer
//!   sees is what they pay, and the app writes amounts by its own rule. Packs carry their own
//!   `gross_inr`; the account's spend and balance are credits, priced at the price list's
//!   `credit_inr` plus `gst_rate`. A figure that does not read is left out, never guessed.
//! - A pay link is held only as a [`PayUrl`], which only the connector's own `/pay/` links
//!   parse to; the view carries its text.
//! - No token, key or account id is held here: the credential is the connectors crate's.

use clauseo_connectors::{
    Account, CredentialStatus, KnownConnector, Liveness, Pack, PayUrl, Pricing,
};
use clauseo_protocol::{
    Balance, ConnectorStatus, ConnectorView, ConnectorsState, DaySpendView, PackView, RecentSpend,
};

mod money;

use money::GrossRate;
pub(crate) use money::paise;

/// The balance is low under this many search pages' worth of credits.
const LOW_SEARCH_PAGES: i64 = 10;

/// The price list's and the account's name for one page of search results.
const SEARCH_PAGE: &str = "search_page";

/// Out of credit, with the connector's pay link when it is our own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutOfCredit {
    pub(crate) pay_url: Option<PayUrl>,
}

/// The case-law connector as the host knows it.
#[derive(Debug, Default)]
pub(crate) struct CaseLaw {
    /// The credential as last read; `None` before the first read.
    pub(crate) credential: Option<CredentialStatus>,
    /// The system browser is open on the sign-in page.
    pub(crate) signing_in: bool,
    /// A sign-in or API key failed, until the next connector command: a short English line.
    pub(crate) error: Option<&'static str>,
    /// A paid call came back out of credit.
    pub(crate) out_of_credit: Option<OutOfCredit>,
    /// The last account read.
    pub(crate) account: Option<Account>,
    /// The pay link of the last `AddCredits`, until it is paid or settled otherwise.
    pub(crate) pay_url: Option<PayUrl>,
    /// The last liveness fetch.
    pub(crate) liveness: Option<Liveness>,
}

impl CaseLaw {
    /// The pay link `OpenPayLink` may open: only the one `AddCredits` got from the recharge API,
    /// a `PayUrl` the connectors crate built, and only while it still reads as the production
    /// form (https, host exactly `data.clauseo.chat`, path `/pay/<id>`). Out of credit's link
    /// came from a tool result the relay passed on, so it is shown, never opened from here; nor
    /// is any URL from relay output, a tool result or the agent.
    pub(crate) fn pay_link(&self) -> Option<&PayUrl> {
        self.pay_url.as_ref().filter(|url| {
            PayUrl::parse(&clauseo_connectors::CASE_LAW, url.as_str()).as_ref() == Some(*url)
        })
    }

    /// Whether the credential is a usable one.
    pub(crate) fn signed_in(&self) -> bool {
        matches!(self.credential, Some(CredentialStatus::SignedIn(_)))
    }

    /// Whether the price list says live.
    pub(crate) fn live(&self) -> bool {
        self.liveness.as_ref().is_some_and(|liveness| liveness.live)
    }

    /// A new credential: when it is no longer usable, the account, the out-of-credit state and
    /// the pay link go with it.
    pub(crate) fn set_credential(&mut self, credential: CredentialStatus) {
        self.credential = Some(credential);
        if !self.signed_in() {
            self.account = None;
            self.out_of_credit = None;
            self.pay_url = None;
        }
    }

    /// A new account read: a balance above nothing ends the out-of-credit state.
    pub(crate) fn set_account(&mut self, account: Account) {
        if account.balance_credits > 0 {
            self.out_of_credit = None;
        }
        self.account = Some(account);
    }

    /// The app's view, under the switch as it took effect. The connector is listed when it is
    /// live, or added (so it can be signed out or removed), or signing in.
    pub(crate) fn view(
        &self,
        known: &KnownConnector,
        available: bool,
        locked: bool,
    ) -> ConnectorsState {
        let added = !matches!(self.credential, None | Some(CredentialStatus::NotAdded));
        let listed = self.live() || added || self.signing_in;
        ConnectorsState {
            available,
            locked,
            connectors: if listed {
                vec![self.connector_view(known)]
            } else {
                Vec::new()
            },
        }
    }

    fn connector_view(&self, known: &KnownConnector) -> ConnectorView {
        let packs = self.packs();
        let rate = self.gross_rate();
        ConnectorView {
            id: known.id.to_owned(),
            name: known.name.to_owned(),
            price_paise: packs.iter().map(|pack| pack.paise).min(),
            packs,
            recent_spend: self
                .account
                .as_ref()
                .map(|account| recent_spend(account, rate)),
            status: self.status(rate),
            pay_url: self.pay_url.as_ref().map(|url| url.as_str().to_owned()),
        }
    }

    /// The price list, when live.
    fn live_pricing(&self) -> Option<&Pricing> {
        self.liveness
            .as_ref()
            .filter(|l| l.live)
            .and_then(|l| l.pricing.as_ref())
    }

    /// The packs on sale, when live; a pack whose price or credits do not read is left out.
    fn packs(&self) -> Vec<PackView> {
        self.live_pricing()
            .map(|pricing| pricing.packs.iter().filter_map(pack_view).collect())
            .unwrap_or_default()
    }

    /// A credit's gross worth, from the live price list: what turns the account's credits into
    /// the rupees the lawyer pays. `None`, with one warning that names no figure, when the
    /// list's `credit_inr` or `gst_rate` does not read, so no amount is guessed.
    fn gross_rate(&self) -> Option<GrossRate> {
        let pricing = self.live_pricing()?;
        let rate = GrossRate::from_pricing(pricing);
        if rate.is_none() {
            tracing::warn!("the price list's credit or GST rate does not read: no rupee amounts");
        }
        rate
    }

    fn status(&self, rate: Option<GrossRate>) -> ConnectorStatus {
        if self.signing_in {
            return ConnectorStatus::SigningIn;
        }
        if let Some(message) = self.error {
            return ConnectorStatus::Error {
                message: message.to_owned(),
            };
        }
        match self.credential {
            None | Some(CredentialStatus::NotAdded) => ConnectorStatus::NotAdded,
            Some(CredentialStatus::SignedOut) => ConnectorStatus::SignedOut,
            Some(CredentialStatus::KeyRevoked) => ConnectorStatus::KeyRevoked,
            Some(CredentialStatus::SignedIn(_)) => match &self.out_of_credit {
                Some(OutOfCredit { pay_url }) => ConnectorStatus::OutOfCredit {
                    pay_url: pay_url.as_ref().map(|url| url.as_str().to_owned()),
                },
                None => ConnectorStatus::SignedIn {
                    account_label: None,
                    balance: self.account.as_ref().map(|account| balance(account, rate)),
                    low: self.low(),
                },
            },
        }
    }

    /// Whether the balance is under ten search pages' worth, by the account's prices or else
    /// the price list's; never low with no balance or no price.
    fn low(&self) -> bool {
        let Some(account) = &self.account else {
            return false;
        };
        let price = account.prices.get(SEARCH_PAGE).copied().or_else(|| {
            self.liveness
                .as_ref()
                .and_then(|liveness| liveness.pricing.as_ref())
                .and_then(|pricing| pricing.prices.get(SEARCH_PAGE).copied())
        });
        price.is_some_and(|price| account.balance_credits < price.saturating_mul(LOW_SEARCH_PAGES))
    }
}

fn pack_view(pack: &Pack) -> Option<PackView> {
    let shown = paise(&pack.gross_inr).zip(i32::try_from(pack.credits).ok());
    if shown.is_none() {
        tracing::warn!(pack = %pack.pack, "a pack whose price or credits do not read is left out");
    }
    let (paise, credits) = shown?;
    Some(PackView {
        pack: pack.pack.clone(),
        credits,
        paise,
    })
}

/// The balance, priced from its credits at the gross rate; the account's own `balance_inr` is
/// before GST, so it is not shown.
fn balance(account: &Account, rate: Option<GrossRate>) -> Balance {
    Balance {
        credits: clamp(account.balance_credits),
        paise: rate.and_then(|rate| rate.paise(account.balance_credits)),
    }
}

fn recent_spend(account: &Account, rate: Option<GrossRate>) -> RecentSpend {
    let priced = |credits: i64| rate.and_then(|rate| rate.paise(credits));
    RecentSpend {
        today_credits: clamp(account.spent_today_credits),
        today_paise: priced(account.spent_today_credits),
        days: account
            .recent
            .iter()
            .map(|day| DaySpendView {
                day: day.day.clone(),
                credits: clamp(day.credits),
                paise: priced(day.credits),
            })
            .collect(),
    }
}

/// A count of credits in the wire's `i32`, held at its ends.
fn clamp(credits: i64) -> i32 {
    i32::try_from(credits).unwrap_or(if credits < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests;
