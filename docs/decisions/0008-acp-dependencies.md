# 0008: acp dependencies

## Context

`acp` is the one wrapper over `agent-client-protocol` 3.3.0. The crates table listed only
`protocol` and `agent-client-protocol` under "May depend on", but the crate must run on whatever
executor the host gives it, without spawning a task or naming `tokio`, and it puts its own line
filter in front of the SDK's `Lines` transport. That needs:

- `futures` 0.3.32: the `AsyncRead` and `AsyncWrite` traits the pipes implement, the 64 KiB
  `BufReader` over the agent's stdout, the oneshot that answers each call, and `select` in the
  driver's command loop. The SDK is built on it too.
- `async-channel` 2.5 (`std`): the bounded event channel of 1,024 events the host drains, the
  unbounded command channel, and the frame tap.
- `serde` 1: `serde::de::IgnoredAny`, which checks a line is JSON without building a value, and
  `Deserialize` over the SDK's untyped messages.
- `serde_json` 1.0.154: the SDK hands its messages over as `serde_json::Value`; `_meta` is a
  JSON object.
- `memchr` 2.8: finds the newline in each read, instead of a byte-by-byte scan.

The table lists a crate's normal dependencies other than `thiserror`.

## Decision

The `acp` row's "May depend on" cell is `protocol, agent-client-protocol, futures, async-channel,
serde, serde_json, memchr`. "Never depends on" stays `tauri, tokio`. Its line for
`agent-client-protocol` turns on exactly `unstable_session_fork`, `unstable_llm_providers`,
`unstable_end_turn_token_usage` and `unstable_plan_operations`, the features Claude Code and Codex
use; never `unstable_subagents`, `unstable`, `schemars`, `process` or `stdio`, since Clauseo
spawns agents and owns the pipes. Its dev-dependencies (`clauseo-fake_agent`, `async-process`,
`uuid`) are not listed, as for every crate.

## Consequences

`acp` stays free of any executor: the host spawns the `Driver` future on its own. A later SDK
major changes only this crate. Cargo unifies features across the workspace, so the four unstable
schema features reach `fake_agent` too in a workspace build: with `unstable_plan_operations` on,
`plan_update` and `plan_removed` decode where they did not before.

## Status

accepted

## Date

2026-10-26
