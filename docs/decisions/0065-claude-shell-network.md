# 0065: Claude's shell network

## Context

Claude Code's sandbox (macOS, and Linux for development) sends every sandboxed Bash command's
traffic through its own proxy. Clauseo set `network.allowedDomains: ["*"]` (decision record
0030, "Claude's sandbox"), so a command could reach any host without asking: a prompt injection
in a client's document could `curl` the matter's papers anywhere. The `WebFetch` pull request has
already dropped the bare `WebFetch` allow for the seeded legal sites (`WEB_FETCH_SITES`), so the
shell was the wider door.

What Claude Code 2.1.293 does, read from its binary and `sdk.d.ts`:

- The proxy matches a host against an entry lower-cased and exactly; `*.x` matches subdomains of
  `x` at any depth and never `x`; an entry without a port matches every port. A
  `WebFetch(domain:x)` rule without `*` also matches the URL's host exactly, and `*.x` the same
  way. The seeds hold no `*`.
- An unlisted host raises a `SandboxNetworkAccess` request. In `auto`, the mode a Clauseo session
  launches in, that request goes to Anthropic's classifier and never to the host: no card, and
  the matter's kept sites cannot answer it. In `default` (forced ask) and `acceptEdits` it is a
  card, and one Yes covers that host, on every port and path, for the session.
- `network.strictAllowlist: true` refuses an unlisted host instead of asking. It is honoured
  from `--settings` and gates sandboxed commands only, never `WebFetch`.
- `allowedDomains` is merged with `WebFetch(domain:…)` allow rules from the settings files
  Claude Code loads. Clauseo's seeds go in as `allowedTools` (`cliArg` rules), which that merge
  does not read.

## Decision

- `allowedDomains` is the seeded sites plus `SHELL_NEEDED_HOSTS` (empty until stage C's census;
  a court site's CDN host only where the court site itself is seeded), sorted, each once, no
  wildcard (`agent/loads/sandbox_network.rs`). The plain hosts reach what the seeded `WebFetch`
  rules reach, no wider and no narrower.
- `deniedDomains` (loopback and the any-address) stays. `allowLocalBinding` and
  `allowUnixSockets` stay unset.
- In `auto`: `strictAllowlist: true`, and the matter's kept sites at launch go into the list
  and, as `WebFetch(domain:<host>)` allow rules, beside the seeds. A kept site is fed only as a
  plain lower-case host name. Removing a kept site takes effect, in both `WebFetch` and the
  shell, from the next conversation.
- In forced ask: no strict and no kept sites. An unlisted host is a card; stage B's arm answers
  a kept site's card, so a Remove acts at once.
- A conversation launched in `auto` keeps strict after a switch to forced ask, or a fallback to
  `acceptEdits`, until it is next launched: stricter, not weaker.
- Windows has no Claude sandbox: a shell command's network is judged by Anthropic's classifier
  alone. The host's Windows shell scan is a belt over it (URL tokens and network verbs checked
  against the same list, giving an ask), not a fence.

## Consequences

- A shell command in `auto` reaches the seeded and kept sites and nothing else; a site the
  lawyer has not kept fails as a refused connection, which Claude reports. A kept site is how
  the lawyer opens another one.
- `SHELL_NEEDED_HOSTS` grows only from evidence (stage C's census). Package indexes and code
  hosts stay out unless that census shows legal work needs them.
- A later Claude Code that changes the matching, the `auto` routing of `SandboxNetworkAccess`, or
  `strictAllowlist` must be re-read before the version bump.
- The matter folder's `.claude/settings.json` and `.claude/settings.local.json` cannot widen
  the list: Claude Code loads only the user settings (`settingSources: ["user"]`), and
  `--settings` sets `allowUnsandboxedCommands: false`, under which project settings'
  `allowedDomains` and `WebFetch(domain:…)` rules are ignored too. Nothing of Clauseo's writes a
  settings file into a client's folder (decision record 0063).

## Status

proposed

## Date

2026-11-16
