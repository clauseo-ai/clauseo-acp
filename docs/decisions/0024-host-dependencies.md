# 0024: host may depend on sha2, for the Codex hook's trust hash

## Context

The pre-write guard (clauseo-acp-private#162) runs as Codex's `PreToolUse` hook too, through
`clauseo_host::codex_hook_config`, which R15 merges into `CODEX_CONFIG`. Codex 0.162.1 runs a hook
from session flags only when it is trusted, and the value can trust itself with a `state` entry
whose `trusted_hash` is the hash Codex computes for the hook: SHA-256 over the normalised hook
identity as sorted, compact JSON (`hooks/src/engine/discovery.rs` `hook_hash`,
`config/src/fingerprint.rs` `version_for_toml`). The hook's command holds the path of the
installed app, so the hash is known only on the lawyer's machine and has to be computed at run
time. No crate `host` may depend on computes SHA-256.

## Decision

- `host` may depend on `sha2`, one line in the workspace dependencies: `sha2 = { version =
  "0.10.9", default-features = false }`. 0.10.9 is the version already in `Cargo.lock` (other
  crates bring it in), so the lock file gains no new package, and it is the version Codex 0.162.1
  hashes with.
- It is used only in `crates/host/src/agent/loads/codex.rs`, to compute that hash.

## Consequences

- `sha2` is pure Rust (RustCrypto), with no I/O, no clock and no threads; without its default
  features it builds without `std`. It adds nothing that `host`'s other rules forbid.
- The port has to follow Codex's hash. Its tests pin it to Codex's own fingerprint test and to
  hashes from a reference program built on Codex 0.162.1's own types. A Codex release that
  changes the hash fails no test here: R15's live check, `hooks/list` reporting the hook
  `trusted`, is where that shows.
- The "May depend on" cell of the `host` row in `ARCHITECTURE.md` names `sha2`.

## Status

proposed

## Date

2026-11-02
