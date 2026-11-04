//! Done-when 2 of S18: Clauseo never names an agent's credential file in its code.
//!
//! The test fails if any file under `crates/agents/src` names Claude's credential file, or names
//! Codex's login file outside `src/signin/reuse.rs`, anywhere but in a comment (string literals
//! count); and if `reuse.rs` calls `File::open`, any `fs::read*`, `OpenOptions` or the crate's
//! own `open_read`. `reuse.rs` (the hard link, the stat and the markers) may name Codex's file to
//! link it and stat it, never to open it.
//!
//! The file names are built with `concat!` below, so this file does not name them itself.

#[cfg(test)]
#[path = "credential_names/scan.rs"]
mod scan;
