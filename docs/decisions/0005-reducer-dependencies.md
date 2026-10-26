# 0005: reducer dependencies, and its bench as a package of its own

## Context

`reducer` folds a stream's events into its state and view changes, and `store` keeps the state as
snapshot bytes. The crates table let `reducer` depend on `protocol` only, and listed it as
`planned`. A snapshot needs the state to derive `Serialize` and `Deserialize`, and its bytes are
`MessagePack` with named maps: a 1 MiB state decoded in 681 to 735 µs against 826 to 1,043 µs for
JSON on an Apple M6. `serde` is already in the graph through `protocol`; a snapshot type in
`protocol` instead would freeze the reducer's internals into the event schema.

`reducer`'s row bans any I/O and the clock, so every target root of the crate forbids
`clippy::disallowed_methods` and `clippy::disallowed_types` under the crate's own `clippy.toml`,
and its cfgs use only `test`, `doc` and positive features. Its instruction-count bench uses
gungraun 0.20, whose `main!` macro reads the command line (`std::env::args`) and exits the
process (`std::process::exit`): clippy refuses both in a target of `reducer`, and `forbid` admits
no `#[expect]`. The bench binary must read its arguments, since gungraun's runner calls it back
with them, so no hand-written `main` avoids the ban. gungraun runs only on Linux under Valgrind,
and `cfg(target_os = "linux")` is not a cfg `reducer` may use.

## Decision

The `reducer` row is `exists`, and its "May depend on" cell is `protocol, serde, rmp-serde`
(`serde` with `derive`; `rmp-serde` for the snapshot bytes, pure Rust, no I/O). Its "Never
depends on" cell stays `any I/O, clock, tauri`.

The bench lives in a bench-only package, `crates/reducer_bench` (`clauseo-reducer_bench`): no
library and no normal dependency; `protocol`, `reducer` and, on Linux only, `gungraun` are its
dev-dependencies. Its row is `exists`, "May depend on" `none`, "Never depends on" `tauri`. It is
not held to the I/O bans, so its bench gates gungraun with `cfg(target_os = "linux")` and on other
systems prints that it runs on Linux only. It shares `reducer`'s test fixtures through a `#[path]`
module.

## Consequences

`reducer`'s state types are its snapshot format: any change to a state field or to how it
serialises bumps `REDUCER_SCHEMA_VERSION`, and `host` then rebuilds snapshots from events. A new
dependency for `reducer` changes this row and needs a record. The bench runs with
`cargo bench -p clauseo-reducer_bench --bench apply`; CI's `reducer-bench` job runs it on every
pull request. A normal dependency added to `reducer_bench` would need its row changed too.

## Status

accepted

## Date

2026-10-26
