# fuzz

cargo-fuzz targets for the parsers that read input we don't control. `.github/workflows/fuzz.yml`
runs them every night; its header says how the job works and where its findings go.

This folder is its own cargo workspace, with its own `Cargo.lock`, so the repo's workspace never
builds it and stable builds never need nightly. `rust-toolchain.toml` here pins the nightly that
libFuzzer needs. Run cargo from this folder so rustup picks that toolchain.

## Targets

Each target's `//!` header gives its input format, the crate items it drives and the invariants
it asserts. All of them go through public APIs only.

| target | what it feeds |
|---|---|
| `acp_stdout` | agent stdout bytes into `clauseo_acp::connect`, through a handshake and a turn |
| `protocol_decode` | the wire types from JSON and MessagePack, and the id parsers |
| `matters_objects` | a matter's loose objects, replaced on disk, then checkpoint and undo |
| `matters_read` | `.clauseo/matter.json` and the stat cache, read by `Matter::open` |
| `store_read` | stored rows, overwritten in the SQLite file, read back through `Store` |
| `settings_parse` | settings.json, policy.json and a UI patch, through `Effective` |
| `reducer_snapshot` | raw snapshot bytes into `Snapshot::from_bytes`, then a fold |

`fuzz_targets/scratch.rs` is a module the three disk-backed targets share, not a target.

## Run them

Install cargo-fuzz at the version `CARGO_FUZZ_VERSION` names in `.github/workflows/fuzz.yml`:

```sh
cargo install --locked cargo-fuzz --version <version>
```

Then, from this folder:

```sh
cargo fuzz build --fuzz-dir . -O --debug-assertions
./nightly.sh 60                  # every target for 60 s, as the nightly job runs them
./nightly.sh 300 store_read      # one target
```

`nightly.sh` grows `corpus/<target>` from `seeds/<target>`. It writes findings to `artifacts/` and
a summary to `out/`. Those three folders are ignored by git. Set `FUZZ_JOBS` to run fewer targets
at once than there are cores. Its header says which findings it reports and which it withholds.
`./test-nightly.sh` checks that with stand-in fuzzers in about a second, and the nightly job runs
it first; run it after any change to `nightly.sh`.

To check one input, such as a crash input downloaded from a nightly run's artifacts:

```sh
cargo fuzz run --fuzz-dir . -O --debug-assertions <target> <file>
```

## Seeds

`seeds/<target>` holds at most 150 files and 1 MiB per target. They were picked from the corpora of
earlier fuzzing campaigns: a libFuzzer merge kept the inputs that add coverage, and the ones adding
the most features stayed. The nightly corpus is an artifact of the newest fuzz run on main (see the
workflow's header). The seeds only start it when there is none. To refresh a target's seeds, merge a grown corpus into an empty folder
with `-merge=1` and keep the result under the same caps.

## Cargo.lock

`Cargo.lock` here started as a copy of the root `Cargo.lock`, so the crates the harnesses share with
the app are fuzzed at the versions the app ships. It does not follow the root lock by itself: when
main changes a shared version, copy `../Cargo.lock` over it and run `cargo fetch` here. The nightly
job builds with this lock as it is (`cargo fetch --locked`).

## When a crate's API changes

A harness that no longer compiles fails the nightly job at its build step, and the job opens an
issue. Port the harness in the same pull request as the API change. Keep it on public APIs. When an
invariant no longer holds because the behaviour changed on purpose, change the invariant and its
line in the target's header together.
