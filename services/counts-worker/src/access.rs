//! The Cloudflare Access check for `/dash` and `/v1/query` (decision 0061). Access sits in front
//! of both routes, and the Worker checks its JWT again, so a misconfigured Access application
//! cannot open them: the `Cf-Access-Jwt-Assertion` token must be RS256 (`none` and `HS*` are
//! refused), name a `kid` from the team's certificates, be unexpired, come from
//! `https://<team>.cloudflareaccess.com`, carry the route's own `aud`, and name the route's one
//! caller: the owner's `email` on `/dash`, the agents' service token's `common_name` (its client
//! id) on `/v1/query`. So a policy added to an Access application by hand lets no one else in.
//! The signature itself is checked behind [`Verifier`], `WebCrypto` in the Worker
//! (`webcrypto.rs`), so the claims are tested natively. Nothing here reads a claim beyond those,
//! and nothing is kept.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

/// How long the team's certificates are kept before they are fetched again, in seconds.
pub(crate) const KEYS_FOR_SECS: u64 = 3_600;
/// How old the kept certificates must be before a token naming an unknown `kid` makes the
/// Worker fetch them again (Access rotates its key every few weeks).
pub(crate) const REFETCH_AFTER_SECS: u64 = 60;
/// The clock difference allowed for `nbf`.
const SKEW_SECS: u64 = 60;
/// The longest token read; Access's are about 1 KB.
const MAX_TOKEN: usize = 8_192;

/// Why a token was refused; every one is answered `403` with an empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Denied {
    /// No token, or not three base64url parts of JSON.
    Malformed,
    /// An algorithm other than RS256, `none` and `HS256` included.
    Algorithm,
    /// No `kid`, or one the team's certificates do not hold.
    Kid,
    /// Past `exp`, or before `nbf`.
    Expired,
    /// Another issuer.
    Issuer,
    /// Not this route's `aud`.
    Audience,
    /// Not this route's caller: another email, another service token, or none.
    Identity,
    /// The signature does not verify.
    Signature,
}

/// One of the team's RSA signing keys, as its JWK parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    /// The key id a token names in its header.
    pub(crate) kid: String,
    /// The modulus, base64url.
    pub(crate) n: String,
    /// The public exponent, base64url.
    pub(crate) e: String,
}

/// The parts of the certificates document read: RSA keys for signing, nothing else.
#[derive(Deserialize)]
struct Certs {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kty: String,
    #[serde(default)]
    alg: Option<String>,
    #[serde(rename = "use", default)]
    usage: Option<String>,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
}

/// The RS256 keys of a `…/cdn-cgi/access/certs` document; `None` when it is not one.
pub(crate) fn parse_keys(json: &[u8]) -> Option<Vec<Key>> {
    let certs: Certs = serde_json::from_slice(json).ok()?;
    Some(
        certs
            .keys
            .into_iter()
            .filter(|jwk| {
                jwk.kty == "RSA"
                    && jwk.alg.as_deref().is_none_or(|alg| alg == "RS256")
                    && jwk.usage.as_deref().is_none_or(|usage| usage == "sig")
            })
            .filter_map(|jwk| {
                Some(Key {
                    kid: jwk.kid?,
                    n: jwk.n?,
                    e: jwk.e?,
                })
            })
            .collect(),
    )
}

/// Who may call a route, and the claim that names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caller {
    /// `/dash`: a person, whose token's `email` must be `DASH_EMAIL` (case-insensitive).
    Owner,
    /// `/v1/query`: a service token, whose token's `common_name` must be `QUERY_CLIENT_ID`.
    Agent,
}

/// A route's Access settings: the team, the application's `aud` and the one caller let in.
/// `None` when any is missing or the team is not a plain name, and then the route answers `404`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Access {
    team: String,
    aud: String,
    caller: Caller,
    who: String,
}

impl Access {
    /// The settings, if all are set and the team is a plain DNS label. `who` is the owner's
    /// email or the service token's client id, as `caller` says.
    pub(crate) fn new(team: &str, aud: &str, caller: Caller, who: &str) -> Option<Access> {
        let label = !team.is_empty()
            && team.len() <= 63
            && team
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        (label && !aud.is_empty() && !who.trim().is_empty()).then(|| Access {
            team: team.to_owned(),
            aud: aud.to_owned(),
            caller,
            who: who.trim().to_owned(),
        })
    }

    /// The issuer every token must name.
    pub(crate) fn issuer(&self) -> String {
        format!("https://{}.cloudflareaccess.com", self.team)
    }

    /// Where the team publishes its signing keys.
    pub(crate) fn certs_url(&self) -> String {
        format!("{}/cdn-cgi/access/certs", self.issuer())
    }
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

/// `aud` is a list in Access's tokens; a single string is read too.
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    iss: String,
    aud: Audience,
    /// A person's login email; absent from a service token's token.
    #[serde(default)]
    email: Option<String>,
    /// A service token's client id; absent from a person's token.
    #[serde(default)]
    common_name: Option<String>,
}

/// A token split and decoded, its algorithm already pinned to RS256.
pub(crate) struct Token {
    kid: String,
    claims: Claims,
    signed: String,
    signature: Vec<u8>,
}

fn decode(part: &str) -> Result<Vec<u8>, Denied> {
    URL_SAFE_NO_PAD.decode(part).or(Err(Denied::Malformed))
}

impl Token {
    /// Splits and decodes `jwt`. The header's `alg` must be exactly `RS256`.
    pub(crate) fn parse(jwt: &str) -> Result<Token, Denied> {
        if jwt.len() > MAX_TOKEN {
            return Err(Denied::Malformed);
        }
        let mut parts = jwt.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Denied::Malformed);
        };
        let head: Header = serde_json::from_slice(&decode(header)?).or(Err(Denied::Malformed))?;
        if head.alg != "RS256" {
            return Err(Denied::Algorithm);
        }
        let kid = head.kid.filter(|kid| !kid.is_empty()).ok_or(Denied::Kid)?;
        let claims: Claims =
            serde_json::from_slice(&decode(payload)?).or(Err(Denied::Malformed))?;
        let signature = decode(signature)?;
        if signature.is_empty() {
            return Err(Denied::Signature);
        }
        Ok(Token {
            kid,
            claims,
            signed: format!("{header}.{payload}"),
            signature,
        })
    }

    /// The key id the header names.
    pub(crate) fn kid(&self) -> &str {
        &self.kid
    }

    /// `exp`, `nbf`, `iss`, `aud` and the caller against the route's settings at `now`
    /// (Unix seconds).
    pub(crate) fn check_claims(&self, access: &Access, now: u64) -> Result<(), Denied> {
        let claims = &self.claims;
        if now >= claims.exp || claims.nbf.is_some_and(|nbf| nbf > now + SKEW_SECS) {
            return Err(Denied::Expired);
        }
        if claims.iss != access.issuer() {
            return Err(Denied::Issuer);
        }
        let ours = match &claims.aud {
            Audience::One(aud) => *aud == access.aud,
            Audience::Many(auds) => auds.contains(&access.aud),
        };
        if !ours {
            return Err(Denied::Audience);
        }
        let caller = match access.caller {
            Caller::Owner => claims
                .email
                .as_deref()
                .is_some_and(|email| email.eq_ignore_ascii_case(&access.who)),
            Caller::Agent => claims.common_name.as_deref() == Some(access.who.as_str()),
        };
        if caller {
            Ok(())
        } else {
            Err(Denied::Identity)
        }
    }
}

/// Checks an RS256 signature over the token's first two parts with one of the team's keys.
pub(crate) trait Verifier {
    /// Whether `signature` is `key`'s RS256 signature of `signed`.
    async fn verify(&self, key: &Key, signed: &[u8], signature: &[u8]) -> bool;
}

/// The whole check: the token parses with RS256, its claims fit `access` at `now`, its `kid`
/// is one of `keys`, and its signature verifies. Claims come before the signature, so a
/// refused token costs no `WebCrypto` call.
pub(crate) async fn check(
    jwt: Option<&str>,
    access: &Access,
    keys: &[Key],
    now: u64,
    verifier: &impl Verifier,
) -> Result<(), Denied> {
    let token = Token::parse(jwt.ok_or(Denied::Malformed)?)?;
    token.check_claims(access, now)?;
    let key = keys
        .iter()
        .find(|key| key.kid == token.kid)
        .ok_or(Denied::Kid)?;
    if verifier
        .verify(key, token.signed.as_bytes(), &token.signature)
        .await
    {
        Ok(())
    } else {
        Err(Denied::Signature)
    }
}

/// The variable `wrangler dev` sets to serve test keys (tests/access.test.mjs). It is never
/// set in production: the deploy passes only `ACCESS_TEAM`, the AUDs and the two callers, and
/// `check-deploy.mjs` refuses a config that names it. Even if it were set, only a loopback URL
/// is taken, which the edge cannot reach, so the check would fail closed.
pub(crate) const DEV_CERTS_VAR: &str = "ACCESS_CERTS_URL_DEV";

/// `value` as the dev certificate URL, if it is an `http://` loopback address with a port.
pub(crate) fn dev_certs_url(value: &str) -> Option<&str> {
    let rest = value
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| value.strip_prefix("http://localhost:"))?;
    let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let plain = !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b));
    plain.then_some(value)
}

/// The team's keys as last fetched, and when.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyCache {
    /// The certificates URL they came from.
    pub(crate) url: String,
    /// When they were fetched, in Unix seconds.
    pub(crate) fetched: u64,
    /// The keys.
    pub(crate) keys: Vec<Key>,
}

impl KeyCache {
    /// Whether the keys must be fetched before checking a token naming `kid` at `now`: none
    /// from `url` yet, older than an hour, or `kid` unknown and the keys over a minute old.
    pub(crate) fn stale(&self, url: &str, kid: &str, now: u64) -> bool {
        let age = now.saturating_sub(self.fetched);
        self.url != url
            || self.keys.is_empty()
            || age >= KEYS_FOR_SECS
            || (age >= REFETCH_AFTER_SECS && !self.keys.iter().any(|key| key.kid == kid))
    }
}

#[cfg(test)]
mod tests;
