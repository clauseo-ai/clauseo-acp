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
  The matter's notes folder is `<data_dir>/agent-state/<matter_id>/notes/`.
- **Logins.** No credential is copied or read. Codex: each state folder is `CODEX_HOME`, with
  `auth.json` hard-linked from the login home; a reused login whose file id changed, or whose
  source is missing, refuses the launch with no re-link; Clauseo's own login is re-linked;
  `HostHandle::remove_login_links(agent)` removes the links and restarts nothing. Claude: no
  link; each state folder is `CLAUDE_CONFIG_DIR`, with `CLAUDE_SECURESTORAGE_CONFIG_DIR` per login
  source. The state folder's config holds Clauseo's keys and nothing else, and is restored when
  the agent edits it. No link is ever made in a matter folder, and a matter folder inside
  `<data_dir>/agent-state` is refused (`matter.folder_refused`).
- **Seams.** `HostConfig::with_sandbox(Sandbox { detection, launchers, setup })`, where
  `SandboxLaunchers::launcher(policy, matter, opts, denials)` returns a launcher and the names
  set aside before launch (one `Notice` names them); and `HostConfig::with_egress_proxy`, with
  `open`, `port_for`, `env`, `allow`, `revoke` and `close` per `SessionKey { matter, thread,
  launch }`. The desktop app implements both over `crates/sandbox`. The policy is built, and the
  launcher called, off the host thread. Without the seams everything runs as today.
- **Launch.** Pick the tier; make the state folder, the links and the config; open the proxy
  with the login's allowlist, `network.allowed_domains`, the matter's remembered sites and the
  seed list; build the policy; launch. In tiers A and B the proxy and CA variables
  (`NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`) join the launch environment, and the agent runs in an
  auto mode; in tier C the proxy is opened but the agent's environment does not point at it, and
  the agent runs in an ask mode. A state folder is readable by its own matter's sandbox and no
  other.
- **Settings.** Sections `sandbox` and `network`, every key a security key (below).
- **Wire.** Below. The bridge admits `HostQuery::Protection` from the main window, and
  `RetryProtection` and `RemoveSandboxSetup` with the administrator prompt as the native
  confirm (`command.invalid` without a set-up seam). The sandbox crate's `NoSandbox` maps to a
  cause: `UnsupportedOs` and `NotBuilt` to `Unsupported`, `ProbeFailed` and `SetUpFailed` to
  `SetUpFailed`, the rest by name.

### The wire

Additive under 0002's rules: none of these types is logged, so none needs a `schema_version`
bump.

- `ProtectionTier { A, B, C }`, closed: the threat model's three tiers.
- `Protection { Protected { tier }, NotProtected { cause } }`, closed: a matter runs in the
  sandbox or it does not.
- `NotProtectedCause`, **open** (`open_data_enum!`, catch-all `Unknown`), since more causes will
  come: `Unsupported`, `NeedsSetUp`, `SetUpRefused`, `SetUpFailed`, `PoolFull`, `SwitchedOff {
  at }`.
- **`PoolFull`** is new against R17's plan: tier B's pool can answer that every pool user is
  taken (S15 PR4), and that matter then runs at tier C. It carries counts only, never a SID or an
  account name: `size` (pool users in all), `open` (held by another live matter's open
  session), `held` (held by a matter that still has them, no session open), `owed` (a permission
  hand-back is owed, which clears at the next launch) and `set_aside` (they still own set-aside
  items). The host logs the SIDs, on the host only, later.
- `HostQuery::Protection` answers `QueryResult::Protection(ProtectionView { tier, computer,
  required, unprotected_auto_allowed })`: the computer's best tier now, its protection state, and
  the two `sandbox` settings.
- `MatterSummary::protection: Option<Protection>`, left out when `None`: not known yet, or an
  older host.
- `HostEvent::ProtectionChanged { matter, protection }`: an app event like `CaseLawHint`,
  never logged and in no stream, so the reducer never folds it. A catch-up carries none:
  `ListMatters` gives each matter's current protection.
- `HostCommand::RetryProtection {}` and `HostCommand::RemoveSandboxSetup {}`, no receipt.
- Error codes, after every existing one: `sandbox.setup_refused` and `sandbox.setup_failed`
  (retryable), `sandbox.setup_required` and `sandbox.required` (not retryable), each a row in
  0003.

### The settings and their safe values

- `sandbox.required` (default false) and `sandbox.allow_unprotected_auto` (default true);
  `network.upstream_proxy` (an `http` or `https` URL with no user name or password; unset means
  the OS's proxy), `network.ca_certs` (agent id to an absolute path) and
  `network.allowed_domains` (host names, `*.` for the names under one).
- All five are in `SECURITY_KEYS`. On an unreadable policy source, or a policy value that does
  not type, `refuse` locks `allow_unprotected_auto` at false, the proxy unset, and the CA map and
  domain list empty: the lawyer's value never stands in for the firm's unknown one.
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
- The keys are read and locked from PR-A; `docs/firm-it.md` lists them under "Coming" until the
  host acts on them.

### The five pull requests

1. **PR-A**: the wire and the settings (this record). The host answers the query and both
   commands `command.invalid` ("not built yet"), `MatterSummary::protection` is always `None`,
   and the bridge refuses the query and both commands. No launch changes.
2. **PR-B**: the seams, the query's real answer, `MatterSummary::protection`,
   `ProtectionChanged` and the bridge's admission of the query and the commands.
3. to 5. The state folders and logins, the launch in the sandbox with the proxy, and the
   settings' enforcement, in the order stage C agrees.

The orchestrator's conditions, which the later pull requests must meet:

- one sign-in serves every matter: the per-matter state folders link to, or read through, the
  one login home, never a sign-in per matter;
- a per-matter state fence: no matter's sandbox can read another matter's state folder;
- a state folder is made in under 50 ms at launch.

## Consequences

- The window can build its Protected and Not protected states against a fixed wire before the
  host fills it; until PR-B it gets `command.invalid` and no `protection` on any row.
- A newer cause decodes to `NotProtectedCause::Unknown`, which the window shows as Not protected.
  A new tier or a third `Protection` variant is a wire change with a plan for the clients
  (0002's freeze rules for view and query types).
- Adding a cause needs only its row in `open_data_enum!`, its sample, a `docs/ui-states.md`
  row and its words.
- Firm IT can set the keys now; nothing acts on them until the host's sandbox reads them.

## Status

proposed

## Date

2026-11-06
