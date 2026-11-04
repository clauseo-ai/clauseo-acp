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
covers the whole of S18: the shared pieces every run is built from, the calls and their job
model, the queued start, one section per agent's flow, reuse, and what R20 must do with it.

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
- `AgentsEvent` gains `Signin { agent, state }`, `OpenUrl { agent, url }` and
  `SigninNote { agent, note }`. A URL to open is an event, not a state, so R20 opens each one
  once. A note (`claude_reuse_fallback`, `codex_link_failed`) is for R20's log: the crate has no
  logger.

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
`PassthroughLauncher::new(SpawnOptions::new().clear_env())` (see "What R20 must do"). A
launcher that inherits the app's environment would silently pass `SSH_*`, `NO_BROWSER` and the
lawyer's own `CLAUDE_CONFIG_DIR` or `CODEX_HOME` on. The crate cannot check this through
`dyn Launcher`. The end-to-end test with a fake that prints its environment's names
(`shim::the_remote_hints_in_the_base_environment_never_reach_the_child`) is what proves it.

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

**The calls on `Agents`.** Each checks what it can at once, on the caller's thread, and
returns `Result<(), SigninError>`; the work runs on threads, as installs do:
- `sign_in(agent, Method)`. `Method` is `ClaudeAi`, `Console`, `Browser` or `DeviceCode`.
  Claude takes the first two, Codex the last two.
- `try_again(agent)`: Codex's "Try again" after `signin.port_busy`.
- `cancel_sign_in(agent)`, `paste_code(agent, PasteCode)`, `sign_out(agent)` and
  `check_status(agent)` (the start check, and R20's "a window shows Plans").
- Reuse's four: `find_login`, `use_found_login`, `use_different_account` and
  `stop_using_login(agent, Option<Found>)` (see "Reuse").
- `signin_state(agent)` reads the last state.

`SigninError` covers an unknown agent, another agent's method, no home in
`SigninConfig::homes`, an install not `Ready` (for the calls that do not queue), no run to
cancel or paste into, a reused login asked to sign out, nothing found to use, and Clauseo's own
login asked to stop reuse. No variant carries a code, an email or a path. Which CLI signs an
agent in comes from its id: `claude-acp` is Claude, `codex-acp` is Codex.

**One job model** (`signin/job.rs`, `worker/signin.rs`) runs every task:
- An agent has at most one job. The call reserves the agent's slot under a lock: a new ticket
  (job number, agent, `Cancel`), for a task that may run a login the channel pasted codes go
  down, and for a task that queues its kick (below). Then the worker thread starts the job on
  its own thread. Reserving on the caller's thread means a `paste_code` straight after
  `sign_in` is never lost.
- A sign-in, sign-out or reuse choice supersedes the running job: its cancel is set, which kills
  its child's tree, and its events are dropped from then on. "Didn't open?" is a new `sign_in`,
  so reopening is a new login run. A status check with a job running starts nothing, since that
  job reports by itself.
- `cancel_sign_in` sets the cancel but keeps the job the agent's. The job then reports
  `Failed { code: Cancelled }` itself, once its child is killed and its URL file removed.
- Every event goes through `SigninHub::emit_job`, which checks that the job is still the agent's.
  The flow's last state ends the job. A flow that cannot tell (a status that timed out) ends it
  with the state unchanged.
- The job hands its task to the agent's flow (`signin/claude.rs`, `signin/codex.rs`) or to
  reuse (`signin/found.rs`). A sign-in of Clauseo's own that ends `SignedIn { Own }` writes
  `login-source` `own`.
- Sign-in jobs run beside installs and do not take the install job lock.

**The queued start** (`signin/queue.rs`). A task that starts a login run (`sign_in`,
`try_again`, `use_different_account`) needs two things first: the agent's install `Ready`, and
the network. The call never refuses for either. It takes the slot as above, and the job waits:
- **The install.** The job reads the shared install state. While it is not `Ready`, the job
  waits on its slot's kick, which every install `Ready` sends (`Shared::emit` and
  `Shared::emit_job` call `SigninHub::install_ready`). The kick is a one-place channel made
  with the slot, so a `Ready` that lands between the call and the job's first look is not lost.
  An install that ends `Failed` or `Waiting` sends nothing, so the sign-in stays `Queued`
  until a later install succeeds, or until it is cancelled or superseded.
- **The network.** Offline means the agent's first sign-in host in the snapshot (`claude.ai`,
  `auth.openai.com`) does not resolve. With a proxy set (`InstallConfig::proxy`), the proxy's
  host is looked up instead: behind a firm proxy that is the one name the CLI resolves, and the
  install's downloads do the same. The lookup goes through the install's resolver seam: the OS
  (`ToSocketAddrs`) in production, `InstallConfig::resolver` under test-support. A failed
  lookup waits 30 s on the `Timer` and looks again, the install's offline cadence (0033,
  Failures). The lookup is S18's only network call; the CLIs make the sign-in's connections
  themselves.
- **Both.** The install is checked first, then the lookup, and each wake checks both again, so
  the order in which they come does not matter. The state is `Queued`, sent once, from the
  first time either fails until the run starts and reports `Waiting`. With both holding at
  once, no `Queued` is sent: the lookup still runs before every login run.
- `cancel_sign_in` on a queued sign-in ends the wait with `Failed { code: Cancelled }`. A new
  sign-in replaces it silently, as it replaces a running one. `paste_code` is refused while
  queued, since no run reads stdin yet.
- The status check, detection, `find_login`, `use_found_login`, `stop_using_login` and
  `sign_out` are unaffected. They run the local CLI, so they need the install but not the
  network: refused with `NotReady` while there is no install, and never queued or looked up.
  A status check while a sign-in is queued starts nothing, as with any job running.

### Claude's sign-in

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
- Clauseo's own login in a status check (`check_status`) is one such status run: `SignedIn` or
  `SignedOut`, unchanged when it cannot tell. The whole start check is under "Reuse".

**The paste route.** `paste_code` puts the code on the run's channel. The runner writes it, with
a newline, to the login's stdin and drops it, zeroed. It appears in no event, state, `Debug` or
error text. The prompt "Paste code here if prompted >" has no newline, so nothing waits on it.

**Sign-out.** `--cli auth logout` with the Own environment, 30 s limit, then a status run: the
result is `SignedOut`, or `SignedIn` if the account is still there. If status cannot tell, a
logout that exited 0 is taken at its word. A login whose recorded source is `Reused` is never
logged out. The call refuses it, and the job checks again before any run (see "Reuse").

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

`claude.com` joins Claude's sign-in hosts, beside the brief's `claude.ai`,
`console.anthropic.com` and `platform.claude.com`: Claude Code's authorize page is
`https://claude.com/cai/oauth/authorize`, the address the brief's fake opens. G7 row 1 confirms
the list against a real sign-in.

**Windows** has no shim until G7 row 1 says Claude honours `BROWSER` there. So no
`CLAUSEO_SIGNIN_URL_FILE` and no URL file, an inherited `BROWSER` is left as the lawyer's own,
Claude opens its own tab, and Clauseo sends no `OpenUrl` for Claude. Everything else (polling,
nudge, expiry, Cancel, paste, sign-out) is the same.

### Codex's sign-in

**The runs.** `node <install>/node_modules/@openai/codex/bin/codex.js` with the Own environment
(`CODEX_HOME` and, on macOS and Linux, `HOME` set to the agent's home; `CODEX_API_KEY=""`;
`SSL_CERT_FILE` for the agent's CA; `HTTPS_PROXY`), in the home, and
`-c cli_auth_credentials_store="file"` before the subcommand, where Codex takes its `-c`
overrides:
- `login` (`Method::Browser`), 10 minute limit;
- `login --device-auth` (`Method::DeviceCode`), 20 minute limit, so the code's own 15 minutes
  end it first;
- `login status`, 5 s limit, exit code only (its output is never read);
- `logout`, 30 s limit;
- `app-server`, 10 s limit, with `limits`' six `-c` overrides as well (plugins off stops its
  start-up git sync, #203). It is asked `initialize` and `account/read` through
  `codex_appserver`'s `Client`, then its client is dropped and its tree killed.

The calls are the same as Claude's, plus `try_again`, which Claude refuses with `WrongMethod`.

**Port 1455.** Before a browser run the job binds `127.0.0.1:1455` and lets go at once. If
the bind fails, no run starts, and the state is `Failed { code: PortBusy, at }`. `try_again`
probes at once and then every 2 s, through the `Timer`. The state stays as it was while it
waits, and `cancel_sign_in` ends the wait with `signin.cancelled`. Once the port is free, the
browser run starts as above. A Codex run that loses the race anyway and prints a line naming
`1455` and `in use` also ends with `signin.port_busy`. Production always probes 1455.

**No `OpenUrl`.** Codex opens the browser from its own process. The job writes no shim, and
the addresses Codex prints (the callback, the device page) are never sent as `OpenUrl`.

**The device code** is read from the run's output, strictly:
- terminal escapes are dropped first (Codex colours the URL and the code), then each
  whitespace-separated token is checked, with surrounding punctuation trimmed;
- the URL must be exactly `https://auth.openai.com/codex/device`, and it must also pass the
  shim's address check against the snapshot's Codex sign-in hosts;
- the code must be `XXXX-XXXX`, each `X` an ASCII capital or digit, and it counts only after
  the URL;
- the first pair wins and is sent once as `DeviceCode { url, code, expires_at }`, where
  `expires_at` is the clock's now, when the code was read, plus 15 minutes.

The 15 minutes are a `Timer` wait started when the code is read. When it ends, the run is killed
and the state is `Failed { code: LinkExpired, at }`. Codex's refusal, a line that says
`Enable device code` and `Security Settings`, gives `signin.device_code_off` when the run
exits.

**Confirming.** While a login runs, the state is `Waiting { since, nudge: false }`. Codex has no
nudge: the screen offers "Use a code instead" from the start. `login status` is polled every
1.5 s, as for Claude, and exit 0 is signed in. The first such poll kills the login. Then one
`account/read` gives `SignedIn { email, plan: plan_type, source: Own, at }`. If the app-server
does not answer, `email` and `plan` are `None`: status already said signed in. Otherwise the run
ends as Claude's does:
- Cancel or a newer job: `signin.cancelled`;
- the run's limit: `signin.link_expired`;
- exit 0: one more status run decides;
- any other exit: `signin.device_code_off`, `signin.port_busy` or `signin.cancelled` (the
  `OAuth error` and `access_denied` pattern) from the lines, else `signin.failed`.

The login, its polls and the app-server are gone before the last state is sent. `check_status`
is one status run, and `account/read` when it says signed in.

**Sign-out.** `logout`, then status, as for Claude, never while the source is `Reused`. Any
Codex `login` or `logout` is also refused while the home's login file and the lawyer's
`~/.codex` one are the same file: `codex::home_shares_lawyers_login`, the guard below, is asked
before every one.

**Refresh race.** `is_sign_in_again` classifies `refresh_token_reused`, tested with a
Codex-shaped error text through the public API.

**`app-server`'s overrides.** `limits` already runs `app-server` with six `-c` overrides; S18
copies them by hand, since `agents` cannot depend on `limits`. A change to one list must be made
to the other.

### Reuse

**The calls.** Four, each a job on the model above:
- `find_login(agent)`: UD2's "Look for my … login". It deletes `no-reuse`, then runs the status
  check.
- `use_found_login(agent)`: the lawyer's "Use it". It is refused with `NothingFound` unless the
  state is `LoginFound`.
- `use_different_account(agent)`: "Sign in with a different account". It writes `no-reuse`,
  then starts Clauseo's own sign-in (Claude `--claudeai`, Codex browser). It is refused with
  `Reused` while the login is reused.
- `stop_using_login(agent, why: Option<Found>)`: it never runs a logout. It is refused with
  `NotReused` for Clauseo's own login.

`use_found_login` and `use_different_account` are cancellable and take pasted codes like
`sign_in`, since they may run a login; `use_different_account` queues as `sign_in` does.
`AgentsEvent::SigninNote` carries `claude_reuse_fallback` and `codex_link_failed`.

**The markers** (`signin/marker.rs`) are Clauseo's own files in `<data_dir>/agents/<id>/`,
never in a CLI's home. They are read and written through `write`, which refuses a link, and
written atomically:
- `login-source`: the line `own`, or the line `reused` followed by the accepted email on a
  second line (left out when the status gave none). No secret. Anything unreadable or garbled
  (a link, over 4 KB, another first line) reads as reused, the side on which no logout runs.
  `own` is written when a sign-in of Clauseo's ends `SignedIn { Own }`, and `reused` on "Use
  it". Clearing the reuse record deletes the file.
- `no-reuse`: the line `no-reuse`. Only whether it exists counts (`symlink_metadata`).

**The status check** (`check_status`) is the start check (`signin/found.rs`). R20 calls it
at start and when a window shows Plans. It runs:
1. A recorded reuse first. For Codex, a missing `login-source` with a login file in the home
   also counts, per the brief.
   - Claude: detection must still find a `claude.ai` login with the accepted email. Another
     email gives `ReusedChanged`, signed out gives `ReusedSignedOut`. Then status under the
     session's environment (`CLAUDE_CONFIG_DIR` the home, `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`)
     must say signed in, giving `SignedIn { Reused }`. Otherwise the run-time fallback below.
   - Codex: `codex_link_holds` must say `Holds`, giving `SignedIn { Reused }` with
     `account/read` under the home. `Split` gives `CodexLinkSplit`, `Gone` gives
     `ReusedSignedOut`.
2. Clauseo's own login: "A Clauseo-owned login, once made, wins over reuse."
3. With `no-reuse` there, `SignedOut`. Otherwise detection.

A run that cannot tell leaves the state unchanged. The job creates the agent's home before
any run. Without it, a status check before the first sign-in could not start, since every run
but detection has the home as its working folder.

**Detection** (`signin/detect.rs`) runs with `RunKind::Detect`: no `CLAUDE_CONFIG_DIR`,
`CLAUDE_SECURESTORAGE_CONFIG_DIR` or `CODEX_HOME`, the lawyer's `HOME`, no
`cli_auth_credentials_store`. Its working folder is the empty `<data_dir>/agents/signin/detect`.
- Claude: `--cli auth status --json`, read by `status::claude_detection`.
- Codex: `login status`, exit code only. Exit 0 is followed by a `symlink_metadata` of the
  lawyer's `~/.codex` login file: present is `LoginFound` (with `account/read`, same
  environment, same folder), absent is `CodexKeyring`.

The lawyer's home folder is `SigninConfig::lawyer_home()` (new, public): the base environment's
`HOME`, or `USERPROFILE` on Windows.

**"Use it"** runs detection again first. If the email differs from the one `LoginFound` showed,
the new `LoginFound` is reported and nothing is reused.
- Claude: status under the session's environment must say signed in. Then `login-source` is
  written, and the state is `SignedIn { Reused }`.
- Codex: `link_reused_login` makes the hard link, then `login-source` is written, then
  `SignedIn { Reused }`, with the plan from detection's `account/read`. The link is refused
  when the lawyer's name is not a plain file, and checked with `same_file` afterwards. If it
  fails (another volume, a name already taken, any error), the `codex_link_failed` note goes
  out, then Clauseo's own browser sign-in runs. If `login-source` cannot be written, the link
  is removed again.

**Reuse stops** (`found::stop`) with no logout:
- no reason: `no-reuse` is written;
- for Codex, a reused home's login file name is removed (`unlink_reused_login`);
- then `login-source` is deleted, and the state is `SignedOut { found: why }`.

A marker write that fails gives `signin.failed`, since the next start could otherwise reuse
again without asking. The status check uses the same steps when it finds reuse has ended. So the
next start shows `LoginFound` again, and nothing re-links on its own.

**The run-time fallback.** Claude's status can say signed out under the session's environment
while detection says signed in. Then the `claude_reuse_fallback` note goes out, and reuse stops
with no reason: `no-reuse`, `SignedOut`. This happens on "Use it" or at a later start. A Claude
Code that ignores the variable would loop through "Use it" otherwise. "Look for my Claude Code
login" offers it again.

**The guard and the split rule** (`signin/reuse.rs`, public):
- `codex_login_shared(codex_home, lawyer_home)` is true while the two login files share a file
  id. S18 refuses its own Codex `login` and `logout` then, whatever `login-source` says. R20
  asks it before codex-acp's `authenticate`.
- `codex_link_holds(codex_home, lawyer_home) -> CodexLink { Holds, Split, Gone }`, with
  `CodexLink::found()`, is the check R20 and R17 run at every Codex session start.
- `unlink_reused_login(codex_home)` is the leave order's unlink step.

`same_file` does not follow links, so a symbolic link is never "one file".

**Sign-out.** A Codex `logout` is refused by the state, then by the job if any of these holds:
`login-source`, the missing-marker rule, or the guard. Claude's `auth logout` is refused the same
way by `login-source`. The job then runs the status check instead, which reports the reused
login. No `auth logout` or `codex logout` runs while the source is `Reused`.

**The leave order** (R20's: no-reuse, stop, unlink, new login, relaunch). `stop_using_login`
covers no-reuse and unlink in one job, for the agent's own home. R20 calls it once every Codex
process using the home has stopped, then `sign_in`, then relaunches. R17 calls
`unlink_reused_login` for each matter's folder.

**The fakes.**
- Codex's mode takes a file with the name of Codex's login file as its marker too. A test's
  empty dummy in `~/.codex`, hard-linked into the home, then signs the home in as Codex's file
  would.
- Claude's status prints the marker's email.
- `FAKE_SIGNIN=secure-ignored` makes Claude's mode drop `CLAUDE_SECURESTORAGE_CONFIG_DIR`, for
  the run-time fallback.

**Every file operation on a login file's path** (for the security review). All are in
`signin/reuse.rs`, the one file that may name Codex's login file; nothing names Claude's. None
opens or reads the file:
- `std::fs::symlink_metadata` (an `lstat`): `reuse.rs` `codex_link_holds` (the lawyer's file),
  `lawyer_login_present` (detection's stat), `codex_reused` (the home's file),
  `link_reused_login` (the lawyer's file is a plain file).
- `same_file`, i.e. `lstat`, or on Windows a `CreateFileW` handle with access 0:
  `codex_link_holds`, `codex_login_shared`, and `link_reused_login` before and after the link.
- `std::fs::hard_link`, through `write::hard_link`: `link_reused_login`, on "Use it" only.
- `std::fs::remove_file`, through `write::remove_file`, of the home's name only (never the
  lawyer's): `unlink_reused_login`, and `link_reused_login` when the new name does not compare
  as one file.

The Codex CLI itself reads and refreshes the linked file in the home, as it would its own.
Claude's reuse touches no file: it is an environment.

### What R20 must do

The crate cannot enforce these through its API; R20's desktop wiring must:
- give `Agents::start` a launcher made with
  `PassthroughLauncher::new(SpawnOptions::new().clear_env())`, so each child gets only the
  environment `signin/env.rs` builds;
- fill `SigninConfig::homes` from `Locations::agent_home(id)`, the text as it is, never
  canonicalised (Claude keys its login store by that text). The base environment defaults to
  the app's own;
- write each `AgentsEvent::SigninNote` to its log by its `code()`, and open each `OpenUrl`
  once;
- stop every Codex process using the home before `stop_using_login` (the leave order), and ask
  `codex_login_shared` before codex-acp's `authenticate`; run `codex_link_holds` at each
  Codex session start (R17 too);
- call `check_status` at start and when a window shows Plans, and add the six `signin.*` codes
  to `ErrorCode` when it maps the states to the wire.

### Tests and their seams

- `AgentsConfig::ready_for_tests` takes an agent as `Ready` with a given `Installed`, so the
  tests run `fake_agent` as the adapter (`fake_agent` must be built first).
  `Agents::finish_install_for_tests` ends an install `Ready` or `Failed` through the same emit
  an install job uses, for the queued tests.
- The tests' timer ends each wait by its duration, so the 30 s nudge, the 10 and 15 minute
  expiries, the 2 s port probe and the 30 s offline wait are tested with no real time.
- `SigninConfig::base_env` stands in for "set in the test's own environment".
  `SigninConfig::callback_port` moves port 1455, so a test can hold it with a listener and tests
  run side by side. `InstallConfig::resolver` plays the network: the fixture's resolver answers
  until a test turns it offline, and records each name looked up. All three seams exist only
  with `test-support`.
- The launcher is `PassthroughLauncher` with `clear_env`, as R20 must give.

## Consequences

- Every sign-in piece builds its environment with `signin/env.rs` and starts its runs with
  `signin/run.rs`. Neither the ring nor `ExitReport::stderr_tail` is ever logged.
- The markers `login-source` and `no-reuse` are read and written in `signin/marker.rs`, since
  `reuse.rs` may not call a read.
- No `#[expect(dead_code)]` is left from the foundations, nor its allowlist lines.
- Choices to look at in review:
  - `claude.com` in Claude's sign-in hosts (see "Claude's sign-in").
  - `limits`' six `-c` overrides for `app-server`, copied by hand (see "Codex's sign-in").
  - With a proxy set, the queued start looks up the proxy's host, not the sign-in host. A firm
    whose DNS resolves only the proxy is then never taken for offline.
  - The lookup blocks the job's thread for as long as the OS takes to fail it; a cancel during
    one lands when it returns.
  - The agent's home is created with `create_dir_all`, which follows a link already standing
    there. The folder is under the trusted data folder, as the adapters are (0033).
  - A crash after Codex's own sign-in but before `login-source` `own` is written leaves a home
    login file with no marker. That reads as reused: no logout runs, and the next status check
    takes it for a split link and removes the name, so the lawyer signs in again. That is the
    safe side.
- For the security review (S18):
  - the env builder and its dependence on the caller's `clear_env` launcher;
  - the Windows `CreateFileW` handle with access 0, opened to read a file id;
  - `PasteCode`'s zeroing and `Debug`;
  - the ring never logged;
  - the shim and the URL file (mode 0600, removed whatever the outcome), and the host check
    before `OpenUrl`;
  - the file operations on login file paths (listed under "Reuse"), the guard, the split check,
    and the markers' "unreadable reads as reused" rule.
- Open items:
  - `Found::ClaudeWindowsSandbox` is never reported yet: detection should run again when S15's
    tier B set-up finishes, and the crate needs a signal from S15 for it.
  - Windows has no `BROWSER` shim until G7 row 1 says Claude honours it there.
  - `fileid.rs`'s Windows branch is written against `windows` 0.62.2 but was not built or run
    here. It needs `cargo nextest` on `clauseo-win-01`. On ReFS, a 64-bit file index may not be
    unique (`FILE_ID_INFO` holds 128 bits). The login files live on the system volume, which is
    NTFS.
  - G7 confirms the sign-in hosts (row 1) and Claude's status field names (row 14) against the
    real CLIs.

## Status

proposed

## Date

2026-11-04
