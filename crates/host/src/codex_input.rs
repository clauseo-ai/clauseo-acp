//! Codex's own cap on a turn's input, counted exactly as Codex counts it, so the host can refuse
//! a `ChatGPT` question Codex would refuse before anything is sent (decision 0057). Codex's
//! refusal reaches the wire only as "Codex encountered an internal error." (recorded,
//! codex-acp 2.2.2), so without this check the lawyer would be told nothing.
//!
//! What Codex counts, checked against codex-acp [`CODEX_ACP_VERSION`] and the Codex
//! [`CODEX_VERSION`] it bundles:
//!
//! - The cap, `codex-rs/protocol/src/user_input.rs:9-10` at `rust-v0.162.1`:
//!   `/// Conservative cap so one user message cannot monopolize a large context window.`
//!   `pub const MAX_USER_INPUT_TEXT_CHARS: usize = 1 << 20;`
//! - Applied to the whole turn, summed over its input items, in `turn/start`
//!   (`codex-rs/app-server/src/request_processors/turn_processor.rs:552-556`, same tag):
//!   `let actual_chars = params.input.iter().map(V2UserInput::text_char_count).sum::<usize>()`,
//!   and `turn/steer` (`:515-516`, `validate_v2_input_limit`):
//!   `let actual_chars: usize = items.iter().map(V2UserInput::text_char_count).sum();`
//!   `if actual_chars > MAX_USER_INPUT_TEXT_CHARS {`
//! - Per item (`codex-rs/app-server-protocol/src/protocol/v2/turn.rs:532-541`, same tag):
//!   `UserInput::Text { text, .. } => text.chars().count(),` and 0 for images, audio, skills and
//!   mentions: Unicode scalar values, not bytes and not UTF-16 units.
//! - What codex-acp makes of an ACP prompt (`dist/index.js`, `buildPromptItems`, :35754-35790 at
//!   2.2.2): a `text` block is a text item as it is; a `resource_link` is a text item of
//!   `formatUriAsLink(block.name, block.uri)` (:35808-35818): `[@${name}](${uri})` when the name
//!   is not empty, else for a `file://` URI `[@${fileName}](${uri})` with `fileName` the last
//!   `/` part of the URI without its first `file://`, else the URI itself. That added text
//!   counts too, and is counted here the same way; nothing else is added.
//! - Measured live with codex-acp 2.2.2 on Codex 0.162.1, no request leaving the machine (private
//!   `measurements/long-question/recordings/codex-boundary/README.md`): 1,048,576 ASCII
//!   characters are accepted and 1,048,577 refused (`actual_chars`); 1,048,576 emoji (4 MiB) and
//!   1,048,576 Devanagari letters are accepted; a link named `paper.txt` with a 44-character URI
//!   adds exactly 58, and with it the boundary holds to one character either way.
//!
//! The host sends a prompt as `Text` blocks and, for each file, a `ResourceLink` with the file's
//! name (`turn/steps.rs`, `prompt_blocks`). Any other block counts 0 here, the smaller count, so
//! the check never refuses what Codex would take. Re-check all of the above, and change
//! [`CODEX_ACP_VERSION`] and [`CODEX_VERSION`], on every codex-acp bump: a test fails when the
//! pinned versions move.

use clauseo_protocol::ContentBlock;

/// Codex's `MAX_USER_INPUT_TEXT_CHARS` (`1 << 20`): the most Unicode scalar values a turn's text
/// items may hold together, as counted by [`codex_input_chars`]. One over is refused.
pub const CODEX_MAX_INPUT_CHARS: usize = 1_048_576;

/// The codex-acp version (`@agentclientprotocol/codex-acp`) [`CODEX_MAX_INPUT_CHARS`] and
/// [`codex_input_chars`] were checked against, as Clauseo pins it
/// (`crates/limits/tests/live/package.json` and its lock, which the agents install runs `npm ci`
/// on).
pub const CODEX_ACP_VERSION: &str = "2.2.2";

/// The Codex (`@openai/codex`) version installed with codex-acp [`CODEX_ACP_VERSION`], at whose
/// tag (`rust-v0.162.1`) the cap was read. codex-acp itself asks for `^0.160.1`; Clauseo pins
/// this version with an `overrides` entry in the same `package.json`.
pub const CODEX_VERSION: &str = "0.162.1";

/// The scalar values Codex counts against [`CODEX_MAX_INPUT_CHARS`] for a prompt of `blocks`:
/// each text block's characters, plus each resource link's text as codex-acp writes it.
pub fn codex_input_chars(blocks: &[ContentBlock]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.chars().count(),
            ContentBlock::ResourceLink { uri, name, .. } => link_chars(name, uri),
            ContentBlock::Image { .. }
            | ContentBlock::Audio { .. }
            | ContentBlock::Resource { .. }
            | ContentBlock::Unsupported { .. } => 0,
        })
        .fold(0, usize::saturating_add)
}

/// The characters of codex-acp's `formatUriAsLink(name, uri)`.
fn link_chars(name: &str, uri: &str) -> usize {
    // `[@` + name + `](` + uri + `)`
    let link = |label: &str| label.chars().count() + uri.chars().count() + 5;
    if !name.is_empty() {
        return link(name);
    }
    match uri.strip_prefix("file://") {
        // JavaScript's `uri.replace("file://", "")` drops the first match, here at the start;
        // `split("/").pop()` is the last part, empty after a trailing `/`.
        Some(path) => link(path.rsplit('/').next().unwrap_or(path)),
        None => uri.chars().count(),
    }
}

/// Whether a prompt of `blocks` is over Codex's cap.
pub fn over_codex_cap(blocks: &[ContentBlock]) -> bool {
    codex_input_chars(blocks) > CODEX_MAX_INPUT_CHARS
}

#[cfg(test)]
mod tests;
