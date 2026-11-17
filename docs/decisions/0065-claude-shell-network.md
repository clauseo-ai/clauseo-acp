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
  launches in, that request goes to Anthropic's classifier and never to the host, so no card
  appears. In `default` (forced ask) and `acceptEdits` it is a card, and one Yes covers that
  host, on every port and path, for the session. Stage C saw this live on 2.1.293.
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
- `strictAllowlist: true` in every mode but forced ask, decided at launch. An unlisted host
  from the shell is refused, never a card: not the classifier's call in `auto`, and not a card
  in the `acceptEdits` fallback. Forced ask keeps the card, one per site.
- A conversation launched outside forced ask keeps strict after a switch to forced ask, until
  it is next launched: stricter, not weaker.
- No per-matter sites go into the list. A per-computer site list, a separate change, will feed
  it later.
- Windows has no Claude sandbox: a shell command's network is judged by Anthropic's classifier
  alone. The host's Windows shell scan is a belt over it (URL tokens and network verbs checked
  against the same list, giving an ask), not a fence.

## Consequences

- Outside forced ask, a shell command reaches the seeded sites and nothing else. Any other site
  fails as a refused connection (exit 56, "host is not on the allow list"), which Claude reports.
- `SHELL_NEEDED_HOSTS` grows only from evidence. Stage C's census found 109 `curl` calls in 19
  of 152 conversations, mostly to unseeded legal sources, which are being added to the seed list.
  Package indexes and code hosts stay out unless a census shows legal work needs them.
- A later Claude Code that changes the matching, the routing of `SandboxNetworkAccess`, or
  `strictAllowlist` must be re-read before the version bump.
- The matter folder's `.claude/settings.json` and `.claude/settings.local.json` cannot widen
  the list: Claude Code loads only the user settings (`settingSources: ["user"]`), and
  `--settings` sets `allowUnsandboxedCommands: false`, under which project settings'
  `allowedDomains` and `WebFetch(domain:…)` rules are ignored too. Claude's own "Yes, and don't
  ask again" option still writes `.claude/settings.local.json` in the matter folder until
  "Always for this matter", kept by the host, replaces it (decision record 0063, a separate
  change).

## Status

proposed

## Date

2026-11-16
