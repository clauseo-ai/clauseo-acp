# 0046: `agents`, sign-in runs, status checks and login reuse

## Context

A lawyer must be signed in to Claude or ChatGPT before the first question, with no terminal
(stage D, S18). Each vendor ships its own login: Claude Code's `auth login` (through
claude-agent-acp's `--cli`) and Codex's `login`. Clauseo runs them hidden, outside the sandbox,
and confirms the result through the vendor's own status. When the lawyer already has a Claude
Code or Codex login, Clauseo says which account it found and reuses it once they choose "Use
it" (Rohan, 2026-11-02). Clauseo never opens or reads a credential (`ARCHITECTURE.md`,
Invariants).

Three facts shape the foundations:
- A `process::LaunchSpec` can add or override a child's variables but never remove one
  (`crates/process/src/spec.rs`). Setting a variable to `""` is not the same as leaving it
  unset: `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` means the lawyer's own store.
- `std::env::set_var` is `unsafe` in edition 2024, and `unsafe_code` is denied in the
  workspace. So a test cannot set `SSH_TTY` in its own process to check that it is removed.
- Codex's reused login is one file under two names (a hard link). Telling whether two names are
  one file needs the file system's id. On Unix, `lstat` gives it and opens nothing. On Windows,
  `std` has no stable call that gives the volume serial number and file index without a handle.

The brief is `tasks/stage-d/S18-agents-signin-and-status.md` (private repository). This record
covers the foundations (S18 piece 2). The flows built on them are listed at the end, each with
its own section to fill when it lands.

## Decision

**The crate.** Sign-in lives in `clauseo-agents`, beside the installs. It adds two dependencies:
- `codex_appserver`, for the types of Codex's `account/read` result (`GetAccountResponse`,
  `ChatgptAccount`);
- `zeroize`, for the pasted code.

`settings` stays a dev-dependency only. The caller passes each agent's home in
`SigninConfig::homes`, as the text `Locations::agent_home(id)` gives, never canonicalised.
Claude keys its login store by that text, so a differently spelt path is a different login.
No `protocol` change: R20 maps the states and codes to the wire.

**The states and codes** are crate-local and the names the brief fixes:
- `SigninState`: `SignedOut { found }`, `Queued`, `Waiting { since, nudge }`,
  `DeviceCode { url, code, expires_at }`, `LoginFound { from, email }`,
  `SignedIn { email, plan, source, at }` and `Failed { code, at }`. It is a separate enum from
  `InstallState`, which already has its own `Waiting`.
- `Found`: `ClaudeSettings`, `ClaudeWindowsSandbox`, `CodexKeyring`, `ReusedFailed`,
  `ReusedSignedOut`, `ReusedChanged` and `CodexLinkSplit`.
- `LoginFrom`: `ClaudeCode` and `Codex`. `Source`: `Reused` and `Own`.
- `SigninCode::code()` gives `signin.cancelled`, `signin.link_expired`, `signin.no_plan`,
  `signin.device_code_off`, `signin.port_busy` or `signin.failed`.
- `AgentsEvent` gains `Signin { agent, state }` and `OpenUrl { agent, url }`. A URL to open is
  an event, not a state, so R20 opens each one once.

**The pasted code.** `PasteCode` wraps a `Zeroizing<String>`:
- Its `Debug` prints `<code>`. It has no `Display` and no `Clone`.
- What reaches stdin is the code trimmed, with control characters dropped, so a paste cannot
  add a second line, then `\n`, in a buffer zeroed on drop.
- A test checks that `{:?}` and `{:#?}` never show it.

**The environment of every run.** S18 builds each sign-in, status and detection child's whole
environment (`signin/env.rs`):
- It starts from a base, `SigninConfig::base_env`, which is this process's
  `std::env::vars_os()` outside tests. A test passes its own map.
- It removes every `SSH_*` name, `NO_BROWSER`, `CLAUDE_CODE_REMOTE`, `CLAUDE_CONFIG_DIR`,
  `CLAUDE_SECURESTORAGE_CONFIG_DIR`, `CODEX_HOME` and `CLAUSEO_SIGNIN_URL_FILE`. Names match
  without regard to ASCII case. The home variables are always removed, so an inherited one never
  survives a run that does not set it (detection).
- Then it adds the run's own variables:
  - Claude: `ANTHROPIC_API_KEY=""` on every run. Own login: `CLAUDE_CONFIG_DIR=<home>`. Reused
    (the session's view, for the run-time fallback): the same plus
    `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`. `HOME` is never changed.
  - Codex: `CODEX_API_KEY=""` on every run. Every run but detection gets `CODEX_HOME=<home>`,
    and on macOS and Linux also `HOME=<home>`, so the lawyer's `~/.agents/skills` does not
    load. Every run but detection gets the arguments `-c cli_auth_credentials_store="file"`.
  - Both: `HTTPS_PROXY` from the proxy, and the agent's `network.ca_certs` file as
    `NODE_EXTRA_CA_CERTS` (Claude) or `SSL_CERT_FILE` (Codex).
  - Claude's login runs on macOS and Linux: `BROWSER` (the shim) and `CLAUSEO_SIGNIN_URL_FILE`.
    An inherited `BROWSER` stays where there is no shim (Codex, Windows).
- Detection runs get no home variable, no `HOME` change and no store override.

**The caller's launcher must clear the environment.** The child gets only the built map
because the caller gives `Agents::start` a launcher made with
`PassthroughLauncher::new(SpawnOptions::new().clear_env())`. This is a requirement on R20's
desktop wiring. A launcher that inherits the app's environment would silently pass `SSH_*`,
`NO_BROWSER` and the lawyer's own `CLAUDE_CONFIG_DIR` or `CODEX_HOME` on. The crate cannot
check this through `dyn Launcher`. The end-to-end test with a fake that prints its environment's
names (the flows' tests) is what proves it.

**The runner** (`signin/run.rs`, crate-private) starts `<node> <script> args…` through the
caller's `Launcher`, with the built environment. It reuses the pattern of `node_version` and
`Npm::run`:
- stdout is read to its end while stderr is driven through its pump and the exit is awaited,
  all joined;
- that work races the install's `Cancel` signal and a time limit through `process::Timer`;
- on a cancel or the limit, the handle is dropped, which kills the child's tree;
- it reports the exit code.

stdout and stderr go into a 64 KB ring that is never shown or logged: its `Debug` prints only its
length. Each line is offered to a callback as it arrives. Pasted codes come over a channel and are
written to stdin as they arrive. With no channel, stdin is closed at once. `process` also keeps
its own stderr ring for `ExitReport::stderr_tail`, and S18 never reads it.

**Status parsing** (`status.rs`) is pure functions:
- Claude's `auth status --json`: `loggedIn`, `authMethod`, `apiProvider`, `email` and
  `subscriptionType`, with unknown fields ignored. The last two names are defaults until G7
  row 14.
- Detection: exit 0, signed in, `authMethod` `claude.ai` and `apiProvider` absent or
  `firstParty` gives `LoginFound { from: ClaudeCode, email }`. Any other signed-in
  `authMethod`, or `claude.ai` with another provider, gives `SignedOut { found: ClaudeSettings }`.
- In Clauseo's own folder, `loggedIn: true` is signed in, whatever the `authMethod`.
- Codex: `login status` exit 0 is signed in. Then the stat of the lawyer's login file decides
  between reusable and `CodexKeyring`.
- `account/read`: only a `chatgpt` account gives an email and plan.
- `is_sign_in_again(error_text)` is true when the text names `refresh_token_reused`,
  `refresh_token_expired` or `refresh_token_invalidated`, in any case.

**File identity** (`fileid.rs`): `same_file(a, b) -> io::Result<bool>`.
- On Unix it compares `symlink_metadata`'s device and inode.
- On Windows it opens each path with `CreateFileW`:
  - desired access 0;
  - share mode read, write and delete;
  - `OPEN_EXISTING` with `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`.

  It then reads the volume serial number and file index with `GetFileInformationByHandle` and
  closes the handle (an `OwnedHandle`) before returning.
- Access 0 grants no right to the contents. The handle can query attributes and ids, but cannot
  read a byte.
- Links are not followed. A symbolic link, junction or other reparse point in either place is
  never the same file, even when the ids agree.
- A missing file is an error.
- `unsafe` appears only in that Windows function, under an `#[expect(unsafe_code)]` with its
  allowlist line.

**The credential-name scan** (`tests/credential_names.rs`, done-when 2) reads every `.rs` file
under `crates/agents/src`, with comments blanked and string literals kept. It fails if any file
names Claude's credential file, or names Codex's login file outside `signin/reuse.rs`. It also
fails if `reuse.rs` calls `File::open`, any `fs::read*`, `OpenOptions` or the crate's own
`open_read`. The test builds the file names with `concat!`, so it never names them itself.
`reuse.rs` does not exist yet; the test guards it from the start.

### The calls and the job model (S18 piece 3)

**The calls on `Agents`.** Each checks what it can at once and returns
`Result<(), SigninError>`; the work runs on threads, as installs do:
- `sign_in(agent, Method)`. `Method` is `ClaudeAi`, `Console`, `Browser` or `DeviceCode`.
  Claude takes the first two, Codex the last two.
- `cancel_sign_in(agent)`, `paste_code(agent, PasteCode)`, `sign_out(agent)`,
  `check_status(agent)` (R20's "a window shows Plans") and `signin_state(agent)`.
- `SigninError` covers an unknown agent, another agent's method, a flow not built yet (Codex
  until piece 4), no home in `SigninConfig::homes`, an install not `Ready`, no run to cancel
  or paste into, and a reused login asked to sign out. No variant carries a code, an email or a
  path.
- Which CLI signs an agent in comes from its id: `claude-acp` is Claude, `codex-acp` is Codex.

**One job model** (`signin/job.rs`, `worker/signin.rs`), for every sign-in, sign-out and status
check, and for the tasks pieces 4 to 6 add:
- An agent has at most one job. The call reserves the agent's slot under a lock: a new ticket
  (job number, agent, `Cancel`) and, for a sign-in, the channel pasted codes go down. Then the
  worker thread starts the job on its own thread.
- A sign-in or sign-out supersedes the running job: its cancel is set, which kills its child's
  tree, and its events are dropped from then on. "Didn't open?" is a new `sign_in`, so
  reopening is a new login run. A status check with a job running starts nothing, since that
  job reports by itself.
- `cancel_sign_in` sets the cancel but keeps the job the agent's. The job then reports
  `Failed { code: Cancelled }` itself, once its child is killed and its URL file removed.
- Every event goes through `SigninHub::emit_job`, which checks that the job is still the agent's.
  The flow's last state ends the job. A flow that cannot tell (a status that timed out) ends it
  with the state unchanged.
- The job hands its task to the agent's flow. Codex's flow (piece 4) is one more arm of that
  dispatch, with no change to the calls. Reuse (piece 5) adds its tasks and reads the recorded
  source at the check point `sign_out` already has. The queued start (piece 6) reserves the
  slot early and starts the same job once the install is ready and the network is up.
- Sign-in jobs run beside installs and do not take the install job lock.

### Claude's sign-in (S18 piece 3)

**The run.** `node <adapter> --cli auth login --claudeai`, or `--console`, in the agent's home
(created if missing; it is also the run's working folder), with the Own environment:
`CLAUDE_CONFIG_DIR` set to the home, everything stripped as above, and `BROWSER` plus
`CLAUSEO_SIGNIN_URL_FILE` on macOS and Linux. stdin stays open for pasted codes.

**While it runs**, the state is `Waiting { since, nudge: false }`, and three things race:
- **Polling.** Every 1.5 s, `--cli auth status --json` runs with a 5 s limit. The first that says
  signed in ends everything: the login is killed and the state is
  `SignedIn { email, plan: subscriptionType, source: Own, at }`.
- **The nudge.** At 30 s, and not before, the state becomes `Waiting { since, nudge: true }`.
- **The login run itself**, with a 10 minute limit.

**How it ends:**
- At 10 minutes the login is killed: `Failed { code: LinkExpired, at }`.
- On Cancel the login is killed: `Failed { code: Cancelled, at }`.
- If it exits 0, one more status run decides between `SignedIn` and `signin.failed`. The real CLI
  exits 0 right after a good login, often before a poll has seen it.
- If it exits otherwise, the result is `signin.cancelled` when an output line matched the cancel
  pattern, and `signin.failed` if not. The pattern is a line holding both `OAuth error` and
  `access_denied`, in any case.
- In every case the child, the pollers with their limits, and the URL file are gone before the
  last state is sent. So no status process runs after it, which the tests check by seeing that no
  timer wait is left armed.
- A status check (`check_status`) is one such status run: `SignedIn` or `SignedOut`, unchanged
  when it cannot tell.

**The paste route.** `paste_code` puts the code on the run's channel. The runner writes it, with
a newline, to the login's stdin and drops it, zeroed. It appears in no event, state, `Debug` or
error text. The prompt "Paste code here if prompted >" has no newline, so nothing waits on it.

**Sign-out.** `--cli auth logout` with the Own environment, 30 s limit, then a status run: the
result is `SignedOut`, or `SignedIn` if the account is still there. If status cannot tell, a
logout that exited 0 is taken at its word. A login whose recorded source is `Reused` is never
logged out. The call refuses it, and the job checks again before any run. Piece 5 makes that
state reachable.

**The shim** (`signin/shim.rs`, macOS and Linux). Before each Claude login, the worker thread
writes `<data_dir>/agents/signin/browser` atomically, with mode 0700. It is an `sh` script that
refuses to run without `CLAUSEO_SIGNIN_URL_FILE` and appends its first argument there as a line.
Writing it on the one worker thread means two jobs never write it at once.

Each run gets `<data_dir>/agents/signin/url-<agent>-<job>`:
- It is made new with mode 0600 and `O_NOFOLLOW` through `write::create_private`, after a stale
  one is removed.
- It is removed when the run ends, whatever the outcome (`UrlFile`'s `Drop`).
- The job reads it on every output line of the login and on every poll.
- Each new line is sent once as `OpenUrl`, but only if it is `https://` with a host exactly in
  the snapshot's sign-in hosts. A user name, a backslash, a space or control character, an IP
  literal or a port that is not digits each refuse it.
- An address read only from the login's output (Claude's paste route,
  `redirect_uri=https://platform.claude.com/...`) is never sent.

`claude.com` joins Claude's sign-in hosts: Claude Code's authorize page is
`https://claude.com/cai/oauth/authorize`, the address the brief's fake opens. G7 row 1 confirms
the list against a real sign-in.

**Windows** has no shim until G7 row 1 says Claude honours `BROWSER` there. So no
`CLAUSEO_SIGNIN_URL_FILE` and no URL file, an inherited `BROWSER` is left as the lawyer's own,
Claude opens its own tab, and Clauseo sends no `OpenUrl` for Claude. Everything else (polling,
nudge, expiry, Cancel, paste, sign-out) is the same.

**Test seams.** `AgentsConfig::ready_for_tests` (test-support only) takes an agent as `Ready`
with a given `Installed`, so the tests run `fake_agent` as the adapter. Their timer ends each
wait by its duration, so the 30 s nudge and the 10 minute expiry are tested with no real time.
The base environment seam stands in for "set in the test's own environment". The launcher is
`PassthroughLauncher` with `clear_env`, as R20 must give. `fake_agent` must be built first.

### Codex's sign-in (to come: S18 piece 4)

The port 1455 probe, the browser and device-code runs, and `account/read` through
`codex app-server`.

### Detection and reuse (to come: S18 piece 5)

`signin/reuse.rs`: the markers, the four reuse calls, the hard link, the guard and the split
rule.

### Worker wiring (to come: S18 piece 6)

`Queued` (a sign-in before the install is ready, or while offline; until then `NotReady` is
refused) and status checks at start. The calls on `Agents` came with piece 3.

## Consequences

- R20 must give `Agents::start` a `clear_env` launcher and fill `SigninConfig::homes` from
  `Locations::agent_home`. The base environment defaults to the app's own.
- Every later sign-in piece builds its environment with `signin/env.rs` and starts its runs with
  `signin/run.rs`. Neither the ring nor `ExitReport::stderr_tail` is ever logged.
- The marker file `login-source` cannot be read in `reuse.rs`, which may not call a read. It is
  read from another module.
- The foundation items that only Codex's flow and reuse call carry an item-level
  `#[expect(dead_code)]` until those pieces land. Each expect then fails and is removed with its
  allowlist line.
- The agent's home is created with `create_dir_all`, which follows a link already standing
  there. The folder is under the trusted data folder, as the adapters are (0033).
- For the security review (S18):
  - the env builder and its dependence on the caller's `clear_env` launcher;
  - the Windows `CreateFileW` handle with access 0, opened to read a file id;
  - `PasteCode`'s zeroing and `Debug`;
  - the ring never logged;
  - the shim and the URL file (mode 0600, removed whatever the outcome), and the host check
    before `OpenUrl`.
- Not checked yet: `fileid.rs`'s Windows branch is written against `windows` 0.62.2 but was not
  built or run here. It needs `cargo nextest` on `clauseo-win-01`. On ReFS, a 64-bit file index
  may not be unique (`FILE_ID_INFO` holds 128 bits). The login files live on the system volume,
  which is NTFS.

## Status

proposed

## Date

2026-11-04
