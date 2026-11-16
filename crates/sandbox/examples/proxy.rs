//! A launch's egress proxy, standing alone, for G8b and G10.
//!
//! ```text
//! cargo run -p clauseo-sandbox --example proxy -- --allow api.anthropic.com \
//!     --allow-file <path> --log <file.jsonl> [--upstream http://host:port] [--open] \
//!     [--block <host>]
//! ```
//!
//! Prints `{"port":..,"browser_port":..,"token":..,"browser_token":..}` on stdout (each
//! listener's own password; the user is `clauseo`), then logs every request as one JSON line
//! (listener, host, port, verdict, why, bytes each way) to `--log`, or to stderr without it,
//! and every denial as a JSON line on stderr. Neither token is ever logged. The proxy's own
//! `tracing` lines at info and above, from Clauseo's crates, go to stderr too, each as
//! `{"log":{"level":..,"target":..,"message":..,<fields>}}` (in open mode, the name-only path's
//! probe turning it on or off, and the learned firm names' counts). `--open` admits
//! any public host (`ProxyMode::Open`) in place of the allowlist. `--block` (may repeat) loads
//! a block list naming those hosts as phishing; without it no list is loaded, the proxy logs
//! once that nothing is blocked, and `--open` does not start (open mode needs a list). Runs
//! until killed. `--allow` and `--allow-file` may repeat;
//! an allow file has one host per line, a `*.domain` line is a firm wildcard, and `#` starts a
//! comment.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, LineWriter, Write};
use std::sync::{Arc, Mutex, mpsc};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

use clauseo_sandbox::{
    Allowlist, Denial, DenialSink, Exchange, ProxyConfig, Reachability, Upstream, start_proxy,
};

/// Denials as JSON lines on stderr.
struct StderrSink;

impl DenialSink for StderrSink {
    fn denied(&self, d: Denial) {
        line(&mut io::stderr(), &serde_json::json!({ "denial": d }));
    }

    fn upstream(&self, r: Reachability) {
        line(&mut io::stderr(), &serde_json::json!({ "upstream": r }));
    }
}

/// The proxy's `tracing` lines at info and above, from `clauseo_*` targets, as JSON on stderr.
struct StderrTrace;

/// An event's fields, each as text.
#[derive(Default)]
struct Fields(serde_json::Map<String, serde_json::Value>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

impl Subscriber for StderrTrace {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event()
            && *metadata.level() <= Level::INFO
            && metadata.target().starts_with("clauseo_")
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::INFO)
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        let meta = event.metadata();
        fields
            .0
            .insert("level".to_owned(), meta.level().as_str().into());
        fields.0.insert("target".to_owned(), meta.target().into());
        event.record(&mut fields);
        line(&mut io::stderr(), &serde_json::json!({ "log": fields.0 }));
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

fn line(out: &mut dyn Write, value: &serde_json::Value) {
    if let Err(error) = writeln!(out, "{value}") {
        drop(writeln!(
            io::stderr(),
            "proxy example: write failed: {error}"
        ));
    }
}

#[derive(Default)]
struct Args {
    allow: Allowlist,
    log: Option<String>,
    upstream: Option<url::Url>,
    open: bool,
    block: Vec<String>,
}

fn args() -> Result<Args, Box<dyn Error>> {
    let mut parsed = Args::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--allow" => {
                let host = value()?;
                parsed.allow = std::mem::take(&mut parsed.allow).union(Allowlist::exact(&host)?);
            }
            "--allow-file" => {
                let text = std::fs::read_to_string(value()?)?;
                for entry in text
                    .lines()
                    .map(|l| l.split('#').next().unwrap_or("").trim())
                {
                    if entry.is_empty() {
                        continue;
                    }
                    let one = if entry.starts_with("*.") {
                        Allowlist::firm_suffix(entry)?
                    } else {
                        Allowlist::exact(entry)?
                    };
                    parsed.allow = std::mem::take(&mut parsed.allow).union(one);
                }
            }
            "--log" => parsed.log = Some(value()?),
            "--open" => parsed.open = true,
            "--block" => parsed.block.push(value()?.to_ascii_lowercase()),
            "--upstream" => parsed.upstream = Some(url::Url::parse(&value()?)?),
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }
    Ok(parsed)
}

/// The block list in use: none loaded without `--block`, else one naming those hosts as
/// phishing (tag `p`), made now as far as the list's freshness rules care.
fn block_list(hosts: &[String]) -> Result<clauseo_blocklist::Current, Box<dyn Error>> {
    if hosts.is_empty() {
        return Ok(clauseo_blocklist::Current::default());
    }
    let mut lines: Vec<String> = hosts.iter().map(|host| format!("{host} p")).collect();
    lines.sort_unstable();
    lines.dedup();
    let text = format!("{}\n", lines.join("\n"));
    let header = clauseo_blocklist::Header {
        generated: clauseo_blocklist::Generated::parse("2026-11-15T00:00:00Z")
            .ok_or("a bad generated time")?,
        entries: u32::try_from(lines.len())?,
    };
    let list = clauseo_blocklist::parse_plain(text.as_bytes(), header)?;
    Ok(clauseo_blocklist::Current::new(list))
}

#[expect(
    clippy::disallowed_methods,
    reason = "the example writes only the log file its user names"
)]
fn open_log(path: &str) -> io::Result<File> {
    File::create(path)
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing::subscriber::set_global_default(StderrTrace)?;
    let args = args()?;
    let log: Box<dyn Write + Send> = match &args.log {
        Some(path) => Box::new(LineWriter::new(open_log(path)?)),
        None => Box::new(io::stderr()),
    };
    let log = Mutex::new(log);
    let record = move |exchange: &Exchange| {
        let value = serde_json::to_value(exchange).unwrap_or_default();
        let mut out = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        line(&mut **out, &value);
    };
    let config = ProxyConfig {
        mode: if args.open {
            clauseo_sandbox::ProxyMode::Open
        } else {
            clauseo_sandbox::ProxyMode::Allowlist
        },
        allow: args.allow,
        silent: Allowlist::default(),
        // No list loaded without --block: the proxy logs once that nothing is blocked, and open
        // mode refuses to start (`Error::BlockListNotLoaded`).
        block: Arc::new(block_list(&args.block)?),
        allowed: Arc::default(),
        upstream: args.upstream.map_or(Upstream::System, Upstream::Fixed),
        denials: Arc::new(StderrSink),
        record: Some(Arc::new(record)),
        browser: true,
        listener: clauseo_sandbox::ports::ListenerFor::Other,
    };
    let handle = start_proxy(config)?;
    let token = handle
        .env()
        .into_iter()
        .find(|(name, _)| name == "HTTPS_PROXY")
        .and_then(|(_, value)| value?.into_string().ok())
        .and_then(|value| url::Url::parse(&value).ok()?.password().map(str::to_owned))
        .ok_or("no token in the proxy's env")?;
    let ready = serde_json::json!({
        "port": handle.port(),
        "browser_port": handle.browser_port(),
        "token": token,
        "browser_token": handle.browser_credential().map(|c| c.password().to_owned()),
    });
    line(&mut io::stdout(), &ready);
    io::stdout().flush()?;
    // Run until killed: wait on a channel whose sender stays alive.
    let (keep, never) = mpsc::channel::<()>();
    match never.recv() {
        Ok(()) | Err(mpsc::RecvError) => {}
    }
    drop(keep);
    drop(handle);
    Ok(())
}
