# 0051: The sandbox in the host

## Context

Stage C built the agent sandbox in pieces: the policy, tiers and launchers in `crates/sandbox`
(0038), Windows tier B (0049) and the egress proxy (S16). Nothing in the host uses them yet:
every agent runs as the lawyer, at tier C, in the adapters' ask modes. Task R17 ("the sandbox in
host") puts each agent in the sandbox its computer's tier allows, gives each matter its own state
folder, runs the auto modes inside the sandbox and the ask modes outside it, shows each matter's
protection in the window, and lets a firm require the sandbox.

R17 is too large for one review, so it lands in five pull requests. This record holds R17's
design, which stage C reviews, and the choices of the first pull request (PR-A: the wire and the
settings, no change to any launch).

## Decision

### The design (R17's "Decided here")

- **Layout.** `<data_dir>/agent-state/<matter_id>/<agent_id>/` is a launch's state folder, on
  every OS and tier; the login homes are `<data_dir>/agents/<agent>/home`
  (`Locations::agent_home`). Threads from before R17 get a new agent session: `session/load`
  fails in the new home and the host falls back, as it does today. The host never follows a link
  when it reads files the agent can write there (`browser/`, `DevToolsActivePort`, downloads).
  The matter's notes folder is `<data_dir>/agent-state/<matter_id>/notes/`. R17 PR-D1 moves
  Claude's state folders to `<data_dir>/claude-state/<matter_id>/<agent_id>/` and the notes to
  `<data_dir>/notes/<matter_id>/`; Codex's state folders stay (below).
- **Logins.** No credential is copied or read. Codex: each state folder is `CODEX_HOME`, with
  `auth.json` hard-linked from the login home; a reused login whose file id changed, or whose
  source is missing, refuses the launch with no re-link; Clauseo's own login is re-linked;
  `HostHandle::remove_login_links(agent)` removes the links and restarts nothing. Claude: no
  link; each state folder is `CLAUDE_CONFIG_DIR`, with `CLAUDE_SECURESTORAGE_CONFIG_DIR` per login
  source. The state folder's config holds Clauseo's keys and nothing else, and is restored when
  the agent edits it. No link is ever made in a matter folder, and a matter folder inside
  `<data_dir>/agent-state` is refused (`matter.folder_refused`).
- **Seams.** `HostConfig::with_sandbox(Sandbox { detect, setup })` (R17 PR-B1, below; R17
  PR-B2 adds `launchers`), where
  `SandboxLaunchers::launcher(policy, matter, opts, denials)` returns a launcher and the names
  set aside before launch (one `Notice` names them); and `HostConfig::with_egress_proxy`, with
  `open`, `port_for`, `env`, `allow`, `revoke` and `close` per `SessionKey { matter, thread,
  launch }`. The desktop app implements both over `crates/sandbox`. The policy is built, and the
  launcher called, off the host thread. Without the seams everything runs as today.
- **Launch.** Pick the tier; make the state folder, the links and the config; open the proxy with
  the login's allowlist, `network.allowed_domains`, the matter's remembered sites and the seed list;
  build the policy; launch. In tiers A and B the proxy and CA variables (`NODE_EXTRA_CA_CERTS`,
  `SSL_CERT_FILE`) join the launch environment, and the agent runs in an auto mode; in tier C the
  proxy is opened but the agent's environment does not point at it, and the agent runs in an ask
  mode by default, in an auto mode only when the lawyer picks one and the firm allows it
  (`sandbox.allow_unprotected_auto`, below). A state folder is readable by its own matter's sandbox
  and no other.
- **Settings.** Sections `sandbox` and `network`, every key a security key (below).
- **Wire.** Below. The bridge admits `HostQuery::Protection`, `RetryProtection` and
  `RemoveSandboxSetup` from the main and the settings windows, with the administrator prompt as
  the native confirm (`command.invalid` without a set-up seam), and carries
  `ProtectionChanged` and `ProtectionViewChanged` to both. The sandbox crate's `NoSandbox` maps
  to a cause: `UnsupportedOs` and `NotBuilt` to `Unsupported` (with its `why`), `ProbeFailed`
  and `SetUpFailed` to `SetUpFailed`, the rest by name.
- **Without the sandbox.** What a matter whose protection is `NotProtected` gets depends
  only on `sandbox.required`:
  - **false:** for any cause, the agents run at tier C, in an ask mode by default; in an auto mode
    only when the lawyer picks one and `sandbox.allow_unprotected_auto` is true. Nothing is refused,
    and the matter shows `NotProtected { cause }`.
  - **true:** never a silent fallback. `NeedsSetUp` and `SetUpRefused`, which running the
    set-up fixes, refuse with `sandbox.setup_required`; every other cause (`Unsupported`,
    `SetUpFailed`, `PoolFull`, `SwitchedOff`, `Unknown`) with `sandbox.required`. Both at
    `CreateThread` and at a turn's launch.

  R17 PR-B1 builds the refusals; PR-A only fixed the docs and the codes' meanings to it. Under
  false, B1 changes nothing: tier C's ask-mode limits are PR-D's.
- **A protected link.** `clauseo_sandbox::Error::ProtectedLink { at }` maps to
  `matter.protected_link` (stage C's #522), with `HostError.files = [at]` (`at` as a
  matter-relative `MatterPath`) and no new field; the window fills its `{path}` from
  `files[0]`. It is built where the host first calls the sandbox (PR-D and PR-E).
- **The end of a session.** The host waits for the agent's process group to be gone before it
  closes the session's proxy and relay. A descendant that calls `setsid` or `setpgid` leaves
  the group and is not covered by that wait: Chrome is covered by the CDP check (and later by
  its own group from the guard); anything else is the sandbox's job, in PR-D and PR-E.
- **The cap.** When the tree is not confirmed gone within the cap (20 s), the session's proxy
  port, relay port, run folder and (tier B, PR-E) pool user stay held until quit, never freed
  under a process that may still live (fail closed), with one warning logged.

### The wire

Additive under 0002's rules: none of these types is logged, so none needs a `schema_version`
bump.

- `ProtectionTier { A, B, C }`, closed: the threat model's three tiers.
- `Protection { Protected { tier }, NotProtected { cause } }`, closed: a matter runs in the
  sandbox or it does not. `Protected` is only ever at tier A or B; the host never sends
  `Protected { tier: C }`.
- `NotProtectedCause`, **open** (`open_data_enum!`, catch-all `Unknown`), since more causes will
  come: `Unsupported { why }`, `NeedsSetUp`, `SetUpRefused`, `SetUpFailed`, `PoolFull`,
  `SwitchedOff { at }`. `why` is an open `UnsupportedWhy` (`os`, `windows_arm`, `not_built`;
  `Unknown` for a newer one). PR-A had `Unsupported` as a unit variant (`"unsupported"`); R17
  PR-B1 made it a data variant (`{"unsupported": {"why": "os"}}`), which changed its JSON. No
  released build had PR-A's form (it merged the same day), so no reader of it exists.
- **`PoolFull`** is new against R17's plan: tier B's pool can answer that every pool user is taken
  (S15 PR4), and that matter then has no sandbox (above for what follows). It carries counts only,
  never a SID or an account name: `size` (pool users in all), `open` (held by another live matter's
  open session), `held` (held by a matter that still has them, no session open), `owed` (a
  permission hand-back is owed, which clears at the next launch) and `set_aside` (they still own
  set-aside items). The host logs the SIDs, on the host only, later.
- `HostQuery::Protection` answers `QueryResult::Protection(ProtectionView { tier, computer,
  required, unprotected_auto_allowed, in_progress, retry_asks_admin })`: the computer's best tier
  now, its protection state, the two `sandbox` settings, the work running on the sandbox
  (`ProtectionWork`: `set_up`, `remove`, `check`; open; left out when none), and whether
  `RetryProtection` runs the elevated set-up (both R17 PR-B1). `tier` is `computer`'s tier when protected and C when not; the
  host builds the view with `ProtectionView::new(computer, ..)`, which takes `tier` from
  `computer` and debug-asserts there is no `Protected { tier: C }`, so the two never disagree.
- `MatterSummary::protection: Option<Protection>`, left out when `None`: not known yet, or an
  older host.
- `HostEvent::ProtectionChanged { matter, protection }`: an app event like `CaseLawHint`,
  never logged and in no stream, so the reducer never folds it. A catch-up carries none:
  `ListMatters` gives each matter's current protection.
- `HostEvent::ProtectionViewChanged { view }` (R17 PR-B1): the whole view after any change of
  it. An app event like `ProtectionChanged`, never logged, in no stream and never folded (no
  `REDUCER_SCHEMA_VERSION` bump); as for it, a catch-up carries none: `HostQuery::Protection`
  gives the current view, so a window reads it when it shows the view and after a lag.
- `HostCommand::RetryProtection {}` and `HostCommand::RemoveSandboxSetup {}`, no receipt.
- Error codes, after every existing one: `sandbox.setup_refused` and `sandbox.setup_failed`
  (retryable), `sandbox.setup_required` and `sandbox.required` (not retryable, and only
  under `sandbox.required`, as above), each a row in 0003.

### R17 PR-B1: the protection state and the set-up commands

PR-B is split in two. **B1** (this section) holds the protection state, the query's real
answer, the two commands, the events, `sandbox.required`'s refusals and the windows'
admission. It changes no launch: an agent starts exactly as before, apart from
`sandbox.required`'s refusal. **R17 PR-B2** routes launches through the launcher seam: it adds
`Sandbox::launchers`, the policy built off the host thread, and the set-aside notices.

- **The seam.** `HostConfig::with_sandbox(Sandbox::new(detect, setup))`, where `detect` is an
  `Arc<dyn Fn() -> clauseo_sandbox::Detection + Send + Sync>` and `setup` an
  `Option<Arc<dyn clauseo_sandbox::SandboxSetup>>`. `Sandbox` is non-exhaustive, so B2's
  `launchers` is additive. Desktop will pass `move || clauseo_sandbox::detect(setup.as_deref())`
  (PR-D or PR-E); B1 wires nothing in desktop's host start. With no seam the computer is
  `NotProtected { Unsupported { why: not_built } }`: the host never calls
  `clauseo_sandbox::detect` on its own, since on macOS it would say tier A while nothing jails
  the agent. For the same reason desktop passes the seam **only together with the launchers**
  (R17 PR-B2/D/E): the seam alone would make macOS say tier A with no jail behind it, and
  `sandbox.required` would pass.
- **The cache.** The actor holds the computer's protection and `retry_asks_admin`, and never
  detects: `detect` reads files and the registry, and macOS' first call starts `sandbox-exec`.
  The first detection runs in `Host::start`, on the calling thread, before the host serves
  (desktop calls `Host::start` off its UI thread), so the cache is full before any query. Each
  set-up, removal or check detects again on its own thread, and reads `setup.state()` there too.
- **The map.** Tier A or B is `Protected { tier }`. At tier C: `UnsupportedOs` is
  `Unsupported { why: windows_arm }` on an ARM64 Windows build and `Unsupported { why: os }`
  otherwise (a Windows build before 22H2 is `os`; so is an x64 build under ARM64's emulation,
  which the compile-time check cannot tell apart); `NotBuilt` is `Unsupported { why: not_built }`;
  `ProbeFailed` and `SetUpFailed` are `SetUpFailed`, their text logged by kind only, never on the
  wire (0047: it may name a path or an account); `NeedsSetUp` and `SetUpRefused` by name. A tier
  C with no reason, or a reason this host does not know, fails closed as `SetUpFailed`.
- **`retry_asks_admin`** is true when a set-up seam is present and its state is `Needed`,
  `Refused`, `Failed` or `Outdated`; false with no set-up, when `Ready`, and for
  `SetUpByOtherUser`, which a set-up cannot fix.
- **Per matter.** Each matter's protection is the computer's until R18's switch (`SwitchedOff`)
  and PR-E's pool (`PoolFull`): `MatterSummary::protection` carries it, and a change of the
  computer's sends `ProtectionChanged { matter, protection }` for every matter `ListMatters`
  lists.
- **`ProtectionViewChanged`** goes out whenever the view changes: a detection, `in_progress`,
  `retry_asks_admin`, or either setting. The settings are compared after every input the host
  loop takes, so any path that changes them is covered; today no runtime path changes either
  `sandbox` key (no wire command sets them and policy is read at start).
- **`RetryProtection`.** With no seam, `command.invalid`. With one: when `retry_asks_admin`,
  `setup.run()` (`in_progress: set_up`); otherwise a detection only (`in_progress: check`).
  Either runs on a named thread of its own, `clauseo-sandbox-setup`, never on the actor and not
  on the launch prep's pool, whose job wait would time out while the set-up sits at an
  administrator prompt for minutes. Then the thread detects again and reads `state()`; the
  actor updates the cache, sends the view and the matters' `ProtectionChanged`, then answers.
  **The answer is held until the work ends**: `Done` when the computer ends protected, and after
  a check; `sandbox.setup_refused` for `SetupError::Refused`; `sandbox.setup_failed` for
  `SetupError::Failed` (its text logged by kind only), and when `run()` returned `Ok` but the
  computer still is not protected. The view's cause follows the new detection.
- **`RemoveSandboxSetup`.** With no seam, or a seam with no set-up, `command.invalid`. With one:
  `setup.remove()` on the same kind of thread (`in_progress: remove`), then as above; `Done`,
  `sandbox.setup_refused` or `sandbox.setup_failed`. What happens to running tier-B sessions is
  PR-E's (a `TODO(R17-E)` at the call).
- **Join and exclusion.** A second `RetryProtection` while a set-up or a check runs joins it: the
  same answer, and no second `run()`; a second `RemoveSandboxSetup` joins a removal. A removal
  while a set-up runs, or a retry while a removal runs, is `command.invalid` at once.
- **Shutdown.** Shutdown never waits for the thread, since the elevated step runs as a process
  of its own. Held answers get `Closed` (`internal` on the wire), the thread's late result is
  dropped, and nothing is sent after close.
- **`sandbox.required`.** `CreateThread` and a turn's launch are refused as "Without the
  sandbox" says. The launch check sits beside the sign-in refusals (`sign_in::launch_refusal`,
  after them), so a queued turn fails with the code and starts no agent.

**A residual: a hung detection.** Neither `detect` nor `state()` has a time bound. A hang at
start blocks `Host::start`; a hang after a set-up, removal or check leaves that work running,
so every later command of its kind joins it and never gets an answer (and the other kind is
refused). Stage C's `state()` only reads files and the registry, and macOS' probe runs
`sandbox-exec` on `/usr/bin/true`, so neither is expected to hang; no bound is built in B1.

**A residual: a removal in progress.** While `remove()` runs, the cache still says
`Protected { tier: B }`; PR-E must refuse launches while a removal runs (a `TODO(R17-E)` at
the call).

### The shared-login identity check (R17 PR-C2)

From the orchestrator's rulings, built in PR-C2 and live with PR-C; a release blocker.

- An agent login's identity comes only from the agent's own status output: Clauseo never reads
  or parses a token (ruling 4). It is cached by the login file's id and mtime, and checked again
  when either changes, for running sessions too.
- Desktop records and watches the identity (stage D); the host enforces it.
- `agent.identity_checking` is a waited launch refusal: retryable, retried quietly.
- `agent.identity_unknown` refuses the launch, retryable.
- `signin.account_changed` (not retryable) stops every agent on that login with
  `HostHandle::stop_agents_for(agent, StopFor::SignInAgain)`, so their questions stay queued,
  held as `sign_in`, and run after the new sign-in.
- `identity.json` keeps a SHA-256 of the identity, never the identity itself.

**A residual.** If desktop is killed or crashes, a jailed descendant of an agent can survive with
its loopback grant to that session's proxy and relay ports, which a later session may bind. The
crash-survivor sweep closes it: each agent's process group id and its leader's start time are
recorded in `run/<session>/`, and at the next start the host `killpg`s each group that still
matches, before any port is claimed. It lands right after #532 and is a release blocker.

### The settings and their safe values

- `sandbox.required` (default false) and `sandbox.allow_unprotected_auto` (default true);
  `network.upstream_proxy` (an `http` or `https` URL with no user name or password; unset means
  the OS's proxy), `network.ca_certs` (agent id to an absolute path) and
  `network.allowed_domains` (host names, `*.` for the names under one).
- All five are in `SECURITY_KEYS`. On an unreadable policy source, or a policy value that does
  not type, `refuse` locks `allow_unprotected_auto` at false, the proxy unset, and the CA map and
  domain list empty: the lawyer's value never stands in for the firm's unknown one. So does a
  policy `sandbox` or `network` section that is not an object (`"sandbox": "on"`), for that
  section's keys. A mistyped `allow_unprotected_auto` in the lawyer's own `settings.json` also
  falls to false, its safe value, not its default (with a warning, no lock).
- **`sandbox.required` locks at its default, false.** Locking it at true would stop every agent
  on a computer at tier C (Linux, Windows before its set-up) whenever one policy file fails to
  parse, for firms that never asked for the sandbox. With `allow_unprotected_auto` locked false,
  an agent outside the sandbox runs only in the ask modes, which is the threat model's tier C
  stance, so the fail-closed rests there. The cost: a firm that did require the sandbox gets ask
  modes at tier C instead of no agent while its source is unreadable; and a weaker readable
  source's `required: true` behind a stronger unreadable one does not apply, since the stronger
  source's lock wins (0009's order).
- `network.allowed_domains` is policy only (`POLICY_ONLY_KEYS`): it is honoured only as the
  firm's, so a `settings.json` value is ignored with a warning. A bad entry in `ca_certs` or
  `allowed_domains` is left out with a warning by id or place, never quoting the value; leaving
  one out trusts or allows less, never more.
- Under `sandbox.required` the account-limits reader does not run (it would run outside the
  jail) until R17 PR-D/E runs it in the jail: no reader starts, each read fails at once, and the
  plan-limits line shows unavailable (stale, if limits were stored before).
- The keys are read and locked from PR-A; `docs/firm-it.md` lists them under "Coming" until the
  host acts on them.

### The five pull requests

1. **PR-A**: the wire and the settings (this record). The host answers the query and both
   commands `command.invalid` ("not built yet"), `MatterSummary::protection` is always `None`,
   and the bridge refuses the query and both commands. No launch changes.
2. **PR-B**, in two: **B1**, the protection state, the query's real answer,
   `MatterSummary::protection`, the two commands, the events, `sandbox.required`'s refusals and
   the bridge's admission (above); **R17 PR-B2**, the launcher seam, the policy and the set-aside
   notices.
3. **PR-C**: the state folders and the logins.
4. **PR-D** and 5. **PR-E**: the launch in the sandbox with the proxy, tier B's pool users
   (PR-E) and the settings' enforcement, as stage C agrees.

The orchestrator's conditions, which the later pull requests must meet:

- one sign-in serves every matter (PR-C): the per-matter state folders link to, or read
  through, the one login home, never a sign-in per matter;
- a per-matter state fence (PR-C): sessions are isolated, so each session, Claude's and
  Codex's alike, sees only its own matter's state folder, never another matter's;
- a state folder is made in under 50 ms at launch (PR-C).

## Consequences

- The window can build its Protected and Not protected states against a fixed wire before the
  host fills it; from R17 PR-B1 every row has a `protection`, and until desktop wires the seam
  it is `Unsupported { why: not_built }`.
- A newer cause decodes to `NotProtectedCause::Unknown`, which the window shows as Not protected.
  A new tier or a third `Protection` variant is a wire change with a plan for the clients
  (0002's freeze rules for view and query types).
- Adding a cause needs only its row in `open_data_enum!`, its sample, a `docs/ui-states.md`
  row and its words.
- Firm IT can set the keys now; nothing acts on them until the host's sandbox reads them.
- Residual of PR-C's Codex fence (rust-v0.162.1): Codex finds rg only through PATH
  (`linux-sandbox/src/bwrap.rs:901`, `sandboxing/src/bwrap.rs:193-206`). The `@openai/codex`
  package Clauseo installs ships `vendor/<triple>/codex-path/rg`, which Codex puts first on its
  PATH (`arg0/src/lib.rs:160-171`). If that copy is missing, Codex's fallback walker lists
  symlinks and masks their targets (bwrap.rs:1124, :930-932). A hostile agent could then plant a
  link that hides a host file, such as /bin/sh, from other matters' sandboxes. PR-D: on Linux,
  refuse the fence when `codex-path/rg` is missing.
- Residual of PR-C's Codex fence: a matter created while another matter's Codex session runs
  stays readable to that session's shell (its transcripts, logs and notes; its logins are
  denied) until the session ends. Neither shape closes it:
  - Deny globs covering every matter's files would count every transcript, log and notes file
    against Codex's 8192-match cap (`MAX_UNREADABLE_GLOB_MATCHES`). Past that cap, every
    sandboxed command fails (closed). The globs therefore name login files only.
  - A deny of `agent-state` whole needs the own `codex-acp/tmp/arg0` to stay readable and never
    writable. But Codex's per-turn rebuild keeps only the deny entries and the writable roots
    (`from_legacy_sandbox_policy_preserving_deny_entries`), so a read-only own `tmp/arg0` is
    dropped. Making it writable reopens round 3's sandbox escape.

  The globs' scan is capped at depth 4 from `agent-state` (`glob_scan_max_depth`, the deepest
  legitimate match being `<matter>/codex-acp/shell_snapshots/<file>`), so a matter can make at
  most 24 names match, plus its live shell snapshots, and cannot push the other matters past
  the cap.

  Follow-up, a must-do: PR-D must move Codex's home (or at least `tmp/arg0`) out of
  `agent-state`, with a migration for existing installs, so `agent-state` is denied whole.
- R17 PR-D1, the layout for the jail: Claude's state and the notes leave `agent-state`, moved at
  the host's start (`agent/state_folder/layout.rs`), so the jail can deny `agent-state` but the
  session's own Codex home and grant the rest exactly. Codex's homes stay, since Codex's SQLite
  stores absolute paths into them.
  - **Downgrades.** The store's schema 6 (`0006_agent_layout.sql`, a marker with no data) lands
    before any folder moves, so a build from before the move, whose Codex fence does not deny
    `<data_dir>/notes`, refuses the data folder (`store.newer_version`) instead of running.
  - **Known limit of test builds:** Claude threads created while a test build from before the
    move ran stay in the old `agent-state/<matter>/claude-acp`, inside the fences; the new
    folder is used and the old one is never merged. Their notes are merged at the next start,
    but for clashes and links.
  - **Links.** An old folder that is a link is never moved, and its agent's launch is refused.
    Old notes that hold a link move entry by entry, the links left behind, since a relative link
    would name another place one level up.
  - **ripgrep.** On Linux, a Codex launch runs only with the `rg` of the pinned `@openai/codex`
    package, found by absolute path in Node's lookup order and checked by SHA-256, beside the
    `codex-package.json` Codex ships (the file through which Codex finds `codex-path`); `PATH` is
    never searched. The residual above about a missing `codex-path/rg` is closed. Codex then
    runs that `rg` in place for the whole session: it puts `codex-path` first on its `PATH` from
    its own executable's folder, and no setting names another `rg` (codex-rs rust-v0.162.1
    `install-context/src/lib.rs:297`, `arg0/src/lib.rs:181, 458-467`), so a checked copy
    elsewhere would never run. Instead Codex's install is read-only to every agent: Codex's
    shell may only read it (`codex_fence/tests.rs`), each jail grants it read and run at most
    (`launch_checks/sandbox/tests.rs`, the Seatbelt snapshot
    `with_a_runtime_path_in_the_data_dir`, `acl_plan/tests/data.rs`), and Claude's deny rules
    name `agents/codex-acp/**` for Read and Edit.
  - **Residual: swapping ripgrep mid-session.** On Linux (no jail yet), any Bash command Claude
    runs could replace `rg` during a Codex session, including renaming a parent folder or
    creating the install folder before Clauseo installs it. Only Claude's deny rules and the
    host's auto-deny stand in the way. A process running as the user outside any agent can
    replace it too; that is outside the threat model.

## Status

proposed

## Date

2026-11-06
