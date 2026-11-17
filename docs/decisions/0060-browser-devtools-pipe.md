# 0060: The browser over a DevTools pipe, on every system (route 2)

Route 2, PR W1: the browser guard's part. It amends decision record 0029 on every system: no
browser anywhere has a DevTools TCP port any more. Which browser a Mac gets is unchanged.

## Context

### The attack

Codex runs its shell commands in its unelevated Windows sandbox (codex 0.162.1,
`windows.sandbox = "unelevated"`, `crates/host/src/agent/codex.rs:98`; workspace-write, network
off). Those commands run without a prompt, and the sandbox doesn't block connections to
127.0.0.1. On main before this PR the browser had three unauthenticated local channels:

- agent-browser's daemon port (TCP, its port derived from the session name);
- Chrome's DevTools port;
- the stream port.

Prompt-injected content could get Codex to run a command that drives the unfenced browser past
the guard. It could open `file:///` pages and read any file the lawyer can, send that data out,
or set the download folder to Startup. That is why `no_browser_on` keeps ChatGPT on Windows
browserless (`crates/profile/src/servers.rs`).

The same hole is assumed open on a Mac on main. ChatGPT's browser there runs Chrome with a
DevTools port on loopback, plus agent-browser's stream server. A Claude session's sandboxed shell
might reach them if Claude Code's sandbox proxy lets some spelling of loopback through: only
`deniedDomains` keeps it out. Live probing was declined, so this is unproven, and it is treated as
open. Main builds are Rohan's only, so it is not public.

PR-1 (Claude's browser on a Mac, which may merge first) relies on that DevTools port. Its case
rests on source evidence about Claude Code 2.1.293's sandbox proxy and Seatbelt profile, which
depends on the agent's sandbox. This design doesn't depend on any agent's sandbox. W1 removes the
port, and W1 blocks any public release.

### The spike

A real x64 laptop, Windows 11, standard user, 2026-11-15. The evidence is on the mac mini at
`/tmp/vm2/spike2`. Every sandboxed line used `codex sandbox --permission-profile :workspace --`.
The token was the user's own SID at Medium integrity, write-restricted, with restricting SIDs of
the TEMP writable root, workspace_by_cwd, the logon SID and Everyone, and `SBX_NONET_ACTIVE=1`.

| # | Check | Result |
|---|---|---|
| a1 | sandbox opens an ordinary same-user process with PROCESS_QUERY_INFORMATION / PROCESS_VM_READ | GRANTED (predicted): a secret in an ordinary process's memory is NOT safe |
| a1b | DUP_HANDLE on the same | denied |
| a1-w | WRITE_DAC | denied (counts as write-type under WRITE_RESTRICTED) |
| a1h | process that set its own protected DACL at start: `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user>)(A;;0x101000;;;OW)` | VM_READ / DUP_HANDLE / WRITE_DAC denied, from the sandbox AND the normal shell; QUERY_LIMITED granted |
| a1h-t | threads of the hardened process | THREAD_SET_CONTEXT 0/24 granted (GET_CONTEXT 24/24) |
| a2 | WMI command line from the sandbox | Access denied (0x80041003) |
| p1/p2 | named pipe (default DACL / user+SYSTEM only), GENERIC_READ\|WRITE from the sandbox | denied |
| p3/p4 | same, GENERIC_READ only | granted |
| p5 | server-side peer check (GetNamedPipeClientProcessId → token) | sees restricted=True for sandbox callers |
| b-CfT | Chrome for Testing 157 (per-user install) over `--remote-debugging-io-pipes` | FAIL: Chrome's own sandbox "cannot access executable" (the folder ACL has only SYSTEM, Administrators and the user; no ALL APPLICATION PACKAGES) |
| b-Edge | Edge 157 (Program Files) over `--remote-debugging-io-pipes`, headless | PASS: Browser.getVersion JSON, 0 TCP listeners, no DevToolsActivePort |
| c | agent-browser 0.41.1 with AGENT_BROWSER_CDP=ws://relay/devtools/browser/<secret>; relay 404s /json | PASS: open + snapshot work; one bare TCP probe, then the one GET; no /json |

Plain `codex sandbox --` gives the read-only token: TEMP isn't writable, and PowerShell drops to
ConstrainedLanguage.

The earlier spike used Codex's *elevated* sandbox on a VM, and it FAILED. Loopback stayed
reachable despite its firewall rules, it needed UAC per CODEX_HOME, and homes broke each other
(evidence `/tmp/vm2/codexsbx`). Route 2 keeps the unelevated sandbox and takes the channels away
instead.

## Decision

Route 2 is four PRs: W1 (the guard, this record), W2 (the patched agent-browser), PR-3 (the
proxy's password) and the gate (the last PR). W1 is safe to merge alone. The gate stays off:
ChatGPT on Windows still gets no browser entry (`no_browser_on`), and Claude gets none on any
OS.

### No DevTools port on any system (`crates/mcp/src/browser_guard/chrome/pipe.rs`)

The guard starts the browser with `--remote-debugging-pipe`, never `--remote-debugging-port`.
CDP then runs over two pipes whose other ends only the guard holds.

- **macOS and Linux:** the browser reads fd 3 and writes fd 4 (Chromium's `kReadFD` and
  `kWriteFD`). The guard's copies sit at fds of 10 or more, close-on-exec, and the child `dup2`s
  them onto 3 and 4 between fork and exec.
- **Windows:** the pipes are two synchronous `CreatePipe` pipes, and the browser's ends are
  duplicated as inheritable. Their decimal handle values go in
  `--remote-debugging-io-pipes=<read>,<write>`; Chromium's `AdoptPipes` reads them with
  `StringToUint` and `Uint32ToHandle`, then checks `FILE_TYPE_PIPE`. Those two ends are the only
  handles beyond the standard three in the child's `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.

Chromium opens the pipe only when `--remote-debugging-pipe` is given
(`remote_debugging_server.cc`). The design named only `--remote-debugging-io-pipes`, so passing
both is a correction, not a change of design.

Every existing flag and pref stays: `--headless=new`, the run's own fresh `--user-data-dir`, the
downloads staging with its Mark of the Web, and the logon and quiet prefs. The browser counts as
ready once it answers the guard's own `Browser.getVersion` on the pipe within the start wait
(15 s).

`--cdp-port` is still parsed and checked, but nothing uses it. The host still claims and passes
it, and dropping it is a host change for the gate PR.

### The relay (`devtools.rs`)

The relay runs in the guard, on 127.0.0.1 at a port the OS picks. On Windows the socket sets
`SO_EXCLUSIVEADDRUSE` before the bind, so no same-user process can bind beside it and take
agent-browser's connection, and with it the secret.

A connection gets in only if two locks both open:

1. **The peer is agent-browser's own process** (`devtools/peer.rs`). This is checked before a byte
   is read, and nothing gets in until the guard names agent-browser's pid, right after spawning
   it. The client's socket must belong to agent-browser or one of its descendants, its daemon
   among them:
   - **Linux:** the socket's inode from `/proc/net/tcp`, held in the `/proc/<pid>/fd` of a
     process in agent-browser's tree. The tree is built from each `/proc/<pid>/stat`'s parent.
   - **macOS:** a process in agent-browser's tree (`proc_listchildpids`) that holds a TCP
     socket with that address pair (`proc_pidinfo(PROC_PIDLISTFDS)`,
     `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`, read at the `socket_fdinfo` offsets of
     `sys/proc_info.h`).
   - **Windows:** the owning pid from `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)`, then its
     parent chain up to agent-browser. Each parent must have been created no later than its
     child, so a reused pid is never taken for agent-browser. A process can't name agent-browser
     as its parent, because agent-browser's hardened DACL denies `PROCESS_CREATE_PROCESS`.

   On macOS and Linux the walk goes down from agent-browser. An orphan is reparented to init or
   launchd, so it is never found. The browser itself (the guard's child, beside agent-browser) is
   never let in.
2. **The secret path.** The request must be exactly `GET /devtools/browser/<secret> HTTP/1.1`, a
   websocket upgrade with `Host: 127.0.0.1:<port>` and no `Origin`. The secret is 256 bits from
   the OS's random source (getrandom), in hex, and is compared in constant time. Everything else
   gets a 404 and a close, including `/json`, `/json/version`, other paths, a wrong `Host`, a
   page's `Origin` and a second client while one is connected.

The relay needs both locks because a secret alone may not stay secret. On macOS and Linux a
process of the same user can read another's environment (`ps eww` or `KERN_PROCARGS2`, and
`/proc/<pid>/environ`), and agent-browser's environment holds the URL.

The relay bridges websocket text frames and the pipe's NUL-ended JSON, both ways. Messages are
capped at Chromium's 100 MiB, and frames the RFC forbids end the connection. agent-browser gets
`AGENT_BROWSER_CDP=ws://127.0.0.1:<port>/devtools/browser/<secret>` in its environment, never on
a command line. The secret is never logged. Each start makes a new pipe, a new port and a new
secret.

### The seam for the guard's own CDP traffic (`devtools/seam.rs`, `devtools/routes.rs`)

This is the contract for stage D's PR-3 `ProxyAuth`. W1 defines it without referring to PR-3's
types.

- **Ids.** `FIRST_ID = 1 << 30`. Guard commands use ids from `FIRST_ID` to `i32::MAX`, all taken
  from one counter, `next_guard_id`; `is_guard_id` tests an id. Chromium only accepts an `INT32`
  id (`crdtp/dispatch.cc`).
- **The link.** `trait CdpLink: Send + Sync { fn send(&self, message: &Value) -> io::Result<()>; }`
  writes to the browser's pipe on every system. A message with no `sessionId` goes to the root
  session. `send` is safe to call from the relay's reader thread.
- **Clients.** A `GuardClient` declares the event methods it takes (`events`) and the commands to
  run before agent-browser starts (`start_commands`). It receives `on_reply(id, &message)` and
  `on_event(&message)`. `seam::clients` is the hook: an empty list in W1, filled by PR-3.
- **Replies.** A reply with a guard id goes to the relay's own waiting call if the relay sent the
  command. Otherwise it goes to each client's `on_reply`. It never goes to agent-browser.
- **Spoofing.** An agent-browser command whose id is 2^30 or more, negative, or not an integer gets
  a local CDP error and is never forwarded. A message with no id or a non-numeric id is dropped.
  What reaches the browser is the parsed message, written out again.
- **Events.** A root-session event (no `sessionId`) of a method a client takes goes to `on_event`.
  It also goes to agent-browser, but only while agent-browser has itself sent a root
  `<Domain>.enable` with no later `.disable`. agent-browser 0.41.1 enables Fetch only on page
  sessions. Page-session events and all other events are forwarded as before.
- **Order.** After the browser answers on its pipe, the guard sends each client's start commands
  and waits up to 5 s for each reply. PR-3's start command is the root `Fetch.enable`. Only then
  is agent-browser started or allowed to connect. A refusal or a missing answer fails the browser
  start.
- **No auto-attach.** The guard never sends `Target.setAutoAttach`.

W1 contains no Fetch logic.

### Hiding the guard (`harden.rs`)

The guard hides itself first thing, before any secret exists. If it can't, it exits 1 with a line
on stderr and starts no browser (fail closed).

- **Windows:** the protected DACL from row a1h, `D:P(A;;0x1FFFFF;;;SY)(A;;0x101000;;;<user
  SID>)(A;;0x101000;;;OW)`. Every child the guard creates (the browser, agent-browser's MCP server,
  each one-shot agent-browser) gets the same descriptor in `CreateProcessW`'s
  `lpProcessAttributes`, so it is hardened from its first instruction (`job/spawn.rs`). The
  initial thread gets the same entries in `lpThreadAttributes`, with the thread rights of the same
  meaning (`0x100800`). agent-browser's own children are W2's job.
- **Linux:** `PR_SET_DUMPABLE` 0, which hides `/proc/<guard>/` (`environ`, `mem`, `fd`) and blocks
  `ptrace` from the user's other processes.
- **macOS:** nothing to set. `task_for_pid` needs an entitlement or root. A same-user process can
  read another's arguments and environment unless its own Seatbelt profile denies `process-info`,
  which is why the peer check exists.

**Revision, 2026-11-17:** the Windows part is removed. The protected DACL denied the guard's own
process `PROCESS_DUP_HANDLE`, which `WSADuplicateSocketW` needs to duplicate a socket into the
current process. That broke std's `TcpStream::try_clone` in the relay (`devtools.rs`) and
agent-browser's clone of its CDP socket, both failing with "os error 10022" (`WSAEINVAL`), so the
relay dropped agent-browser. The guard no longer starts a hardened copy of itself, and its
children get the user's default DACL; Clauseo doesn't try to be safer than the agents' own mode.
Linux's `PR_SET_DUMPABLE` stays.

### Which browser

- **Windows** (`browsers.rs`, host `browser/chrome.rs`): the host's choice comes first, but only
  when it lies under `%ProgramFiles%` or `%ProgramFiles(x86)%`; then Microsoft Edge under
  Program Files, then Google Chrome under Program Files. Chrome for Testing is never used on
  Windows, and neither is a per-user Chrome: row b-CfT shows a per-user folder's ACL stops Chrome
  for Testing's sandbox, and a per-user Chrome install sits under the same folder rules (inferred,
  not tried). A browser whose firm policy sets `RemoteDebuggingAllowed` to 0 or
  `DeveloperToolsAvailability` to 2 is set aside up front (for each value the machine's wins over
  the user's), and Clauseo's own opt-in (`browser.allow_when_remote_debugging_disabled`) does not
  bring it back, on the guard or in the host's panel. One that exits, or doesn't answer in time,
  is killed, and the next one starts on a fresh profile. A policy set through MDM or cloud
  management rather than the registry is not seen up front: it costs that browser's 15 s start
  wait, then the next is tried.
- **macOS and Linux:** the host's choice, as today, only over the pipe. On macOS that means
  installed Chrome, else Chrome for Testing.

### Failure messages on Windows

- **Neither installed:** the host shows its existing Windows words (UET-48 and UET-50). If the
  guard finds none, it tells the agent to say that installing Microsoft Edge or Google Chrome and
  starting a new conversation would give it a browser.
- **Only a per-user Chrome:** the host shows UC-554 and UC-551 (provisional): Clauseo can't use
  a Chrome installed just for the lawyer; install Microsoft Edge, or have the firm's IT install
  Chrome for everyone. The guard tells the agent the same.
- **Every installed browser blocked by policy:** the host shows UC-550 (both), UC-552 (Edge) or
  UC-553 (Chrome), naming only the blocked browsers installed (provisional): the firm's settings
  don't let apps control them, so ask the firm's IT to allow this for Clauseo. The guard tells the
  agent the same.
- **Tried and failed:** the plain "the browser could not start".

### The stream server

agent-browser gets `AGENT_BROWSER_STREAM_DISABLED=1` on every system, the switch W2's patched
agent-browser honours. It gets no stream port on any system: the guard neither reads nor passes
`AGENT_BROWSER_STREAM_PORT`, and the host no longer claims one. With the switch honoured, no
server starts, and the guard never waits for a stream port file that will never come.

agent-browser 0.41.1 always starts a stream server, on a port the OS picks.

- **Windows:** any `<session>.stream` file ends agent-browser (`port_file.rs`), so until W2 the
  Windows browser ends after its first line. The gate is off there.
- **macOS and Linux:** the file isn't read, and an upstream agent-browser's server is let be until
  the patched agent-browser ships there.

## Consequences

- No browser has a DevTools TCP port on any system. The only loopback listeners left in the
  browser's trees are the relay and, with an upstream agent-browser on macOS and Linux, its
  stream server.
- PR-1's DevTools-port design no longer exists after W1. Its browser reaches Chrome only through
  the relay.
- On Windows a lawyer's browser is Microsoft Edge, or a Program Files Google Chrome where Edge is
  missing or blocked. Chrome for Testing is never used on Windows; macOS keeps today's order.
- On Windows a by-pid open from the user gets no `PROCESS_TERMINATE` on a hardened process. The
  host ends the guard's tree through jobs, which the DACL doesn't affect.
- Each later change must keep these: the int32 id ranges; the relay's one path and its peer
  check; the secret off every command line and log; Windows children created only through
  `job/spawn.rs` with the hardened descriptors; and the gate off until W2, PR-3, the security
  review and the laptop proof are in.
- The proof rows (VM thread; every attack must FAIL): R1 no DevTools, daemon or stream TCP
  listener; R4 VM_READ, DUP_HANDLE and WRITE_DAC denied, and SET_CONTEXT 0, on the guard,
  agent-browser and the browser process; R5 no secret in WMI command lines or environments; R6 no
  path, a wrong path or `/json` gives 404. W1 covers the guard's share of R1, R4, R5 and R6. R2 and
  R3 are W2's, R8 is PR-3's, and R7 and R9 are the gate's.

## Not verified

- No real browser ran in this PR's checks on iris. The Linux CI real-Chrome tests
  (`tests/guard/real_chrome.rs`) now go through the pipe and the relay.
- Untested on a real machine:
  - Edge and Chrome with the hardened `lpProcessAttributes` and `lpThreadAttributes` (crash
    reporting may lose access to the browser process);
  - Chrome on macOS over fds 3 and 4;
  - the macOS `socket_fdinfo` offsets, which were taken from `sys/proc_info.h` and only
    compile-checked here;
  - the Windows peer check against agent-browser's detached daemon;
  - Edge on ARM64.
- The Windows-only tests (`job/harden_tests.rs`, the Windows build of the relay tests) run in the
  Windows CI job, in the merge queue.

## Status

proposed

## Date

2026-11-15
