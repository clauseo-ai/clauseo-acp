//! G16's live contract tests: one ignored test per login kind, each reading one real account's
//! plan usage through `ProcessReaders` at the pinned adapters (`tests/live/package.json`).
//!
//! Each test reads its login folder from `CLAUSEO_G16_<KIND>`. With it unset (or empty), the test
//! prints `skipped: no logins (CLAUSEO_G16_<KIND>)` and passes. With it set, it also needs
//! `CLAUSEO_G16_INSTALL`, the folder `npm ci` filled from the pins file, and `CLAUSEO_G16_NODE`,
//! the absolute path of `node`. It writes a `settings.json` into a temp data folder whose agent
//! entry runs the adapter's bin script under the install, loads it with
//! `Effective::load(&Locations::under_root(..), ..)`, and reads once with `PassthroughLauncher`
//! and the real timer. A Claude test points `CLAUDE_CONFIG_DIR` at the login folder through the
//! entry's `env`; a Codex test passes the folder as `ProcessReaders::new`'s `codex_home` (#160).
//!
//! No test reads a credential file or a key. An `_API` test relies on the caller exporting the
//! vendor's key variable (`ANTHROPIC_API_KEY`, `CODEX_API_KEY`) for that test alone, as
//! `nightly.sh` does; Claude Code prefers a key it finds, so the plan tests run without one.
//!
//! Each read's raw stdout, taken through the hidden `ProcessReaders::with_stdout_tap`, is
//! redacted (`live/redact.rs`) and written to `<target>/g16/<test>.jsonl`, then put through the
//! secret check (`fixtures/check.rs`), whose findings are printed. The G16 owner copies a
//! recording into `fixtures/recorded/` by hand once it passes (`fixtures/README.md`).
//!
//! The first release's plan tests are Claude Pro, `ChatGPT` Plus and `ChatGPT` Go; Claude Max and
//! Team stay fixture-only for now (Rohan, 2026-11-03).
//!
//! Run: `cargo nextest run -p clauseo-limits --run-ignored only --no-capture`.

#[cfg(test)]
#[path = "live/write.rs"]
mod write;

#[cfg(test)]
#[path = "fixtures/check.rs"]
mod check;

#[cfg(test)]
#[path = "live/redact.rs"]
mod redact;

#[cfg(test)]
#[path = "live/run.rs"]
mod run;

#[cfg(test)]
use run::{Kind, live};

#[test]
#[ignore = "live: reads a real Claude Pro login (CLAUSEO_G16_CLAUDE_PRO)"]
fn claude_pro() {
    live(Kind::ClaudePro);
}

#[test]
#[ignore = "live: reads a real Claude Team login (CLAUSEO_G16_CLAUDE_TEAM)"]
fn claude_team() {
    live(Kind::ClaudeTeam);
}

#[test]
#[ignore = "live: reads a Claude API-key login (CLAUSEO_G16_CLAUDE_API)"]
fn claude_api() {
    live(Kind::ClaudeApi);
}

#[test]
#[ignore = "live: reads a real ChatGPT Plus login for Codex (CLAUSEO_G16_CODEX_PLUS)"]
fn codex_plus() {
    live(Kind::CodexPlus);
}

#[test]
#[ignore = "live: reads a real ChatGPT Go login for Codex (CLAUSEO_G16_CODEX_GO)"]
fn codex_go() {
    live(Kind::CodexGo);
}

#[test]
#[ignore = "live: reads a Codex API-key login (CLAUSEO_G16_CODEX_API)"]
fn codex_api() {
    live(Kind::CodexApi);
}

#[cfg(test)]
#[path = "live/redact_tests.rs"]
mod redact_tests;
