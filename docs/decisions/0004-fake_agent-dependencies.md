# 0004: fake_agent dependencies

## Context

`fake_agent` is the scripted ACP agent that other crates' tests start over stdio. It must stay free
of every Clauseo crate, `protocol` included, so that it tests the ACP wrapper from the outside. The
crates table listed only `agent-client-protocol` under "May depend on". The crate also needs `serde`
(derive, for the scenario format) and `serde_json` (scenario lines and the JSON-RPC lines it reads
and writes). The table lists a crate's normal dependencies other than `thiserror`.

## Decision

The `fake_agent` row's "May depend on" cell is `agent-client-protocol, serde, serde_json`. Its
normal dependencies are those three and `thiserror`, with no other dependency of any kind, and the
`agent-client-protocol` line turns on no feature. The "Never depends on" cell stays empty. The test
`manifest_has_only_allowed_dependencies` guards the rule instead: it reads the crate's own
`Cargo.toml` and fails on any other dependency (normal, dev or build) and on any feature turned on
for `agent-client-protocol`.

## Consequences

A new dependency for `fake_agent` changes this row, that test and a decision record together. A
dependency on a Clauseo crate fails the test, not the dependency check, so the rule holds only while
the test does. Cargo unifies features across the workspace, so `fake_agent` compiles with whatever
SDK features other crates turn on; the test proves the manifest, not that `fake_agent` stays on the
stable schema. The SDK always turns on `serde_json`'s `preserve_order` feature, which then reaches
every crate in a workspace build: a test must not depend on the key order of a `serde_json::Value`.

## Status

proposed

## Date

2026-10-19
