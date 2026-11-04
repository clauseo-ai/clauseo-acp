//! Done-when 2 of S18: Clauseo never names an agent's credential file in its code.
//!
//! The test fails if any file under `crates/agents/src` names Claude's credential file, or names
//! Codex's login file outside `src/signin/reuse.rs`, anywhere but in a comment (string literals
//! count); if a literal spells a dot with an escape, or an `"auth"` literal has `json` or
//! `with_extension` beside it (the name built in pieces); and if `reuse.rs` calls `File::open`,
//! any `fs::read*`, `OpenOptions`, the crate's own `open_read` or `read_small`, any `read_…(`,
//! `options(` or `open(`, or names `File` at all. `reuse.rs` (the hard link, the stat and the
//! markers) may name Codex's file to link it and stat it, never to open it.
//!
//! The scan is defence in depth, not proof (decision 0046): it reads text, and code written to
//! get around it can still pass. The file names are built with `concat!` below, so this file
//! does not name them itself.

#[cfg(test)]
#[path = "credential_names/scan.rs"]
mod scan;
