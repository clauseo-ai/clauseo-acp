# 0053: Clauseo's unsafe-site block list

## Context

The egress proxy (S16) lets an agent and its browser reach only the hosts on a launch's
allowlist: the login's hosts, the firm's `network.allowed_domains`, the seed list and the sites
the lawyer allowed. Nothing stops an allowed or wildcard-covered host from being a phishing page,
or an agent from being led to a known botnet controller by a page it reads. Rohan made a block
list a launch blocker.

The list covers **phishing domains plus known botnet command-and-control IP addresses**: the
Phishing.Database active domains (MIT, tag `p`) and Feodo Tracker's C2 IPs (CC0, tag `m`, IP
literals). URLhaus, the first choice for malware hosts, is not used: abuse.ch's terms of 4
November 2025 put its exports behind a key and no longer allow commercial use. Stage F publishes
the list from the public repo `clauseo-ai/blocklist` every 6 hours as the release `current`, with
two assets, `blocklist-v1.txt.gz` and `blocklist-v1.txt.gz.minisig`. The format is stage F's
published spec (v1): a sorted `<host> <tag>` list under three comment lines, signed with minisign,
with the trusted comment `clauseo-blocklist v1 generated=<RFC 3339 UTC> entries=<n>`. The first
real list has 391,962 entries (391,961 `p`, 1 `m`); the spec allows 2,000,000 entries and
64 MiB.

PR 1 built the list and its enforcement; PR 2 built the lawyer's "Allow this site" override at
the core (protocol, host, desktop bridge and proxy, below); its UI comes after.

## Decision

### Layout

- **`crates/blocklist` (`clauseo-blocklist`)**, new and pure: the format's parse, the minisign
  check, matching, the rollback rule and `Current`, the list in use. Its row bans any I/O, the
  network crates, `host` and `sandbox`, so it has the core crates' `forbid` line and its own
  `clippy.toml`. It depends on `minisign-verify` (already in the lock, through the updater
  plugin), `flate2` and `serde` (the tag and time travel in the proxy's JSON denials).
- **`sandbox`** depends on it for enforcement only: the proxy receives a parsed list, never
  bytes or a signature.
- **`host`** depends on it for the update path: fetching, verifying, storing, the bundled snapshot
  and the lawyer's notice. The HTTP client is the connectors crate's ureq set-up (rustls with the
  OS trust store), made reusable, so no new network dependency.

### The signature and the keys

- A list loads only if its minisign signature over the exact `.gz` bytes verifies against one key
  of `PUBLIC_KEYS`, a compiled-in slice of minisign public key files
  (`crates/blocklist/keys/*.pub`, read with `include_str!`; today `stage-f.pub`, stage F's key,
  id E8E787F0560990DA, the same file stage F's release job verifies against), and its trusted
  comment is exactly the v1 form. `entries` must equal the entry lines; the `sources` comment is
  opaque and never parsed.
- Only prehashed signatures (`ED`, minisign's default since 0.8 and rsign2's) verify; the legacy
  `Ed` form is refused, and a test holds that.
- **Rotation:** a release adds the new key as a file in `crates/blocklist/keys/` and its entry in
  `PUBLIC_KEYS` beside the old one (stage F's job verifies against the same file); once that
  release is the oldest one supported, stage F signs with the new key; a later release drops the
  old key and its file.
  The secret key exists only in the repo's GitHub secrets. If it is lost, a release ships a new
  key, and until users have it the stored copy and the bundled snapshot keep blocking.

### The update path (host)

- At start the host's `clauseo-blocklist` thread verifies the stored copy
  (`<data_dir>/blocklist/blocklist-v1.txt.gz` and `.minisig`) and the bundled snapshot, and puts
  the newer valid one in use; neither leaves the list unloaded, logged once.
- Then it fetches both assets at once and every 6 hours over the host's own network, never through
  an agent's proxy. The route is asked before each request, each redirect hop included:
  `network.upstream_proxy` when set (as agent installs do), otherwise the connectors' route,
  `HTTPS_PROXY` and its kin, then the OS's proxy settings, PAC included
  (`clauseo_sandbox::upstream_for` with `Upstream::System`). A proxy setting it refuses fails the
  round rather than going around the proxy. TLS trusts the OS's store; `network.ca_certs` is per
  agent and not used here. Redirects are followed by hand, at most 5, https
  only, with no explicit port, and only to `github.com`, `release-assets.githubusercontent.com`
  (where release downloads redirect today) and `objects.githubusercontent.com` (their older
  host). Bodies are capped (64 MiB for the list, 4 KiB for the signature; the list is
  decompressed under the 64 MiB cap, so a gzip bomb stops there), and the list's download may
  take 10 minutes, the signature's 60 s.
- **The off switch:** `network.unsafe_site_list_updates` (default true, policy only, a security
  key that locks at true so an unreadable policy file never stops updates) lets a firm that
  blocks GitHub turn the fetch off. Then the host makes no request at all and enforces the stored
  copy or the bundled snapshot, whichever is newer; the block list stays on. Each round reads
  the settings the actor last published (`Live`), so a settings change the actor publishes
  (`SettingsChanged`) applies from the next round. The policy file is read only when Clauseo
  starts, so a policy change takes effect at the next start; a future runtime reload would
  publish `SettingsChanged` and need nothing more here.
- **Clock checks:** a list whose `generated` is more than a day ahead of the host's clock is
  refused when fetched and set aside when loaded at start, stored copy and bundled snapshot alike
  (a mistaken future date would otherwise freeze updates). A list in use more than 3
  days old (about 12 missed publishes) is stale: the host warns at most once a day, and
  `clauseo_host::block_list_is_stale` reports it for a later surface.
- **Timing:** the 6-hour wait is jittered by up to ±30 minutes; a signature mismatch (stage F's
  publish window) is retried once after 10 minutes, then the normal interval resumes.
- **404 on either asset** (no `current` release yet) is "no update": logged at info, retried at
  the next interval. **Any other failure**, a signature that does not verify included, keeps the
  list in use and retries at the next interval (after a signature mismatch, once after 10
  minutes), logged quietly with no backoff growth: stage F's two assets are not replaced at once,
  so a new `.gz` can sit beside the old `.minisig` for a moment, and that pair is refused.
- **Rollback:** a list replaces the one in use only when its `generated` is newer than the list in
  use's and not older than the bundled snapshot's (`accept`). The check and the swap happen under
  one write lock (`Current::offer`). A verified newer list is put in use, then stored: each
  file goes to a synced temporary file renamed over the old one, the list first and the
  signature last. A crash between the two renames leaves a pair that fails verification, so the
  next start uses the bundled snapshot and the next fetch writes a good pair.
- **The bundled snapshot:** release builds carry the latest list as a Tauri resource at
  `<resources>/blocklist/blocklist-v1.txt.gz` and `blocklist-v1.txt.gz.minisig` (the desktop
  reads `app.path().resource_dir()` joined with `blocklist`); stage F's release workflow
  downloads `current` before the build and maps it there in `bundle.resources`. A dev build
  without it runs with no list until the first fetch succeeds.

### How the list reaches the proxy

- `Current` holds an `Arc<BlockList>` behind an `RwLock` that guards only the swap. The host and
  every launch's proxy share one `Arc<Current>` (`ProxyConfig::block`). A request takes a clone
  of the `Arc` and matches without a lock; an update swaps the `Arc`. Updates reach running
  sessions at their next request, without a restart, and never close a live tunnel: a tunnel
  opened before a host was listed lives until it ends.

### Matching and its cost

- A request host matches when it equals a listed host or lies under one on a label boundary
  (`evil.example` covers `a.evil.example`, never `notevil.example`). The request side is
  normalised by the proxy's `Name::parse`: case folded, one trailing dot dropped, IDNs in
  punycode. IP literals are covered deliberately and exactly: a dotted IPv4 or bracketed IPv6
  entry matches only that address, in any spelling; an IPv6 address that carries an IPv4 address
  (mapped, translated, compatible, NAT64 or 6to4, the forms the proxy's address check reads)
  also matches that IPv4 entry; a domain never matches an IP entry. No DNS lookup is made for
  the check, and the addresses a name resolves to are not checked against the list: a domain
  pointing at a listed C2 address is not blocked by that entry (stage C may rule on checking the
  resolved addresses, which needs no extra lookup, in a later change).
- The list is four allocations: every host back to back in one string, a `u32` end per entry, a
  tag bit per entry, and an open-addressing hash index of `u32` entry numbers at a load of at
  most 3/4. A lookup is one probe per label. Measured on a release build (8 vCPUs, under load):
  a synthetic 1,000,000-entry list (28.7 MB of text) loads in about 0.76 s and holds 38 MB of
  heap (44 MiB more RSS); the first real list (391,962 entries) verifies in 6 ms, loads in
  0.25 s and holds 14 MB (11 MiB RSS); a lookup of a five-label host takes about 1 µs. Dropping a
  list frees four allocations, so the last request holding an old list never pays for a million
  frees. A sorted `Vec<Box<str>>` or a `HashSet<Box<str>>` would add a 16-byte pointer and an
  allocation per entry (estimated, not measured).
- The parse refuses, and so keeps the list in use: a malformed line, an upper-case host, a host
  with a scheme, port, path or trailing dot, a single-label host (a bare TLD) or an all-digit last
  label, unsorted or duplicate hosts, an entry count that differs from the trusted comment, more
  than 2,000,000 entries or more than 64 MiB.

### Precedence and the block answer

- The proxy's checks, in order: the request's form, the silent list (the deny list: unchanged, it
  wins and reports nothing), **the block list**, the port, a private literal or `localhost`, then
  the allowlist. So the block list beats every allowlist entry, the seed list's included.
- For plain `http://` the URL's host and the `Host` header are both checked (the proxy rewrites
  `Host` from the URL anyway). A listed host in origin-form (a request with no absolute URL) gets
  the `Scheme` denial naming it, not `Blocked`, by design: the proxy carries no origin-form
  request at all.
- A listed host gets 403 in the proxy's usual short answer; plain http's carries
  `Clauseo-Blocked: malware` or `phishing`. A CONNECT is refused with the plain 403, as the other
  refusals are. The agent sees an ordinary refusal.
- PR 2's override is checked in the proxy's one method that asks the list, before the list
  (below).

### The report and the lawyer's notice

- The proxy reports `Denial::Blocked { host, port, listed, tag, generated }` through the launch's
  sink, behind the rate cap every denial has (20 a second, the rest counted), so reporting never
  blocks or fails a request.
- The host turns each into `HostEvent::SiteBlocked { block_id, matter, thread, turn, host, tag,
  list_generated, override_allowed, count }`, an app event like `ProtectionChanged`: never logged,
  in no stream. The same host blocked again in the same turn re-sends the same `block_id` with
  `count` one higher (the `FolderScanned` practice: the same event re-sent with new counts), so
  the thread shows one notice per site per turn: "Blocked <host>: on Clauseo's unsafe-site list
  (malware/phishing), list of <date>". `SiteTag` is an open enum (`Malware`, `Phishing`,
  `Unknown`). `override_allowed` is the policy key `network.allow_unsafe_site_override`
  (default true; a security key that locks at false).
- The host logs a host's first refusal in a turn only (`count` 1), at info, by host, tag and
  list date, so a looping agent writes one line per site per turn there. The proxy logs each
  refusal by host, as it logs its other refusals.
- R17 PR-D's proxy sink calls `HostHandle::site_blocked(session, site)` for each
  `Denial::Blocked`; until it lands no production proxy runs.

### A missing or unverified list

- The proxy runs without a list (fails open) and logs once per process that the block list is not
  loaded. The host keeps the last good copy, so the only windows are a dev build without the
  snapshot, the first start before the stored copy or the snapshot has been read (the load runs
  on the block list thread, not on the start-up path), and a release built without the snapshot.
  Signature checking stays in the host; the proxy only receives a parsed list.

### PR 2: the lawyer's override (built)

- **Commands:** `HostCommand::AllowBlockedSite { block_id, scope }` and
  `HostCommand::RevokeAllowedSite { host, scope, matter }`, with `scope` either `Matter` or
  `Always` (`AllowScope`) and `matter` named exactly for `Matter`; and
  `HostQuery::AllowedSites`, answering `QueryResult::AllowedSites`: each site's `host`,
  `scope`, `matter` (for `Matter`) and `allowed_at`, `Always` first. Both commands answer
  `Done` with no receipt.
- **What an allow covers:** the exact host the notice named, as the proxy printed it, never a
  host under it. The host keeps the notices it sent (`block_id` to host and matter,
  `actor/allowed.rs`), each thread's latest 64 and 1024 in all; an allow names one by
  `block_id`, so a client cannot pick the host, and an id the host no longer keeps (or never
  sent) is `blocklist.notice_expired` (0003).
- **Who may send them:** the commands reach the host only through `HostHandle::dispatch`,
  which the desktop bridge alone calls; it admits by window label (`bridge/admit.rs`, 0019):
  `AllowBlockedSite` from `main`, `RevokeAllowedSite` and the query from `main` and
  `settings`, nothing from any other label (another window or a webview), with
  `command.invalid`. No agent, tool call or MCP sidecar has a path to a host command, and the
  `block_id` reaches app subscribers only. `bridge/tests/allowed_sites.rs` tests each window.
- **Policy:** while `network.allow_unsafe_site_override` is false, `AllowBlockedSite` is
  refused with `blocklist.override_off` (0003) and no stored allow is honoured: every view is
  empty, and nothing is deleted. Revoking and listing work either way.
- **Storage:** both scopes in the host's own `<data_dir>/blocklist/allowed.json`
  (`{"v":1,"always":{host: ms},"matters":{id: {host: ms}}}`), written whole through
  `write::replace_private_file`, never in `settings.json`, with the stored list's discipline
  (`blocklist/disk.rs`): leftover temp files swept first, the folder synced after. Read at
  start, capped at 4 MiB and 10,000 allows (the rest ignored, logged); a damaged or larger file
  counts as no allows (logged). 10,000 allows at their longest (a 253-byte host, each in its own
  matter) take about 3.0 MiB. A new allow past 10,000 is refused with `command.invalid` (a count
  past its most, 0003) and changes nothing. Each `AllowedSite` says whether it is `saved`:
  whether the next start would honour it.
- **Integrity** (`blocklist/allowed/integrity.rs`, security review fix 5): the lowercase hex
  sha256 of the file's bytes as last written is kept in the OS secret store
  (`clauseo_connectors::SecretStore`; the desktop's keychain under its own service
  `ai.clauseo.desktop.blocklist`, apart from the connectors', account
  `blocklist-allowed-sha256`), handed to the host by `BlockListSeams::with_secrets`. The host
  asks the store once, at the start, and trusts the file only when its bytes hash to the item.
  - **One thread of its own** (`clauseo-allowed`, `actor/allowed/saving.rs`) does the read
    (the store's `get` and the file) and then every save, in order, never the actor, so a
    Keychain prompt holds up only the allowed sites, never the host or the app. Until the read
    is back every view is empty and `AllowedSites` lists none; an allow or revoke sent
    meanwhile is held and run, in order, once it is. A thread the OS refuses, or a read not
    back within 30 s (a prompt left open; the host clock's timer), counts as no store for the
    run, logged once; a read that lands later is ignored. `AllowedSites::loaded` tells
    whether the read is in. A command held, or waiting on its save, when the host stops is
    dropped with the stop.
  - **At the host's stop** the queue gets up to 2 s (host clock) to drain. A revoke still
    unsaved then deletes the stored hash as the stop's last step, on a thread of its own waited
    on for at most 1 s more (a Keychain prompt cannot hold the stop past 3 s in all), so the
    next start honours no allows rather than the revoked site; allows still queued are lost
    (they were never saved). The thread is told first to write nothing more: a save it
    finishes later writes no file and deletes the hash it just set (its state may equal the
    old file's, a repeat allow or a revoke that removed nothing), so no matching pair remains.
  - No item while the file exists, a hash that does not match (an edit, or an older copy put
    back: the item holds the latest hash, so a rollback fails too), means no allows, logged.
    No file is no allows, whatever the item.
  - No store (no seam, Linux's `Unsupported`), a refusal (a macOS Keychain "Deny", say after
    an update) or any other store error means no allows for the run, logged once; the store is
    not asked again that run (but for one delete, below), so a denied prompt never repeats. The
    run then keeps what the lawyer allows and revokes in memory only and writes nothing.
  - **An allow or revoke takes effect in memory at once,** and is answered once its save is:
    `Done` when saved. Each save is the whole state: **the hash first, then the file**. A hash
    not set writes nothing (the old pair stays whole) and turns the run to writing nothing. A
    file not written after its hash leaves a mismatch, as does a crash between the two: the
    next start has no allows. Both fail closed: the worst outcome is the sites blocked again.
  - **A save that fails is visible:** an allow answers `blocklist.allow_not_saved` (it lasts
    until Clauseo closes). A revoke whose save fails while the stored pair may still hold the
    site **deletes the stored hash** (`SecretStore::delete`), so the next start honours no
    allows rather than the revoked one, and answers `Done`; in a run that writes nothing the
    delete is tried once. If the delete fails too, it answers `blocklist.revoke_not_saved`:
    the site may be allowed again after a restart. With no store at all (Linux, no seam) no
    start honours an allow, so a revoke answers `Done`; allows still answer
    `blocklist.allow_not_saved`. Both codes are errors whose change still
    applied (0003), not retryable within the run.
  - A file the start rejected (unreadable, over the cap, unparsable, no item, a mismatch) is
    moved to `allowed.json.rejected-<UTC time, YYYYMMDDTHHMMSSZ>` beside it (`-<n>` after a
    taken name) at the run's first write, never at the start, and logged with the reason and
    the new name, so the old allows and the evidence of tampering survive; only the latest 3
    are kept. The move never replaces a file (a hard link, then the old name removed), so the
    free-name check has no race. Where the folder takes no hard link (FAT, exFAT, some network
    shares) it checks the name is free and renames instead, a small race accepted there alone
    (only from inside the fenced folder). A move that fails overwrites nothing and the run
    writes nothing more. The temp-file sweep (`allowed.json.clauseo-*.new`) never matches them, and
    the agent's fence denies `blocklist/`.
  - **Strength, per OS.** macOS: real; the Keychain item trusts only the app that made it, and
    any other process (a script running `security`) gets a prompt the lawyer must allow.
    Windows tier B: real; the agent runs as a pool user, a different account, which cannot read
    the owner's Credential Manager. Windows tier C: obfuscation only; Credential Manager has no
    per-app access control, so any process of the same user can read **and write** the item,
    and an unsandboxed agent could forge the file and its hash together. Linux: none; there is
    no store, so allows never outlive a run (not a product target).
- **Enforcement:** `ProxyConfig::allowed` (`clauseo_blocklist::Allowed`, empty by default) is
  asked in `Ctx::blocked` (`proxy/server/blocked.rs`) before the list, by each name the
  request carries; an allowed name skips the list only, and still meets the silent list, the
  port, the private-address rule and the allowlist. `clauseo_host::AllowedSites::for_matter`
  gives a launch the live view for its matter (its `Matter` allows and every `Always` one),
  which the actor swaps after each allow, revoke and `SettingsChanged`, so a change reaches a
  running proxy at its next request. The desktop keeps the shared `AllowedSites` as
  `AllowedSitesInUse` beside `BlockListInUse`; R17 PR-D passes
  `allowed.for_matter(<launch's matter>)` to `ProxyConfig::allowed`.
- **Logging:** each allow and revoke logs one info line: the host, the scope and the matter id.

#### PR 2 decisions

- **Both scopes in one host file.** No per-matter memory of sites exists yet; the host's
  per-matter state outside the store is a data-folder file keyed by matter id
  (`connectors/hints.json`), so `Matter` allows follow it, beside `Always` ones.
- **`RevokeAllowedSite` names the matter** of a `Matter` allow (`matter`, required for
  `Matter` and refused for `Always`), since one host can be allowed in several matters.
- **The code is `blocklist.override_off`,** not `settings.locked`: the key may be off by policy
  or in `settings.json`, and the window needs to tell this refusal apart.
- **Changes apply before their save, answered after it:** the save runs off the actor, so the
  views change at once and the reply says whether the change outlives the run
  (`blocklist.allow_not_saved`, `blocklist.revoke_not_saved`), never `store.io`.
- **Every allow and revoke saves the whole state,** even when nothing changed in memory, so
  retrying one whose save failed puts it right once the store and the disk work.
- **Allowing again keeps the first `allowed_at`;** revoking what is not there answers `Done`.
- **The integrity check is a hash in the secret store, not a MAC in the file:** sha256 is
  already the host's (0024), so no crate is added; the item is replaced at each save, so it
  refuses a rollback as a MAC alone would not. While the store is unusable, allows last one run.
- **The agent's fence denies `blocklist/`** (`agent/data_fence.rs`'s `DENIED` for Claude,
  `agent/codex_fence.rs`'s `DATA_DENIED` for Codex) whether or not it exists at the launch, so
  no agent reads or edits the stored list or the lawyer's allows.
- **Notices are kept per thread** (the latest 64 of each, 1024 in all), so an agent looping in
  one thread cannot push out another thread's notice before the lawyer acts on it; none is
  dropped when its turn or session ends. An expired one is `blocklist.notice_expired`, which
  the window words as "open the site again".
- **A forgotten matter's allows are kept,** as its import brings the matter back.
- **The views follow every `SettingsChanged` the actor publishes** (the same hook as the
  updater's `Live`), so a settings change applies at the next request after it. The policy file
  and `settings.json` are read only when Clauseo starts, and no command writes
  `network.allow_unsafe_site_override`, so a change to it takes effect at the next start; a
  future runtime reload would publish `SettingsChanged` and needs nothing more here.
- **A revoke closes the open tunnels and forwards to a host the list still names,** as the
  allowlist's revoke does: `Allowed::set` tells its watchers the hosts it removed, and the
  proxy aborts each tunnel or forwarded request to one that is not allowed again and still
  listed. A request that passed the list before the revoke and opens its tunnel after it is
  refused as listed (the proxy compares `Allowed::generation` under its tunnel lock). An
  allow is stored as `Name::text` prints the host, an IPv6 address without brackets. A list
  update still closes nothing.

## Consequences

- A phishing host or a C2 address is refused for the agent's fetches and the browser even when a
  wildcard or the seed list allows it, with one notice per site per turn.
- Running sessions pick up a new list at their next request; open tunnels are not cut.
- Key rotation needs two releases' overlap; a lost key needs an app release.
- A signed list dated up to a day ahead is accepted and holds back lists dated before it until
  that time passes; only the key holder can cause it.
- The list adds its measured memory to every running app; the blank app's budgets see it only once
  a list is loaded (the snapshot, in release builds).
- Stage F: download `current` before each release build and map the two files to
  `blocklist/` in `bundle.resources`; a listed resource that is missing fails the build, so the
  mapping lands with the download step.

## Status

proposed

## Date

2026-11-07
