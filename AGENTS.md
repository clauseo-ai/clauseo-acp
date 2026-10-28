# AGENTS.md

Clauseo is a desktop ACP client for lawyers: a Rust session host, a thin Tauri 2 shell and a React UI.
The lawyer brings their own agent (Claude Code or Codex). This file is for coding agents and people
alike. `CLAUDE.md` imports it, so Claude Code and Codex read the same rules.

Words: "agent" means the lawyer's ACP agent (Claude Code, Codex). "You" means the coding agent reading this. Use the words in `GLOSSARY.md` exactly.

## Read next

- `ARCHITECTURE.md`: crate map, dependency rules, seams, event flow.
- `GLOSSARY.md`: one name per thing.
- `CONTRIBUTING.md`: the steps from task to merge.
- `docs/review-lessons.md`: rules from past reviews, each with the defect it prevents.
- The contract at the top of each crate you touch (`//!` in `lib.rs`).
- Plans and task briefs live in the private repo, cloned next to this one as `../clauseo-acp-private`
  (start at `../clauseo-acp-private/START-HERE.md`). Outside contributors will not have it; this repo
  must make sense without it.

## Commands

<!-- Name only commands that exist. A check fails on any `cargo xtask ...` here that does not parse. -->
- `cargo xtask check --quick`: changed crates only. Run it before every report.
- `cargo xtask check`: exactly CI's fast lane plus this OS's tests.
- `cargo xtask buildtime`: build-loop timings against `budgets.toml`.
- `cargo xtask setup`: installs the pinned cargo tools and checks gitleaks and lefthook.
- `cargo xtask setup --binaries`: also installs the binaries `--only` names (repeatable; all if none).
- `pnpm -C ui install --frozen-lockfile`: frontend dependencies.
- Run every xtask command from the repo root (why: the alias in `.cargo/config.toml`).

## Map

<!-- One line per crate or package that exists. Planned crates are in ARCHITECTURE.md. -->
- `crates/acp`: the ACP client over the agent's pipes; agent traffic as events. Scenarios in `tests/scenarios/`.
- `crates/desktop`: the Tauri shell. Blank for now.
- `crates/fake_agent`: scripted ACP agent for tests; scenarios and their format in `crates/fake_agent/scenarios/` (README.md).
- `crates/matters`: matter folders and their shadow git history (checkpoints, undo). See `docs/decisions/0007-matter-history-format.md`.
- `crates/process`: starts an agent in its own process tree, shuts it down and kills the tree; the `Launcher` seam.
- `crates/protocol`: types crossing the host boundary, no I/O. See `docs/decisions/`, `docs/ui-states.md`.
- `crates/reducer`: the pure fold from events to state and view changes, and snapshots; its gungraun bench is `crates/reducer_bench` (Linux).
- `crates/store`: the event log, snapshots, blobs and receipts in one SQLite file; format in `docs/decisions/0006-store-format.md`.
- `ui/app`: the React app, empty for now; `ui/components`: its tokens, icons, words and controls, checked by ui/app's scripts. Tauri APIs only under `ui/app/src/transport/`.
- `xtask`: checks, build timing, budgets. Builds without any app crate. Frozen: no new checks.

## Never

- Never add `tauri` or `tokio` to a core crate (see "Never depends on" in `ARCHITECTURE.md`). Tauri is
  about 90% of cold build time, and the core must run without a window.
- Never add a Tauri plugin or grant a webview permission. The plugin set is fixed (each change forces
  a near-cold rebuild), and every permission widens what an XSS can reach.
- Never write a sync `#[tauri::command]` or stream through Tauri events. Sync commands block the UI
  thread; events go to every webview. Use async commands and one `ipc::Channel` per window.
- Never read or write the real app data folder, `~/.claude` or `~/.codex`. Those hold a lawyer's data and agent credentials.
- Never kill a process by name. Kill only the PID you started.
- Never open a `.env` file or print a secret.
- Never create or edit CLA.md or anything in cla/. Signing is done by the person, never by an agent.

## Rules the checks enforce

<!-- Each phrased as a trap. Details live in the failure's `fix:` line. -->
- A crate never depends on what `ARCHITECTURE.md` lists under "Never depends on".
- Capability files and the desktop crate's permission files (`permissions/`) never contain `remote`, `core:default`, `fs:`, `shell:`, `http:`, `opener:` or `dialog:`.
- Only `#[expect(lint, reason = "...")]` may silence a lint, never `allow` or `warn` in any form
  (outer, inner, `cfg_attr`, macro bodies). Every `#[expect]` is listed in `xtask/expect-allowlist.txt`.
- A crate that `ARCHITECTURE.md` marks as banning I/O or the clock gets no exception to the bans,
  not even `#[expect]`. What such a crate carries is in the I/O paragraph of `ARCHITECTURE.md`.
- No `unwrap`, `expect`, `panic!`, `todo!` or `println!` outside tests and `main`.
- A source file has at most 600 lines.
- Rust comments stay within 100 columns; a file past 500 lines gets a note to split it.
- A check that inspects nothing fails.
- Files are written only through each crate's write module: `xtask/src/write.rs` in xtask,
  `Scratch` (`xtask/src/scratch.rs`) in xtask's tests, `ui/app/scripts/guard.mjs` in scripts.
  Child processes start only through `xtask/src/process.rs` in xtask and
  `ui/app/scripts/buildtime-proc.mjs` in scripts, each with a timeout and a whole-tree kill.
- Each pinned version has one home.
- Component files in `ui/` export only components.
- Tests never reach the network; CI's Linux job runs them with it cut off.

## Testing

- Tests are deterministic: no sleeps, no real agent. The scripted `fake_agent` stands in for one.

## Performance

- Budgets live in `budgets.toml`. Never trade a budget for tidiness.
- The blank app is the control for every memory and start-up budget. Keep it blank.
- Product code takes every speed or memory win, however small. The same few paths run all day:
  streaming an answer to the screen, saving each event, versioning the matter after every turn,
  reopening long conversations. Many small savings there make the app feel instant, the way a car
  maker that shaves a few grams off every part ends up with a much lighter car. When two ways to do
  something on a product path both meet the brief, measure both and keep the faster, even when the
  gap is tiny. Under Performance, list what you compared, the machine, the medians, what you kept.
- Tooling, CI, docs and tests are exempt: keep them correct and simple, and do not benchmark them.

## How you work

- One worktree per task, on its own branch. Never share `target/` between worktrees.
- Commit with `git commit -s`.
- Report what you verified, with the command and its result, and what you did not verify.
- In the pull request, list every difference from the brief (or the issue, when there is no brief)
  under Deviations and every code path you never ran (Windows, SIGKILL, a missing tool) under Not
  verified. Update both after each review.
- Give each fact one home: a version, a key list, a policy string, a type, a rule's wording. Derive
  the rest from it; where two files cannot share it, add a test that fails when they disagree.
- Touch the files your brief lists. If you need another, change it and list it under Deviations. If the brief is wrong, stop and say so.
- When you review a pull request, follow the review bar in `CONTRIBUTING.md`.
- Do not merge. A person reviews and merges.

## Definition of done

- `cargo xtask check` passes and CI is green.
- Every acceptance check in the brief passes, with output in the pull request.
- Tests sit at the interface and include a failing case.
- Docs you changed still pass the doc checks.
- Every comment, doc, help text, test name and PR sentence about what you changed is still true, and claims only what a test or command showed.

## Changing this file

Propose additions in your pull request. Add a line only if it is non-obvious, keeps coming up, and
tells the reader what to do. Rules are traps, not maps. Keep this file to at most 120 lines and 8 KiB.
