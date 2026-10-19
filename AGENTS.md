# AGENTS.md

Clauseo is a desktop ACP client for lawyers: a Rust session host, a thin Tauri 2 shell and a React UI.
The lawyer brings their own agent (Claude Code or Codex). This file is for coding agents and people
alike. `CLAUDE.md` imports it, so Claude Code and Codex read the same rules.

Words: "agent" means the lawyer's ACP agent (Claude Code, Codex). "You" means the coding agent reading
this. Use the words in `GLOSSARY.md` exactly.

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
- `cargo xtask setup`: installs the pinned tools and lefthook.
- `pnpm -C ui install --frozen-lockfile`: frontend dependencies.

## Map

<!-- One line per crate or package that exists. Planned crates are in ARCHITECTURE.md. -->
- `crates/desktop`: the Tauri shell. Blank for now.
- `ui/app`: the React app. Empty for now. Tauri APIs only under `ui/app/src/transport/`.
- `xtask`: checks, build timing, budgets. Builds without any app crate.

## Never

- Never add `tauri` or `tokio` to a core crate (see "Never depends on" in `ARCHITECTURE.md`). Tauri is
  about 90% of cold build time, and the core must run without a window.
- Never add a Tauri plugin or grant a webview permission. The plugin set is fixed (each change forces
  a near-cold rebuild), and every permission widens what an XSS can reach.
- Never write a sync `#[tauri::command]` or stream through Tauri events. Sync commands block the UI
  thread; events go to every webview. Use async commands and one `ipc::Channel` per window.
- Never read or write the real app data folder, `~/.claude` or `~/.codex`. Those hold a lawyer's data
  and agent credentials.
- Never kill a process by name. Kill only the PID you started.
- Never open a `.env` file or print a secret.
- Never create or edit CLA.md or anything in cla/. Signing is done by the person, never by an agent.

## Rules the checks enforce

<!-- Each phrased as a trap. Details live in the failure's `fix:` line. -->
- A crate never depends on what `ARCHITECTURE.md` lists under "Never depends on".
- Capability files never contain `remote`, `core:default`, `fs:`, `shell:`, `http:`, `opener:`
  or `dialog:`.
- Only `#[expect(lint, reason = "...")]` may silence a lint. `#[allow]` fails.
- No `unwrap`, `expect`, `panic!`, `todo!` or `println!` outside tests and `main`.
- Files stay under 600 lines.
- Component files in `ui/` export only components.

## Testing

- Tests are deterministic: no sleeps, no network, no real agent. A scripted `fake_agent` arrives with
  the core crates.

## Performance

- Budgets live in `budgets.toml`. Never trade a budget for tidiness.
- The blank app is the control for every memory and start-up budget. Keep it blank.
- Take every speed or memory win, however small. Lawyers keep Clauseo open while they work, and the
  same few paths run all day: streaming an agent's answer to the screen, saving each event, versioning
  the matter after every turn, reopening long conversations. A few percent saved on one of those paths
  repeats thousands of times a day, and many small savings together are what make the app feel
  instant, the way a car maker that shaves a few grams off every part ends up with a much lighter car.
  So when two ways to do something both meet the brief, measure both and keep the faster one, even
  when the gap looks too small to notice. List each comparison in the pull request under Performance:
  what you compared, the machine, the medians, and what you kept.

## How you work

- One worktree per task, on its own branch. Never share `target/` between worktrees.
- Commit with `git commit -s`.
- Run `cargo xtask check --quick` before you report.
- Report what you verified, with the command and its result, and what you did not verify.
- In the pull request, list every difference from the brief (or the issue, when there is no brief)
  under Deviations and every code path you never ran (Windows, SIGKILL, a missing tool) under Not
  verified. Update both after each review.
- Give each fact one home: a version, a key list, a policy string, a type, a rule's wording. Derive
  the rest from it; where two files cannot share it, add a test that fails when they disagree.
- Touch only the files your brief lists. If the brief is wrong or silent, stop and say so.
- Do not merge. A person reviews and merges.

## Definition of done

- `cargo xtask check` passes and CI is green.
- Every acceptance check in the brief passes, with output in the pull request.
- Tests sit at the interface and include a failing case.
- Docs you changed still pass the doc checks.
- Every comment, doc, help text, test name and PR sentence about what you changed is still true,
  and claims only what a test or command showed.

## Changing this file

Propose additions in your pull request. Add a line only if it is non-obvious, keeps coming up, and
tells the reader what to do. Rules are traps, not maps. Stay under 120 lines and 8 KiB.
