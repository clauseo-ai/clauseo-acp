//! The skills.sh catalogue's search.

use std::time::Duration;

use clauseo_profile::packs::added::display_name;
use clauseo_profile::packs::source::{SourceError, parse_skills_sh};
use serde_json::Value;
use url::Url;

use super::download::{Net, download};

/// The most items a search answers.
pub(crate) const SEARCH_MAX: usize = 30;

/// The most bytes of a search's answer.
const ANSWER_MAX: u64 = 1024 * 1024;

/// The longest a search takes.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);

/// The most characters of a field kept.
const FIELD_MAX: usize = 200;

/// One catalogue item, as skills.sh answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogueSkill {
    /// `<owner>/<repo>/<skillId>`, also its page's path on skills.sh.
    pub(crate) id: String,
    /// `<owner>/<repo>`.
    pub(crate) source: String,
    /// Its name.
    pub(crate) name: String,
    /// How many times it was installed.
    pub(crate) installs: u64,
}

/// Searches the catalogue for `terms` (every term must match): at most [`SEARCH_MAX`] items;
/// none found and more than one term given, once more with the last term dropped. Empty terms
/// find nothing and send nothing. Off the actor.
pub(crate) fn search(net: &Net, terms: &str) -> Result<Vec<CatalogueSkill>, SourceError> {
    let words: Vec<&str> = terms.split_whitespace().collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let found = ask(net, &words.join(" "))?;
    if let Some((_last, rest)) = words.split_last()
        && found.is_empty()
        && !rest.is_empty()
    {
        return ask(net, &rest.join(" "));
    }
    Ok(found)
}

/// One search request.
fn ask(net: &Net, query: &str) -> Result<Vec<CatalogueSkill>, SourceError> {
    let mut url = Url::parse(&net.search_url).map_err(|_bad| SourceError::BadSource)?;
    url.query_pairs_mut().clear().append_pair("q", query);
    let body = download(net, url.as_str(), ANSWER_MAX, SEARCH_TIMEOUT)?;
    let answer: Value =
        serde_json::from_slice(&body).map_err(|_bad| SourceError::DownloadFailed)?;
    let items = answer
        .get("skills")
        .and_then(Value::as_array)
        .ok_or(SourceError::DownloadFailed)?;
    Ok(items.iter().filter_map(item).take(SEARCH_MAX).collect())
}

/// One answer item, or `None` when a field is missing or not a short text, or when `id` and
/// `source` are not a skills.sh name (`<owner>/<repo>/<skill>`, `<owner>/<repo>`). The name is
/// listed as a preview lists a skill's (`added::display_name`: no control, bidi or zero-width
/// character, at most 200 characters and bytes).
fn item(value: &Value) -> Option<CatalogueSkill> {
    let text = |key: &str| {
        let text = value.get(key)?.as_str()?;
        let plain = !text.is_empty()
            && text.chars().count() <= FIELD_MAX
            && !text.chars().any(char::is_control);
        plain.then(|| text.to_owned())
    };
    let name_of = |key: &str, parts: usize| {
        text(key).filter(|name| {
            name.split('/').count() == parts && !name.contains('@') && parse_skills_sh(name).is_ok()
        })
    };
    let id = name_of("id", 3)?;
    let source = name_of("source", 2)?;
    let name = display_name(value.get("name")?.as_str()?);
    Some(CatalogueSkill {
        id,
        source,
        name: (!name.is_empty()).then_some(name)?,
        installs: value.get("installs").and_then(Value::as_u64).unwrap_or(0),
    })
}
