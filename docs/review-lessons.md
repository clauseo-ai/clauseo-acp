# Review lessons

Rules that came out of code review. Each stops a defect that review found more than once, or one
that cost a whole extra round. The most common are in `AGENTS.md`; these are the rest.

"Rules" stay judgment. "Until the checks land" lists what review still enforces by hand. When a
check starts enforcing one of those lines, the pull request that lands the check deletes the line
here and adds the rule to "Rules the checks enforce" in `AGENTS.md`.

## Rules

- Every failure message and `fix:` line is true on every path that reaches it, and doing what the
  fix says never loses work. Test the exact text for each cause. Never make up a `file:line`; a
  failure with no file prints its message and the fix.
  Why: a fix line told the reader to copy a backup over a file whose newer edit the check had just
  kept (PR #6).
- Before code or a comment says how a tool reads a setting (an env var, a config key, a cargo
  feature, which files it loads), check it against the pinned version: run it verbosely or read its
  source. Put the command or the source line in the pull request.
  Why: cargo turns incremental on only for `CARGO_INCREMENTAL=1`, and Tauri reads only json, json5
  and toml capability files; both were first guessed wrong (PR #6, PR #8).
- When meeting the brief word for word still leaves a cheap gap (a config file that skips a step, a
  comment that satisfies a search, a check that passes on a broken build), close it if it is inside
  your files, and otherwise name it in the pull request.
  Why: six review comments in PR #5 and PR #8 found a gap that the brief's wording allowed.
- Model states that exclude each other as one enum, and give "nothing" its own variant. An empty
  list, string or interface never carries a meaning.
  Why: an empty package list meant every package to cargo (PR #5).
- Code that finds a program or a path handles Windows: `pnpm.cmd` beside `pnpm.exe`, PATHEXT order,
  backslashes, and `\\?\` prefixes from `canonicalize`. It honours overrides such as `CARGO_HOME`.
  Add a test with a Windows-style input, and list the path under Not verified until a Windows run.
  Why: pnpm lookup and `~/.cargo/bin` would have failed, or run another program, on Windows or with
  `CARGO_HOME` set (PR #6, PR #8).
- Test side effects through an injected runner on a scratch directory (`xtask/src/scratch.rs`), not
  only the pure functions around them.
  Why: all 17 buildtime tests were on pure functions, so none checked the file's bytes after a
  failing cargo or a spawn error (PR #6).
- Find steps and rows by name, not by position.
  Why: a test took the ui steps as the slice `[7..13]`, so a new step before them would break it
  with a slice error that says nothing useful (PR #8).
- Build fixtures in the test, through the real tool, rather than committing captured output.
  Why: 649 lines of captured `cargo metadata` that nothing said how to refresh, and that tested a
  captured file instead of how cargo resolves the renames (PR #8).

## Until the checks land

- Test each new check against the ways around it: a rename, another spelling or escape, another
  file or config the tool also reads, another API that does the same thing. Prefer clippy
  `disallowed-methods`, cargo-deny or a real parser over a text scan.
  Replaced by: the planted-violation test, which fails any check that ships no bypass forms.
  Why: PR #8 took five rounds; each found a new way around the last fix.
- Before you report, break what each new check, test or probe guards (remove it, move it, empty it)
  and watch it fail.
  Replaced by: mutation testing of the diff with cargo-mutants.
  Why: a launch check and a compiler test passed with the thing they guard removed (PR #8).
- Only buildtime edits a tracked file today. Its guard is `xtask/src/buildtime/probe/guard.rs` in
  Rust and `ui/app/scripts/guard.mjs` in scripts. The next tool that needs to edit a tracked
  file moves the guard out of buildtime instead of copying it, and never writes a second guard.
  Replaced by: clippy `disallowed-methods` and an oxlint import ban on file-writing calls outside
  the guard.
  Why: the second tool that edited a file repeated the defects the first had fixed (PR #6, PR #8).
- Every child process has a timeout, and stopping it stops its whole process tree, also when the
  parent gets SIGKILL. Test the stop on a real tree with a wrapper that starts a grandchild, as
  `ui/app/scripts/buildtime-proc.test.mjs` does.
  Replaced by: a process API in `xtask/src/process.rs` that cannot start a child without a timeout
  and always kills the whole tree.
  Why: it took three rounds in PR #8 to stop tsc's native binary and Vite.
- Tests never touch the network.
  Replaced by: CI running the tests with the network cut off.
  Why: `cargo test -p xtask` quietly downloaded crates (PR #8).
