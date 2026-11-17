# Codex config schema fixture

`config.schema.json` is Codex's own JSON schema for `config.toml`, copied byte for byte from
`codex-rs/core/config.schema.json` at tag `rust-v0.162.1` of `openai/codex` (commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d`, Apache-2.0), the Codex version codex-acp 2.2.2 runs.
SHA-256: `7933a70506e753a6e98c16800d2a76770b2af2b7295491f8f0b8e713e952027e`.

Codex generates it from its `ConfigToml` type with schemars: the `codex-rs/config-schema` crate
(`codex-write-config-schema`, run as `just write-config-schema`) calls
`codex_config::schema::write_config_schema`, and Codex's
`core/src/config/schema_tests.rs` fails when the committed file drifts from the type.

Codex 0.162.1 ignores unknown config keys when it parses a config, so a misspelled key is
dropped without a word; the schema is the one place that lists the keys it reads.
`src/agent/codex/schema_tests.rs` checks every config Clauseo writes for Codex (the home
`config.toml`, each launch's `CODEX_CONFIG` with its hooks, and `clauseo_limits::CODEX_OVERRIDES`)
against it, with unknown keys refused. That test closes the objects the schema leaves open and
describes the two maps serde flattens; its module docs say which.

**Rule:** whenever the pinned Codex version moves, copy `codex-rs/core/config.schema.json` from
the new tag over this file, update the tag, commit and hash above, and run the schema tests:

```text
cp <codex checkout>/codex-rs/core/config.schema.json crates/host/tests/fixtures/codex/
cargo nextest run -p clauseo-host --features test-support -E 'test(/schema/)'
```

# Codex hook input fixture

`hook-session-id.json` is a recording from Codex 0.162.1 under codex-acp 2.2.2 with a mock model
server (16 Nov 2026): the `sessionId` `session/new` returned, then the `PreToolUse` hook input
Codex wrote for a `Bash` call of that session, of another thread of the same app-server, and of a
`multi_agent` child of the session. Folders are shortened and each `tool_input` is replaced by
`ls`; every other field is as Codex wrote it. `src/pre_write_guard/sessions/tests.rs` reads it to
pin that the hook's `session_id` is the ACP session id. Record it again when the pinned Codex or
codex-acp version moves.
