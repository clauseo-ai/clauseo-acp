//! `cargo xtask setup --list`: prints every pin, as text or as JSON, and installs nothing. Its
//! format is written out once, in `--help` (the `list` field of `Commands::Setup` in `main.rs`).

use super::{BINARIES, Pins, TOOLS, install};
use crate::output::Printer;
use std::io::Write;
use std::process::ExitCode;

/// What `setup --list` prints: lines of tab-separated fields, or one JSON object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ListFormat {
    Text,
    Json,
}

/// Prints every pin to `out` in `format`, as `--help` writes out, and installs nothing. Fails when
/// a pinned sha256 has no release asset to belong to, saying why on `err` and printing nothing on
/// `out`. Production passes `Printer::stdout()` and `Printer::stderr()`.
pub(crate) fn list<W: Write, E: Write>(
    out: &mut Printer<W>,
    err: &mut Printer<E>,
    format: ListFormat,
) -> ExitCode {
    let pins = Pins {
        tools: TOOLS,
        binaries: BINARIES,
    };
    if list_with(out, err, &pins, format) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `list` on `pins`. Returns whether every pin has a release asset; when one has none, it prints
/// that failure on `err` and nothing on `out`.
pub(super) fn list_with<W: Write, E: Write>(
    out: &mut Printer<W>,
    err: &mut Printer<E>,
    pins: &Pins<'_>,
    format: ListFormat,
) -> bool {
    let mut assets = Vec::new();
    let mut all_named = true;
    for b in pins.binaries {
        for pin in b.sha256 {
            if let Some(download) = install::pinned_asset(b, pin.os, pin.arch) {
                assets.push((b, pin, download));
            } else {
                err.failure(
                    &format!(
                        "setup --list: {} pins a sha256 for {}-{}, but names no release asset there",
                        b.name, pin.os, pin.arch
                    ),
                    &format!(
                        "return that asset's file name from {}'s `asset` function in \
                         xtask/src/setup.rs, or remove the pin",
                        b.name
                    ),
                );
                all_named = false;
            }
        }
    }
    if !all_named {
        return false;
    }
    match format {
        ListFormat::Text => {
            for t in pins.tools {
                out.note(format_args!("tool\t{}\t{}", t.name, t.version));
            }
            for (b, pin, download) in &assets {
                out.note(format_args!(
                    "binary\t{}\t{}\t{}\t{}\t{}\t{}",
                    b.name, b.version, pin.os, pin.arch, download.sha256, download.url
                ));
            }
        }
        ListFormat::Json => {
            let tools: Vec<serde_json::Value> = (pins.tools.iter())
                .map(|t| serde_json::json!({ "name": t.name, "version": t.version }))
                .collect();
            let binaries: Vec<serde_json::Value> = (assets.iter())
                .map(|(b, pin, download)| {
                    serde_json::json!({
                        "name": b.name,
                        "version": b.version,
                        "os": pin.os,
                        "arch": pin.arch,
                        "sha256": download.sha256,
                        "asset": download.asset,
                        "url": download.url,
                    })
                })
                .collect();
            out.note(serde_json::json!({ "tools": tools, "binaries": binaries }));
        }
    }
    true
}
