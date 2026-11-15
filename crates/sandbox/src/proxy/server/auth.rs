//! Each listener's credential check: the agent listener takes the agent's token only, the
//! browser listener its own only. Both modes, every request, before any other check.

use crate::proxy::head::Request;
use crate::proxy::relay::Stream;
use crate::proxy::target::classify;
use crate::proxy::token::Token;
use crate::proxy::{Exchange, Listener, Verdict};

use super::Ctx;

/// The challenge a 407 carries, on either listener.
const CHALLENGE: &str = "Proxy-Authenticate: Basic realm=\"clauseo\"\r\n";

/// The launch's two tokens: the agent listener's, and the browser listener's when it is bound.
/// Never logged; neither appears in an `Exchange`.
pub(crate) struct Tokens {
    pub(crate) agent: Token,
    pub(crate) browser: Option<Token>,
}

impl Tokens {
    /// The token `which` checks; `None` (no browser token) admits no request.
    fn of(&self, which: Listener) -> Option<&Token> {
        match which {
            Listener::Agent => Some(&self.agent),
            Listener::Browser => self.browser.as_ref(),
        }
    }
}

/// True when the request carries the credential of the listener it came in on, compared in
/// constant time (`Token::admits`). Otherwise answers 407 with the challenge, sets
/// `Verdict::AuthRequired`, and returns false: a missing credential, a wrong one, or the other
/// listener's token alike. The exchange then names the host asked for, for the log; nothing is
/// checked or reported for it.
pub(super) async fn admitted(
    ctx: &Ctx,
    client: &Stream,
    request: &Request,
    exchange: &mut Exchange,
) -> bool {
    let values = request.values("proxy-authorization");
    if ctx
        .tokens
        .of(exchange.listener)
        .is_some_and(|token| token.admits(&values))
    {
        return true;
    }
    match classify(request) {
        Ok(target) => {
            exchange.host = target.name.text();
            exchange.port = target.port;
        }
        Err(refusal) => {
            exchange.host = refusal.host;
            exchange.port = refusal.port;
        }
    }
    ctx.answer(client, exchange, 407, Verdict::AuthRequired, CHALLENGE)
        .await;
    false
}
