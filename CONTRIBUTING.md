# Contributing

Clauseo is GPL-3.0. Most code here is written by coding agents and checked by people.

## Steps

1. **Pick a task.** Each task has a brief: goal, files, checks.
2. **Give the brief to your agent.** One agent per task.
3. **Work in a worktree.** `git worktree add ../clauseo-<task> -b <task>`. Never share a worktree or
   a `target/` folder between agents.
4. **Commit with sign-off.** `git commit -s` on every commit (the DCO,
   https://developercertificate.org). The `Signed-off-by` email must match the author email.
5. **Run the check.** `cargo xtask check` must pass.
6. **Open a pull request** with the template filled in, including the brief's checks and their output.
7. **Get one approval.** Someone other than the author approves. Changes under these paths also need
   their code owner (see `.github/CODEOWNERS`): `crates/protocol`, `crates/reducer`, `crates/store`,
   `crates/matters`, `crates/host`, `crates/acp`, `crates/sandbox`.
8. **Merge** once CI is green.

## If a check fails

- Every `cargo xtask check` failure prints `file:line: what` and a `fix:` line. Do what it says.
- Missing sign-off: `git rebase --signoff origin/main && git push --force-with-lease`.

## Licence

Contributions are licensed under the GPL-3.0, as in `LICENSE`. Code copied from another project keeps
its licence text in `LICENSES/` and an SPDX header in each copied file.
