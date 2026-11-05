//! The main window's search route (0043). Searches are superseded per page, so the host hears
//! which page asks: the window's label and the number of its kept `open_stream`, so a reloaded
//! page counts afresh.

use clauseo_protocol::{HostError, QueryResult, SearchRequest};

use super::{Bridge, PRINCIPAL, lock};

/// Runs the window `label`'s search `request` on the host, as that page.
pub(super) async fn page(
    bridge: &Bridge,
    label: &str,
    request: SearchRequest,
) -> Result<QueryResult, HostError> {
    let page = lock(&bridge.windows).get(label).map_or_else(
        || label.to_owned(),
        |(number, _)| format!("{label}#{number}"),
    );
    let host = bridge.host().await?;
    let answer = host.search(PRINCIPAL, &page, request).await?;
    Ok(QueryResult::Search(answer))
}
