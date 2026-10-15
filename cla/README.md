# Signing the Clauseo CLA

Read [CLA.md](../CLA.md) first. The CLA check (`.github/workflows/cla.yml`) reads the files in your pull
request, so your signature goes in the same pull request as your first change. Both sign-offs are
needed: the commits (`git commit -s`) and this file.

## Individuals

1. Copy `cla/TEMPLATE-individual.md` to `cla/signatures/<github-login>.md`, using your exact GitHub login in lower case.
2. Fill in Name, GitHub login, Email and Date (YYYY-MM-DD).
3. Include the file in your pull request.

## Entities

1. Copy `cla/TEMPLATE-entity.md` to `cla/entities/<entity-short-name>.md`.
2. Fill it in and list every covered GitHub login as a line `- @login`.
3. Changes to `cla/entities/` need approval from the company (see `.github/CODEOWNERS`).

A new version of the CLA means signing again.
