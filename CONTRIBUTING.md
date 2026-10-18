# Contributing

Clauseo is GPL-3.0-or-later. Most code here is written by coding agents and checked by people.

## Steps

1. **Pick a task.** Each task has a brief: goal, files, checks.
2. **Give the brief to your agent.** One agent per task.
3. **Work in a worktree.** `git worktree add ../clauseo-<task> -b <task>`. Never share a worktree or
   a `target/` folder between agents.
4. **Commit with sign-off.** `git commit -s` on every commit (the DCO,
   https://developercertificate.org). The `Signed-off-by` email must match the author email.
   The DCO workflow, `.github/workflows/dco.yml`, fails a pull request with a commit that lacks it.
5. **Sign the CLA** (outside contributors, on your first pull request). Add your signature file as
   described in `cla/README.md`. The founders are covered by their agreement with the company.
6. **Run the check.** `cargo xtask check` must pass.
7. **Open a pull request** with the template filled in, including the brief's checks and their output.
8. **Get one approval.** Someone other than the author approves. A change to a path in
   `.github/CODEOWNERS` also needs that path's code owner.
9. **Merge** once CI is green.

## If a check fails

- Every `cargo xtask check` failure prints `file:line: what` and a `fix:` line. Do what it says.
- Missing sign-off: `git rebase --signoff origin/main && git push --force-with-lease`.

## Licence

Contributions are licensed under the GPL-3.0-or-later, as in `LICENSE`. Code copied from another project keeps
its licence text in `LICENSES/` and an SPDX header in each copied file.
