//! Prints where the egress proxy would send a connection to one URL, as one JSON line, using the
//! host's `HTTPS_PROXY` (`HTTP_PROXY` for an `http://` URL) and then the OS proxy settings or PAC
//! (`Upstream::System`).
//!
//! `cargo run -p clauseo-sandbox --example upstream -- https://api.anthropic.com/`
//!
//! Stage C's gate G10 runs it behind a firm proxy and PAC file to see what the OS returns.

use std::io::Write;

use clauseo_sandbox::proxy::upstream::{Route, Upstream, upstream_for};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("usage: upstream <url>, for example https://api.anthropic.com/")?;
    let url = url::Url::parse(&arg)?;
    let host = url.host_str().ok_or("the URL has no host")?;
    let port = url.port_or_known_default().ok_or("the URL has no port")?;
    let route = match upstream_for(&Upstream::System, &url)? {
        Route::Direct => serde_json::json!("direct"),
        Route::Http { host, port } => serde_json::json!({ "http": { "host": host, "port": port } }),
    };
    let line = serde_json::json!({ "host": host, "port": port, "route": route });
    writeln!(std::io::stdout().lock(), "{line}")?;
    Ok(())
}
