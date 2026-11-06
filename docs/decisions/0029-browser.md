# 0029: The browser: agent-browser behind Clauseo's guard

## Context

Both agents get one browser tool: agent-browser's built-in MCP server, `agent-browser mcp --tools
core`, pinned at v0.41.1 (Apache-2.0), driving a headless Chrome through Clauseo's egress proxy.
The agent starts every MCP server itself, with its own environment. That environment can hold
`HTTP_PROXY`, `NO_PROXY` or `AGENT_BROWSER_*` values, which agent-browser reads and which would undo
the isolation: a cloud browser provider, the lawyer's Chrome profile, a CDP attach, a proxy bypass.
And agent-browser's own check on tool arguments still lets through flags and fields that change the
session, show a window, write files anywhere or load saved browser state.

So the agent never starts agent-browser. It starts `clauseo-mcp browser-guard`, which starts
agent-browser with an environment built from scratch and checks every tool call on the way in.

Source lines below are agent-browser's, at tag v0.41.1, under `cli/src/`.

## Decision

### The guard (`crates/mcp`, the first S27 pull request)

**The binary.** `clauseo-mcp`, package `clauseo-mcp`. `main.rs` dispatches on argv[1] through one
table of modes, which also builds the usage line; R24 adds `connector` to it. No mode, or any
name not in the table (case-sensitive), prints `usage: clauseo-mcp <mode> ... (modes:
browser-guard)` on stderr and exits 2. Dependencies: `serde_json`, `nix` on Unix (signals, the daemon socket's peer pid) and `windows` on Windows (the Job Object, the parent wait). Plain threads, no async
runtime, no HTTP client; `crates/mcp/tests/guard/deps.rs` walks `Cargo.lock` and fails if
reqwest, hyper, hyper-util, h2, ureq, curl, isahc, attohttpc, rustls, native-tls, openssl,
socket2, mio, tokio, async-std, smol or tauri becomes reachable.

**The command line.** Exactly `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
--cdp-port <port> -- <args...>`, in that order, and on Windows `--daemon-port <port>` after
`--cdp-port` (required there, refused on macOS and Linux). `<file>` must be an absolute path to
an existing file and `<folder>` an absolute path to an existing folder; each port is 1024 to
65535 in plain decimal (no sign, no leading zero). Both ports are ones the host claimed for the
session (host part, "Ports and socket folder"). The daemon port is an argument, not a variable,
so it stays out of agent-browser's environment and the allow-list. The child's arguments are
passed exactly as given. Anything else exits 2 with a reason and the mode's usage line.

**The environment.** The child gets exactly this map (`env.rs`), nothing inherited:

| Variable | Value |
|---|---|
| `AGENT_BROWSER_CONFIG`, `AGENT_BROWSER_EXECUTABLE_PATH` | from the guard's environment, set by the host; each must be an existing file at an absolute path; on Windows a path starting with two slashes or backslashes (a network share or device path) or `\??` is refused before anything reads it |
| `AGENT_BROWSER_PROXY` | from the guard's environment; exactly `http://127.0.0.1:<port>`, port 1 to 65535, no leading zero |
| `AGENT_BROWSER_SESSION` | from the guard's environment; 1 to 64 of `A-Z a-z 0-9 _ -` |
| `AGENT_BROWSER_PROXY_BYPASS` | `<-loopback>`, so loopback goes through the proxy too |
| `AGENT_BROWSER_ARGS` | `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` only. agent-browser no longer launches Chrome (next row); the value holds should it ever. Until the S27 launch fix it also carried `--remote-debugging-port=<--cdp-port>`, which broke every launch ("Chrome on the claimed port", below) |
| `AGENT_BROWSER_CDP` | `ws://127.0.0.1:<--cdp-port>/devtools/browser/<id>`, the endpoint of the Chrome the guard started, read from Chrome's stderr; agent-browser connects to it instead of launching a Chrome |
| `AGENT_BROWSER_IDLE_TIMEOUT_MS` | `600000` |
| `AGENT_BROWSER_MAX_OUTPUT` | `25000` (characters; see below) |
| `AGENT_BROWSER_DEFAULT_TIMEOUT` | `30000` |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (and lower case on Unix) | the validated proxy URL; no `NO_PROXY` |
| `AGENT_BROWSER_DOWNLOAD_PATH`, `_SCREENSHOT_DIR` | `<cwd>/downloads`, `<cwd>/screenshots` |
| `AGENT_BROWSER_SOCKET_DIR` | Unix: from the guard's environment, the folder the host made for the session (below); Windows: `<cwd>\run`, whatever the environment says |
| `AGENT_BROWSER_STREAM_PORT` | from the guard's environment, set by the host; a port from 1024 to 65535, no leading zero |
| Unix: `HOME`, `TMPDIR` | `<cwd>/home`, `<cwd>/tmp` |
| Windows: `TEMP`, `TMP` | `<cwd>\tmp` |
| `PATH` | Unix `/usr/bin:/bin:/usr/sbin:/sbin`; Windows `<SystemRoot>\System32;<SystemRoot>`, `C:\Windows` when `SystemRoot` is absent |
| Windows: `SystemRoot` | passed through, or `C:\Windows` when absent (Windows networking needs it) |
| Unix: `LANG` | passed through when present |
| Windows: `windir`, `SystemDrive`, `ComSpec`, `PATHEXT`, `ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`, `LOCALAPPDATA`, `APPDATA`, `USERPROFILE` | passed through when present, matched without ASCII case, first match wins, each emitted once under this spelling |

A required variable that is missing or invalid exits 2 with a stderr line naming it; the guard
never falls back. Whatever the guard's environment says, the fixed values above win. The guard
makes the folders under `--cwd` if missing (`--cwd` itself must exist), through the crate's write
module, and refuses one that exists as anything but a real folder (a file or a link): exit 1.

`AGENT_BROWSER_NAMESPACE` is never set, on any OS. With a namespace, agent-browser puts the socket
at `<socket dir>/namespaces/<ns>/run/<session>.sock` (`connection.rs:136-141`,
`session_files.rs:94-96`), and with a session id of UUID length that is over the 103 bytes a Unix
socket path may hold (`connection.rs:902-913`), so the browser would never start on macOS. The
socket folder, `HOME` and `TMPDIR` are per session already, so a namespace adds nothing.

**The Unix socket folder.** `<cwd>/run` can be too long for a socket path, so on Unix the host
makes a short folder per session, `/tmp/clauseo-ab-<16 random>` (stage C's `BrowserSocketDir`,
mode 0700, with its lock file beside it), and passes it as `AGENT_BROWSER_SOCKET_DIR`. The guard
makes nothing there: it checks the folder (`browser_guard/socket.rs`) and exits 2 with a stderr
line unless the path is absolute, names a real folder under `symlink_metadata` (not a link),
with mode exactly 0700 and the same owner uid as `--cwd`, and `<socket folder>/<session>.sock`
is at most 103 bytes (with the host's 32-byte folder it is at most 102 for a 64-character
session). It passes the same value to agent-browser. On Windows the socket folder stays
`<cwd>\run`: agent-browser uses loopback TCP there, and the folder holds only `.port` and
`.pid` files. The host removes the folder once the agent's process tree was killed (host part,
"Ports and socket folder", for what can outlive that).

**The ports agent-browser reports.** agent-browser writes the port each listener got to a file
in the socket folder, and when the port it was asked for will not bind it silently takes another
and writes that one: the stream server's `<socket folder>/<session>.stream` on every OS (started
with port fallback allowed, `native/daemon.rs` `StreamServer::start_without_client`,
`stream/mod.rs`), and on Windows the daemon's `<cwd>\run\<session>.port` (from a port it derives
from the session name, with an ephemeral fallback, the `TcpListener::bind` fallback). The host
claims the stream port (`AGENT_BROWSER_STREAM_PORT`) and draws the Windows session name so the
daemon's port is one it claimed (host part), passed as `--daemon-port`. After every line it
relays from the child, for the session's whole life, the guard reads each file without following
a link (`browser_guard/port_file.rs`): no file yet, or an empty one, means not started or being
written; the claimed port is fine for now, and the file is read again after the next line, since
the daemon can end (idle timeout, `agent_browser_close`) and start again on another port;
another port, text that is no port, a link, or anything but a regular file ends agent-browser,
and the guard exits 1 with a stderr line. A listener on another port would sit outside the ports
the jail pins to the session.

The socket folder is agent-writable (`<cwd>` is, and the jail allows the Unix socket folder to
the whole session), so the agent can write the claimed port into a file itself. The check
catches agent-browser's silent fallback to another port; it is not a boundary against the agent.

How far "ends agent-browser" reaches differs by OS. On Windows the guard starts Chrome and then
agent-browser suspended, puts each in a Job Object the guard owns
(`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, no breakaway allowed), then resumes it
(`browser_guard/job.rs`); `DETACHED_PROCESS` and a new process group do not leave a job, so
ending the job ends the daemon and Chrome too. On Unix the guard kills the direct child, then
the daemon, which has left the process group (`setsid`), by the pid of the process listening on
the session's socket, then Chrome's process group ("Chrome on the claimed port", below).

Why each value:
- The proxy variables: `agent_browser_read` with a `url` fetches through the CLI's own reqwest
  client (`read.rs:206-212`, no explicit proxy), which honours only these variables. Without
  them it would reach the network around the proxy. They are Clauseo's own values, never the
  agent's.
- `AGENT_BROWSER_SCREENSHOT_DIR`: the default is `~/.agent-browser/tmp/screenshots`
  (`screenshot.rs:658-666`), and on Windows `dirs::home_dir` ignores `HOME`, so it would land in
  the lawyer's real profile.
- `AGENT_BROWSER_MAX_OUTPUT` has no effect on MCP results: the MCP server always runs the CLI
  with `--json` (`mcp.rs:2658`), and the limit applies only to text output (`output.rs:525-548`).
  It is kept so the value holds if that changes, and the unit is characters (`output.rs:36-59`).

**The child.** agent-browser starts with `--cwd` as its working folder, piped stdin and stdout,
and the guard's stderr; on Windows with `CREATE_NO_WINDOW` (agent-browser issue #1644). The
guard starts it with `std::process::Command::spawn`, which `clippy.toml` bans because agents
start through the `process` crate: this crate may depend only on serde, serde_json, nix (Unix)
and windows (Windows), the children are fixed programs (agent-browser, the host's Chrome, and on
Unix the guard's own binary as its reaper), and the host starts the guard itself through the
`process` crate, whose tree kill reaches the guard and agent-browser. One `#[expect]` at each
spawn carries it (`spawn_child` in `crates/mcp/src/browser_guard/relay.rs`, `spawn` in
`chrome.rs`, `Reaper::start` in `reaper.rs`). The guard exits with the child's exit code, or
1 when a signal killed it.

**The relay.** Child to agent: each stdout line is copied whole, bytes unchanged. Agent to child:
each stdin line goes through the filter, and what passes is written as the guard's own
`serde_json::to_string` of the value it checked, so the child sees exactly what was checked (no
duplicate-key or other parser differential). Both directions write whole lines under one lock on
the guard's stdout. When the agent closes stdin, the guard sends one request,
`{"jsonrpc":"2.0","id":"clauseo-guard-close","method":"tools/call","params":{"name":"agent_browser_close","arguments":{}}}`,
swallows its reply, waits at most 10 s for it, then closes the child's stdin and waits at most
5 s for the child's stdout to end; then it kills the child (exit 1, with a stderr line). An agent
message that uses the id `clauseo-guard-close` is refused (below), so no reply of the agent's
can pass for the close reply. When the child ends first, the guard exits at once.

The child's lines are capped at 64 MiB: a longer one kills the child, and the guard writes a
stderr line and exits 1 without relaying any of it (fail closed; page content decides how big a
snapshot is). A last line the child leaves without a newline when it ends is dropped, with a
note on stderr, so the agent never gets half a JSON message.

agent-browser's daemon detaches. On Unix it is started with `setsid` (`connection.rs:947-951`):
it leaves the guard's process group and is reparented once agent-browser's MCP server exits, so
the `process` crate's group kill misses it. Stage B's live macOS check saw such a daemon outlive
the host's shutdown at ppid 1 and relaunch Chrome until it was killed. Since the S27 launch fix
the guard ends it on every way out ("Chrome on the claimed port", below): the host still does not
kill it from the `<session>.pid` file (a PID read from a file can name a process the system
reused; AGENTS.md), and the guard does not either, it asks the kernel for the pid of the process
listening on the session's socket. On Windows it is started with
`CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` (`connection.rs:963-983`), which does not leave a
Job Object; agent-browser runs in the job the guard owns (`browser_guard/job.rs`), whose last
handle closes when the guard exits, however it exits, so the daemon and Chrome end with the guard
on Windows.

**The filter** (`filter.rs`) applies to agent-to-child lines only, in this order:

1. A line that is not JSON, or longer than 16 MiB: `{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}`, nothing forwarded.
2. A JSON array (a batch): `-32600`, "Clauseo does not allow batched requests in the browser.",
   nothing forwarded (agent-browser drops batches anyway, `mcp.rs:673-682`). A bare scalar:
   `-32600`, nothing forwarded.
3. Any message whose `id` is the string `clauseo-guard-close`, the guard's own close request's:
   `-32600` "Clauseo reserves the id clauseo-guard-close for itself." with that id, nothing
   forwarded.
4. An object whose `method` is exactly `tools/call` (case-sensitive, as `mcp.rs:700-709`
   matches) has `params.arguments` checked whatever the tool's name. Arguments that are absent
   or not an object pass: agent-browser refuses a non-object before running anything
   (`mcp.rs:3967-3975`). Keys match exactly, as agent-browser's do (`mcp.rs:2264-2308`).
   - **Refused keys:** `session`, `namespace`, `idleTimeout`, `restore`, `restoreSave`, `restoreCheckUrl`, `restoreCheckText`, `restoreCheckFn`, `caCert`, `useSystemCa`, `clearCaCert`, `headed`, `screenshotDir`, `path`.
     Present with any value, even `false` or `null`. `path` on any tool: `screenshot.path` writes
     any file the process can reach (`screenshot.rs:626-649`), past the agent's own write
     permissions; screenshots go to the guard's screenshot folder instead.
   - **Refused unless `false`:** `newTab`, `webgpu`.
     `newTab: true` makes agent-browser read the element's `href` and open it through CDP
     (`actions.rs:6619-6689`), with no scheme check and past Chrome's block on pages opening local
     files, so a hostile page could get `file:`, `chrome:` or `data:` pages opened.
     `webgpu: true` adds `--enable-unsafe-webgpu` (`chrome.rs:483-486`). Any other value, even
     `null`, is refused by the key's name.
   - **`timeoutMs`:** refused unless a whole number up to 300000. The MCP server runs one call at
     a time (`mcp.rs:586`), so a huge value would stall it and the guard's shutdown.
   - **`selector` on `agent_browser_screenshot`:** refused when it holds `/` or `\`, or ends in
     `.png`, `.jpg`, `.jpeg` or `.webp` in any case: "Clauseo does not allow a file path as the
     screenshot selector in the browser." The selector is the screenshot command's one
     positional (`mcp.rs:3106-3109`), and agent-browser takes a lone positional for the output
     path when it holds `/` or ends in one of those extensions (`commands.rs:912-933`), so
     `/Users/x/.zshrc` would be overwritten with PNG bytes (`screenshot.rs:648`). This test is
     stricter than agent-browser's: no `.`/`#`/`@` exemption, any case, and `\` too.
   - **`url`:** refused unless it is a string whose trimmed, lower-cased value starts with
     `http://` or `https://`, equals `about:blank`, or holds no `:` at all (agent-browser then adds
     `https://`, `commands.rs:76-90`, `read.rs:151-165`). The refusal names the scheme when it is
     a well-formed one (`file URLs`), else `non-web URLs`; a non-string is refused as `url`.
     Then its host (`filter/host.rs`, read as a WHATWG parser reads it: userinfo up to the last
     `@`, a port, brackets, trailing dots, one-to-four-part decimal, octal or hex IPv4 such as
     `2130706433` or `0x7f.1`) is refused as `local or private network addresses` when it is
     `localhost` or `*.localhost`, in `0/8` (so `0.0.0.0`), `127/8`, `10/8`, `172.16/12`,
     `192.168/16`, `169.254/16`, or is `[::1]`, `[::]`, in `fc00::/7` or `fe80::/10`, or an
     IPv4-mapped or IPv4-compatible form of a refused IPv4 address. A host it cannot settle (a
     control character or space anywhere, a `%` escape, a character outside letters, digits,
     `.`, `-` and `_`, an empty host, a bad port, an IPv4 that does not fit, an IPv6 that does
     not parse) is refused as `URLs whose host Clauseo cannot read`, and a non-ASCII host as
     `non-ASCII host names (use the xn-- form)`, since IDNA could map it to digits and dots.
     This is extra defence: the proxy's rule below is the real one.
   - **`extraArgs`:** must be an array of strings, each exactly one of the allowed tokens below;
     anything else (another flag, a `--flag=value`, a positional) is refused by the token's name
     before any `=`, and a non-array or a non-string entry as `extraArgs`. None of the allowed
     tokens takes a value, and agent-browser lets positional tokens through unexamined
     (`mcp.rs:2679-2681`).
   - **`extraArgs` tokens allowed:** `--json`, `--full`, `-i`, `--quiet`, `-q`, `--annotate`.
   - **Any other string shaped like a flag:** a string at any depth of the arguments (except
     inside `extraArgs`, and `agent_browser_eval`'s `script`, which travels on stdin, never
     argv, `mcp.rs:3950-3957`) that is `-` or `--`, then a name of ASCII letters, digits and
     inner dashes that starts with a letter and does not end in a dash, then optionally `=` and
     anything. agent-browser puts typed values into the CLI's argv, and "the CLI reads global
     flags anywhere in argv" (`mcp.rs:2659-2661`; the CLI's loop, `flags.rs:820-900`), so a
     `selector` of `--headed` or a `url` of `--session` would set them. agent-browser's own
     check admits allow-listed names there (`mcp.rs:2682`). `--headed`, `-p`, `--proxy=x` and
     `-Rohan` are refused; `-5`, `-10`, `-`, `--`, `---x`, `- item`, `--Select--` and `-Select-`
     (placeholder options on Indian government forms) pass. The rule is a shape, not a list of
     flag names, so a flag added upstream is refused too.
5. A refused request is answered with its own id:
   `{"jsonrpc":"2.0","id":<same>,"result":{"content":[{"type":"text","text":"Clauseo does not allow <name> in the browser."}],"isError":true}}`,
   and never reaches the child. A refused notification (no `id`) is dropped unanswered.
6. Everything else passes: other methods (`initialize`, `tools/list`, ...), other tools' calls,
   and `allowedDomains`, which can only narrow.

A unit test in `filter.rs` fails when the three lists above differ from the code's.

### Chrome on the claimed port (the S27 launch fix)

**The bug.** As first built, the guard let agent-browser launch Chrome and pinned its debugging
port with `--remote-debugging-port=<--cdp-port>` in `AGENT_BROWSER_ARGS`. That was never run;
it rested on Chrome's switch rule alone (the last of a repeated switch wins,
`base/command_line.cc:447-449`), and the rule did hold. But agent-browser v0.41.1 puts its own
`--remote-debugging-port=0` first (`cli/src/native/cdp/chrome.rs:467`) and learns the port only
from the `DevToolsActivePort` file in the profile (`chrome.rs:885-976`), which Chrome writes only
when the port is 0 (`chrome/browser/devtools/remote_debugging_server.cc:366-396`,
`content/browser/devtools/devtools_http_handler.cc:293-309`). Its stderr fallback shares the
expired 30 s deadline and never reads (`chrome.rs:978-1000`, an upstream bug). So every
`agent_browser_open` through the guard failed after about 31 s with `os error 11 ...
outcome_unknown`, and the daemon's three launch retries (`chrome.rs:764`) left Chrome running.
No Chrome switch makes it write the file for a fixed port, on any OS.

**The fix.** The guard starts Chrome itself (`browser_guard/chrome.rs`) and agent-browser only
connects to it (`AGENT_BROWSER_CDP`; `flags.rs:689`, `native/actions.rs:4334-4436`):
- Chrome gets agent-browser's own headless flags (`chrome.rs:466-597`, less port 0), the
  session's proxy with the `<-loopback>` bypass, the WebRTC flag, `--user-data-dir=<cwd>/
  chrome-profile-<run id>` (below) and `--remote-debugging-port=<--cdp-port>`; on Linux, as root or in a
  container, `--no-sandbox --disable-dev-shm-usage` (agent-browser's rule less its `CI` test,
  which its child never saw). Its environment is agent-browser's less the `AGENT_BROWSER_*`
  names.
- The profile is the run's own, made fresh and deleted on the guard's exit ("Each run's own
  names", below). Its
  `Default/Preferences` sends downloads to a staging folder in it (below) and allows "insecure
  content" (`mixed_script`) for `http://` pages only (`http://*:*`): Chrome otherwise holds an
  `http://` download of a "high-risk" type (`.md`, `.pdf`) until someone clicks Keep, which
  never happens headless (seen on iris). Its scope:
  - It is set only in the session's own profile, which the guard makes and deletes; no other
    Chrome profile and no user's Chrome sees it.
  - Its intended effect is to waive the insecure-download check, and only for downloads started
    from `http://` pages (Chromium 147 `insecure_download_blocking.cc:454-519`; that check has
    no feature switch of its own). An `http://` download from an `https://` page stays blocked,
    as in Chrome by default.
  - It may also let an `https://` frame inside an `http://` page load `http://` scripts (not
    confirmed in Chromium's source). The impact is low: the `http://` page around it can already
    be tampered with in transit, and the `https://` site itself would have to load scripts over
    `http://`. Every request still goes through the session's egress proxy.
  - The alternative, CDP's `Browser.setDownloadBehavior` `allow` (what agent-browser's own
    launch set, letting every download through), was rejected for the reasons `allowAndName` was
    (Downloads, below): the behaviour is one per browser context, agent-browser sets `default` on
    each connect, and Chrome resets it when the client that set it disconnects, so the guard
    would have to win that race on every connect.
- The guard starts Chrome and, before it waits for Chrome to listen, tells the reaper Chrome's
  pid (Unix; on Windows Chrome is in the job before it runs), so a guard killed during the wait
  leaves no Chrome. It then reads Chrome's stderr until `DevTools listening on ws://127.0.0.1:
  <port>/devtools/browser/<id>`, at most 15 s (giving up sooner if its parent goes), and refuses (exit 1, Chrome killed, agent-browser never started)
  unless the port is exactly `--cdp-port` and the endpoint has that form. It then writes one
  stderr line naming the port, never the endpoint (which grants control of the browser).
- **The full `ws://` endpoint, not the port** (orchestrator's ruling): for a port or `http://`
  value agent-browser looks the endpoint up with a plain reqwest client (`native/cdp/
  discovery.rs:307-308`) that honours the guard's `HTTP_PROXY`, so the lookup went to the egress
  proxy and failed; a `ws://` value skips it (`browser.rs:2920-2923`). `NO_PROXY` for loopback
  would have fixed the lookup but also let `agent_browser_read`'s client, which follows
  redirects (`read.rs:203-215`), reach loopback around the proxy after a redirect.

Behaviour that changes with it:
- **`agent_browser_close` only disconnects** (`browser.rs:1648-1652`: `Browser.close` only for a
  Chrome it launched). Tabs, cookies, storage and history live until the session ends, then go
  with the profile. Before, each close deleted agent-browser's temp profile.
- **The idle timeout** (10 minutes) ends the daemon, not Chrome: Chrome lives as long as the
  guard, and the next call starts a daemon that reconnects to it.
- **Chrome starts at the session's start**, before agent-browser, not at the first browser call:
  the endpoint holds Chrome's id, so it must exist before agent-browser's environment is built.
  Every session that loads the browser server pays a Chrome start (0.3 s on iris) and its memory,
  used or not: an idle Chrome on `about:blank` with these flags is 12 processes and about 310 MB
  PSS on iris (Chrome for Testing 147, one sample; `browser_session_memory_p95` stays
  unmeasured). **Lazy start is the
  very next pull request and a release blocker:** Chrome and agent-browser start at the first
  browser call and stop after about 10 minutes idle, the profile kept until the session ends, and
  start again lazily on the next call.
- **Crash: the guard exits 1.** agent-browser does not watch a Chrome it did not launch
  (`browser.rs:1703-1709`), and a restarted Chrome has a new id, so the endpoint agent-browser
  holds would be stale. The guard polls its Chrome child every 250 ms; when Chrome exits it ends
  agent-browser and exits 1 (fail closed, like a refused port). It also looks at Chrome once the
  relay ends, so a Chrome that died between two polls while agent-browser ended (the agent
  closing its stdin at the same moment) still makes the guard exit 1. Restarting Chrome once comes with
  the lazy-start machinery in the next pull request.
- **Downloads** land in `<cwd>/downloads` as before, now moved there by the guard
  (`browser_guard/downloads.rs`): Chrome downloads into `<profile>/downloads-staging`, and every
  200 ms the guard moves each finished file (Chrome renames `*.crdownload` only when done) into
  the downloads folder. A name in the sandbox's `PROTECTED_NAMES` (`.codex`, `.claude`,
  `.agents`, `.clauseo`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`) becomes
  `<stem> (download)<ext>`, compared without ASCII case, trailing dots and spaces, and with `ſ`
  read as `s`; a taken name gets ` (2)`. The move never replaces a file (`write::move_new`: a hard
  link and then the staging name's removal on Unix, `MoveFileExW` without
  `MOVEFILE_REPLACE_EXISTING` on Windows, whose "exists" errors, `ERROR_ALREADY_EXISTS` and
  `ERROR_FILE_EXISTS`, reach std as the Win32 code, not the HRESULT, so they read as
  `AlreadyExists`; its paths are made absolute and normalised before the `\\?\` prefix); a file that appears under the chosen name between the
  look and the move makes the mover take the next free name, 16 tries at most. A web page
  therefore cannot plant agent instructions or config in the downloads folder. The page's name can exist on disk in the staging folder for
  up to one 200 ms poll; that folder is inside the session's browser state folder
  (`<data>/run/<session>/browser`), outside the matter and the work folder, where no agent loads
  instructions from. `Browser.setDownloadBehavior` `allowAndName` (files saved by GUID, so the
  page's name never touches the disk) was rejected: the behaviour is one per browser context, the
  last client to set it wins, agent-browser sets `default` each time it connects
  (`browser.rs:784`, `browser.rs:2563-2574`), and Chrome resets it when the client that set it
  disconnects. agent-browser's explicit `download` tool, with a path the agent picks, is not in
  `--tools core` and is unchanged.
- **Launch options from the agent** (`extraArgs` and the like) have no effect: agent-browser
  launches nothing. `headed` and `webgpu` were refused already.

**No process outlives the guard.** Chrome is the guard's child, on Unix the leader of its own
process group. On every way out of the guard (stdin's end after the close request, a refused
port, an oversize line, Chrome's exit, the parent's end) the guard ends agent-browser, then on
Unix the daemon (SIGKILL to the pid of the process listening on `<socket dir>/run-<run id>.sock`,
from `SO_PEERCRED` on Linux or `LOCAL_PEERPID` on macOS; no listener, no kill), then Chrome's
process group (only while Chrome is unreaped, so the id cannot be reused), moves the last
finished downloads and deletes the profile. On Windows the job does all of it.
- **agent-browser's process group.** On Unix agent-browser leads a process group of its own, and
  the guard kills that whole group, before it reaps agent-browser and before the daemon kill, on
  every way out, agent-browser's own exit included: a CLI call of agent-browser's still running
  could otherwise start a daemon after the cleanup. A daemon that has already left the group
  (`setsid`) but not yet bound its socket when the guard looks is caught by the reaper, which
  ends the run's daemon once more about 1 s after the guard's `done` or its death; that is safe
  now that the daemon's socket is the run's own. One that binds later still finds Chrome gone and
  exits at its idle timeout.
- **The parent's end.** The guard notices that the process that started it has gone even when
  nothing closes its stdin. It notes its parent first thing, before anything slow, so a parent
  that dies while Chrome starts is still the one watched: on Unix the parent pid (a parent pid of
  1 at the start means the parent is gone already, as no Clauseo setup starts the guard from pid
  1), on Windows a handle to the parent (found with a Toolhelp snapshot, and counted only if it
  started before the guard, so a reused pid is not mistaken for it). A parent gone already (Unix:
  pid 1; Windows: no process under the pid, or one started after the guard) makes the guard exit
  1 at once; a parent Windows will not open a handle to leaves only stdin's end, with a note.
  While Chrome starts the guard looks every 250 ms; afterwards a thread polls the parent pid
  every 250 ms (Unix) or waits on the handle (Windows). Either way the guard then ends
  agent-browser and Chrome and exits 1. **Residual (left as is):** on Unix only a parent pid of 1
  counts as gone at the start. A parent that dies before the guard's first look, under a
  subreaper (which then becomes the guard's parent, so the pid is not 1), goes unnoticed; stdin's
  end, which the dead parent's pipe gives, still stops the guard.
- **A killed or crashed guard** (SIGKILL, an unhandled signal, `panic = "abort"`) cannot clean
  up. On Unix the guard first starts a reaper, `clauseo-mcp browser-reaper` (its own binary, in
  a process group of its own, `browser_guard/reaper.rs`), with a pipe on its stdin, and waits
  for its `ready`, which it writes once it holds its share of the run's lock (below). The guard
  writes Chrome's pid to it as soon as Chrome is started, agent-browser's as soon as it is,
  `reaped chrome` or `reaped agent` once it has killed or reaped that one itself (the reaper
  then never signals that pid or group, which could have been reused), and `done` once it has
  cleaned up itself. When the pipe ends without `done`, the reaper does the guard's cleanup for
  its own run alone, agent-browser's process group first. On Windows the job's last handle closes
  with the guard. The next run's sweep, or the host's run-folder removal, deletes a profile either
  leaves.
- **Each run's own names** (`browser_guard/runs.rs`). An MCP reconnect starts a new guard under
  the same `--cwd` and ports at once. With names shared by the session's runs, the killed
  guard's reaper deleted the new run's `chrome-profile` (the code owner's repro: 5 of 5 runs,
  140 ms after the new guard listened) and could kill its daemon. Now each run has:
  - an id, `<guard pid>-<8 random hex>`;
  - its profile `<cwd>/chrome-profile-<id>`, with Chrome's downloads staging folder inside it;
  - on Unix the agent-browser session name `run-<id>` (at most 23 bytes, which the socket
    folder's length check assumes), so the daemon's socket `<socket dir>/run-<id>.sock` is the
    run's own. Windows keeps the host's name: the daemon's port derives from it and the host
    claimed that port; there no reaper runs, and the job ends the daemon with its guard;
  - a lock file `<cwd>/chrome-profile-<id>.lock`, which the guard and its reaper hold shared
    (`File::lock_shared`) for as long as either runs.
  The reaper gets the id (`--run`) and the session name, and deletes and kills only its run's.
  At its start each guard sweeps stale runs: one whose lock file it can lock exclusively (no
  guard and no reaper left; the OS drops a lock with its holder, so a reused pid cannot fool it)
  loses its profile and then its lock file, and so does a profile with no lock file; a run whose
  lock is held stays, whatever its pid. The sweep and the making and locking of the new run's
  lock file happen under an exclusive lock on `<cwd>/chrome-profiles.lock`, so no sweep finds a
  lock file its run does not hold yet.
- The reaper and the guard run in the agent's jail, so they can signal nothing the agent could
  not. The guard holds no CDP connection of its own.

**Tests.** `crates/mcp/tests/guard/lifecycle.rs` runs the guard with stub Chrome and
agent-browser binaries (`examples/chrome_stub.rs`, `daemon_stub.rs`, whose daemon leaves the
process group and listens on the session's socket, and `parent_stub.rs`): stdin's end, a SIGKILL
of the guard, a SIGKILL of its parent while the test holds the guard's stdin, Chrome's exit, a
Chrome on another port, a Chrome that exits at start, and protected download names; after each,
no stub process is left and the run's profile and lock file are gone. `daemon_stub linger`
also leaves a process in agent-browser's group, which must be gone after the guard's exit and
after its SIGKILL.
`crates/mcp/tests/guard/races.rs` holds the guard in its wait for Chrome (a stub that never
prints its endpoint) and SIGKILLs the guard, or its parent, there: Chrome and the profile must be
gone, and, on Linux, every process whose command line names the test's folder; and a Chrome that
dies as the agent's stdin closes must still make the guard exit 1. It also runs the code owner's
repro: guard A is SIGKILLed once Chrome listens and guard B starts at once on the same `--cwd`
and ports; B's profile, staging folder and daemon must stay for 3 s, past A's reaper (on the
guard before per-run names it failed 5 of 5 runs, B's profile gone). And a daemon that binds its
socket only after the cleanup (`daemon_stub late`, in a group of its own) must be ended, after
the guard's SIGKILL and after its `done` (both failed without the reaper's second kill). Unit
tests cover the sweep (`runs.rs`) and the reaper dropping a pid the guard reaped. A unit test
puts a file under
the download's chosen name between the look and the move; it must not be replaced. `crates/mcp/tests/guard/real_chrome.rs` is the
regression test with the real agent-browser v0.41.1 and a real Chrome, through a test proxy that
serves `http://page.test`: `initialize`, `tools/list`, `agent_browser_open`, `_snapshot` (the
page's heading), four `_click`s that download `CLAUDE.md`, `claude.MD`, `.mcp.json` and
`AGENTS.md` (none arrives under a protected name), `_close`, and the guard's exit, after which no
process of Chrome's process group, no daemon and no process whose command line names the test's
folder (Chrome's crashpad handlers lead process groups of their own) is left; a SIGKILL of the
guard's parent after an `open`, after which the guard, Chrome and the daemon are all gone; and a
SIGKILL of the guard while it waits for Chrome's endpoint (Chrome's stderr hidden from it), once
Chrome and its helpers run, after which nothing of Chrome is left; and the code owner's repro
above with the real binaries, after which B's daemon is the same process and still serves B.
CI's Linux job runs
it with the pinned agent-browser and the runner's Google Chrome on merge groups and main
(`ci.yml`, "Fetch agent-browser for the guard's real-Chrome test"; not yet run there); without `CLAUSEO_TEST_AGENT_BROWSER` it skips with a note on
stderr. On the old guard it failed after 31 s with the `outcome_unknown` error above.

### Requirements on stage C's proxy and jail

The guard cannot enforce these; stage C's egress proxy and sandbox must.

- **The proxy refuses local destinations for browser sessions.** After DNS resolution, it
  refuses loopback, unspecified, private and link-local addresses (the ranges the `url` rule
  lists, and their IPv6 forms) for every browser session's request, with no "Allow this site?"
  card and no way for the lawyer to allow them. Reason: `AGENT_BROWSER_PROXY_BYPASS=<-loopback>`
  sends loopback through the proxy, and if the proxy forwarded it, a page at
  `http://127.0.0.1:<stream port>/` passes the stream server's Origin check (`http.rs:127-146`)
  and can post to `/api/command` (`http.rs:270-290`), which relaunches Chrome with any
  arguments, executable or profile; `/api/sessions` has no Origin check at all
  (`http.rs:249-268`). A name that resolves to a local address, a redirect, or a page's own
  `fetch` all reach the proxy without passing the guard's `url` check, which is why that check
  is only extra defence. The smoke test needs a test-only exception for its local page.
- **The proxy handles absolute-form requests.** For `http://` targets, Chrome and reqwest send
  `GET http://host/path` to the proxy, not `CONNECT`; the proxy must apply the same rules to
  those.
- **The jail allows one socket folder, made by the host.** On Unix agent-browser keeps its
  socket in a folder outside `--cwd` (the state path is over the 103-byte socket limit). Since
  #257 the host makes it (`BrowserSocketDir`: `/tmp/clauseo-ab-<16 random>`, mode 0700,
  lstat-checked, removed once the agent's process tree was killed; an agent that ignores its
  kill and the Unix daemon, which leaves the tree, can outlive that) and the jail may use only that
  folder, not create it. The guard makes nothing there: it checks the folder it is given and
  refuses to start on a failed check.
- **`<state>/browser` and the browser's loopback ports, in the jail: the jail adds nothing
  beyond itself.** This restates stage C's two requirements that `--cwd` (`<state>/browser`)
  sit outside what the agent can write and that the agent's own tools be kept off the daemon's
  socket and Chrome's CDP and stream ports (none has authentication, `daemon.rs:548`). On the
  in-jail path neither can hold: one Seatbelt profile cannot tell the agent from the browser,
  so everything Chrome may write or reach, the agent may too, and the guard, which checks its
  folders only at start (`browser_guard.rs`), cannot stop a later swap of one for a link. What
  keeps it safe is that a swapped link, or a direct CDP or stream call, reaches nothing beyond
  the jail: Seatbelt checks resolved paths, and Chrome is jailed too. Real separation needs
  Chrome in its own jail, the outside-jail fallback (`BrowserLaunch::OutsideJail`), which is a
  row of G17 item 1.
- **The host treats `<state>/browser` as agent-writable.** The host runs outside the jail, so it
  follows no link there when it reads or writes: agent-browser's config file (already so:
  `browser::write_config` refuses a linked folder and replaces a link at the file name),
  Chrome's profile, and downloads (the host reads neither yet; whoever adds the read keeps
  this rule).
- **Stage C's `BrowserSocketDir` and `ports::claim_port` merged in #257.** The host makes the
  socket folder per session (`AGENT_BROWSER_SOCKET_DIR = BrowserSocketDir::path()`) and claims
  the CDP, stream and Windows daemon ports (host part, "Ports and socket folder"). The jail
  policy is not built on main yet: R17 takes both from the launch's `RunFolders::browser_session`
  where it builds the session's policy, `PolicyBuilder::browser_socket_dir(&session.socket_dir)`
  and `Loopback::Ports(session.ports.loopback())` (CDP, stream, then the daemon's on Windows).
- **No daemon outlives its session** since the S27 launch fix: the guard, or its reaper when
  the guard is killed, ends it ("Chrome on the claimed port"). The host still does not kill it
  by PID.

### The host part (the second S27 pull request, `crates/host`)

#### Which Chrome (`host/src/browser/chrome.rs`)

The launch-prep thread (`actor/turn/launch_checks.rs`), never the actor, picks each session's
Chrome and keeps it on the session's `RunFolders` (`chrome`), which fills
`{browser_executable}`. It looks only when `tools.browser` is on and there is a proxy, so a
session that cannot get the browser makes no file system calls for it. In order:

1. Google Chrome installed: on macOS `/Applications/Google Chrome.app/Contents/MacOS/Google
   Chrome`, then the same under `~/Applications`; on Windows `%ProgramFiles%`,
   `%ProgramFiles(x86)%`, then `%LOCALAPPDATA%`, each with `\Google\Chrome\Application\chrome.exe`.
2. Windows only: Microsoft Edge (`%ProgramFiles(x86)%`, then `%ProgramFiles%`, with
   `\Microsoft\Edge\Application\msedge.exe`), only while `EDGE_ALLOWED` is true. It is false
   until G17 item 4.
3. Chrome for Testing in `<app data>/browsers/chrome-<version>/`, highest dotted version first,
   with the binary where the zip puts it: `chrome-mac-arm64/Google Chrome for
   Testing.app/Contents/MacOS/Google Chrome for Testing` (and `chrome-mac-x64/...`),
   `chrome-win64\chrome.exe`. A version folder without its binary is passed over. Skipped on
   Windows ARM64. `<app data>` is the host's data folder.
4. Otherwise none: `BrowserNotInstalled`.

The download that fills step 3 is the next section.

Linux (development and CI only): `/usr/bin/google-chrome`, then `/usr/bin/chromium`. A path
counts when it is a regular file, following links (an installed Chrome may be one). In
`<app data>/browsers/` a version folder must be a real folder (`file_type`, no link followed)
and the binary, links resolved, must sit under the browsers folder, so a planted link picks
nothing. That folder must stay outside what any agent can write (in tier C too). Every root comes in through `ChromeRoots`
(`HostConfig::with_chrome_roots`; production uses `ChromeRoots::system`, which reads `HOME`,
`ProgramFiles`, `ProgramFiles(x86)` and `LOCALAPPDATA`). The tests use one fixture folder per
case (`browser/chrome/tests.rs`), ARM64 included, and `tests/host/browser.rs` checks that the
Chrome found reaches `session/new`. `with_browser_executable` stays as a test-support override.

**agent-browser's config file.** The guard refuses an `AGENT_BROWSER_CONFIG` that names no
file, so the same prep, once it has found a Chrome, makes `<data>/browser/` (through no link
below the data folder, 0700 on Unix) and writes `{}` to `agent-browser.json` there, atomically,
at every such launch (`browser::write_config`, through the host's write path). What was there is
never kept: a planted file could set agent-browser's `provider` or `profile`. A link at the
file's name is replaced, never followed. A failed write is logged with the file's path and the
session gets no Chrome, so the entry drops with `BrowserNotInstalled`; since the Chrome is the
last of the drop reasons, an off switch, the firm's `managed-mcp.json`, no slot or no proxy
still shows first. `BrowserNotInstalled` is the nearest existing reason; a reason of its own
would be a protocol change. `tests/host/browser.rs` checks the file after a browser session
starts and that its path is the one the profile's entry names. The smoke test writes its own
config file, which stands in for this one.

#### Ports and socket folder (`host/src/browser/session.rs`)

For a session that can get the browser (`tools.browser` on, a proxy, a free slot, and a
`browser` entry from the profile hook), the launch-prep thread, after finding its Chrome,
claims a `BrowserSession`:

- `BrowserPorts { cdp, stream, daemon }`, each a stage C `PortClaim`: `cdp` and `stream` from
  `ports::claim_port()`, `daemon` on Windows only (below). No other session of this host is
  handed them while they are held. They fill `{cdp_port}`, `{stream_port}` and `{daemon_port}`.
- macOS and Linux: one `BrowserSocketDir` (`BrowserSocketDir::create()`), whose `path()`, the
  `/tmp/...` form, fills `{socket_dir}`; never `canonical()`, which on macOS is 8 bytes longer
  (`/private/tmp/...`) and counts against the socket path's 103 bytes.
- agent-browser's session name, `{session_id}`: the session's id on macOS and Linux.

The `BrowserSession` rides on the launch's `RunFolders` (in an `Arc`; the thread's turn state
is its one lasting holder) and drops with the run folder in `launch_checks::discard`, off the
actor: after the agent's process tree was killed (`clauseo-process` kills what is left of the
tree however the agent ends), or when the agent never started. Processes can outlive that
kill, and the claims drop anyway: an agent that ignores its kill (`StopStep::Unresponsive`,
logged), and any jailed process that calls `setsid`, which `killpg` does not reach, such as
agent-browser's Unix daemon, which the guard or its reaper ends ("Chrome on the claimed port").

**Residual risk: a claimed port taken by another host.** A claim holds within this host process
only (stage C's `ports`). Chrome binds at the guard's start (until lazy start), the stream
server and agent-browser's daemon only at the agent's first browser tool call, and the stream
server and the daemon fall back to another port
when theirs is taken. So a second host on the same machine (two OS users, or two Windows users
sharing 61080 to 61179) can be handed a port this host claimed but has not bound yet. This
host's jailed agent could then reach that host's Chrome on its CDP port, and this host's
`.port` and `.stream` files never appear, so the guard's checks never fire. The odds are low on
macOS (two hosts drawing the same ephemeral port in the window before the first tool call); on
Windows, S15's per-launch rules (exact ports per launch, never the whole range, and a
listener-side rule) close it; the upstream mitigation is a pipe or socket mode for CDP and the
stream (vercel-labs/agent-browser#2313). Stage C's next sandbox PR also retires released ports
for the host's life on Unix. Dropping the session releases the ports
and removes the folder and its lock file. A claim that fails, or a token the claims cannot fill
(`{socket_dir}` on Windows, `{daemon_port}` elsewhere), leaves the browser out with
`BrowserNotInstalled` and a log line.

At host start, once the store is open, `BrowserSocketDir::sweep_stale` removes the socket
folders an earlier host left in `/tmp` (no id is kept: no launch is live yet; a folder whose
lock another live host holds stays). `HostConfig::with_browser_socket_root` (test support)
moves both the folders and the sweep to a test folder.

**The Windows name search.** agent-browser derives its Windows daemon port from the session
name (`connection.rs` `get_port_for_session`, no namespace): start with `h: i32 = 0`; for each
character `c`, `h = (h << 5).wrapping_sub(h).wrapping_add(c as i32)`; the port is
`49152 + (h.unsigned_abs() % 16383)`. The jail's Windows range is 61080 to 61179, so the
remainder must be 11928 to 12027. The host draws random names of 16 characters from the guard's
charset (`A-Z a-z 0-9 _ -`, from the OS's random source) until a name's port is in the range and
`ports::claim_port_at(port)` claims it (`AddrInUse` draws again), at most 200,000 draws (a
name lands in range about once in 164), then fails typed (`ClaimError::NameSearchExhausted`).
That name is agent-browser's session name, and the port goes to the guard as `--daemon-port`,
which checks the daemon's `.port` file (guard part, "The ports agent-browser reports"). Upstream issue
vercel-labs/agent-browser#2313, a daemon port set by the caller, would remove the search.

**Missing sidecars.** The same prep checks that the browser entry's `clauseo-mcp` (its command)
and agent-browser (its `--agent-browser` argument) are files. A development build has no
agent-browser: a missing one leaves the browser out with `BrowserNotInstalled` and a log line
naming the file, and claims nothing.

#### The Chrome for Testing download (`host/src/browser/install.rs`, part 3)

agent-browser's own `install` writes under the home folder, which Clauseo cannot move on
Windows, so the host downloads Chrome for Testing itself, once the lawyer agreed.

**Wire.** `HostCommand::InstallBrowser {}` starts it and answers `Done` at once; while one runs
it answers `Done` and does nothing (no second download), except while a cancelled one is still
ending: then a new download starts once its `Failed { cancelled }` is out. `HostCommand::CancelBrowserInstall
{}` stops it, and does nothing when none runs. Neither stores a receipt. Every app subscriber gets
`HostEvent::BrowserInstall { state }` as it goes: some `Downloading { bytes, total }` (at most
four a second, only once `Content-Length` is known), `Unpacking`, then one `Done` or one
`Failed { code, message }`. The app's copy keys on `code` (`BrowserInstallFailure`, an open
enum: `network`, `size_mismatch`, `disk_space`, `host_refused`, `signature`, `unpack`,
`unsupported`, `cancelled`); `message` is for the log. The latest state goes into every app catch-up:
the running download's last step, or the last download's end until the next one starts. So a
window that lagged, resubscribed after a pause or the watchdog, or reloaded hears where the
download stands, its end included. The desktop bridge admits both commands from the main window only, and only main's app
feed carries the event (0012, 0028).

**Flow.** One download at a time, on a thread of its own; the actor never waits on it.

1. The machine: `mac-arm64`, `mac-x64` or `win64`. Linux, Windows ARM64 and the rest:
   `unsupported`. The platform and `<app data>` are the launch prep's `ChromeRoots`, so the
   search looks where the download writes.
2. `<app data>/browsers/` is made (0700, through no link). Under 1 GB free there
   (`statvfs` on macOS, `GetDiskFreeSpaceExW` on Windows): `disk_space`. A failed read only
   warns.
3. The version list, at most 1 MiB. Its URL must be `https://googlechromelabs.github.io/...`.
   Stable's `version` must be dotted digits (32 bytes at most), and the `chrome` URL for the
   platform must equal, byte for byte,
   `https://storage.googleapis.com/chrome-for-testing-public/<version>/<platform>/chrome-<platform>.zip`.
   Else `host_refused`, before anything downloads. The list has no hash (checked 2026-11-02),
   so integrity is HTTPS to Google's host plus the size check.
4. The zip goes into `browsers/.partial-<version>-<random>/chrome.zip`. Its size must equal
   the final response's `Content-Length`; a response without one fails too (`size_mismatch`).
5. Unpacking (`Unpacking`): first the zip's entry names are read in Rust, from the central
   directory and each local header (zip64 included). Refused: absolute names, `:` (drives and
   streams), `\`, `..`, `.` and empty segments (a folder entry's trailing `/` aside), segments
   ending in a dot or a space (Windows drops those), control characters, names that are not ASCII (macOS and
   Windows match names ignoring Unicode case and normalisation, so two spellings could slip an
   entry under a link entry), a name twice
   (ignoring ASCII case), a Unicode-path extra field naming another file, and any entry under a
   link entry (made on Unix or OS X), compared segment by segment, so every path has one
   spelling. Then `ditto -x -k` (macOS) or `tar -xf` (Windows) unpacks into `.partial-…/chrome/`.
   Then the result is walked without following links: a link must be relative, must never
   climb above the unpack folder at any step when read as text from its own folder (so it
   cannot leave and come back by that folder's name, which changes at the rename), and must
   resolve inside; anything not a file, folder or link is refused. The browser binary must be a file
   where the search expects it. Else `unpack`.
6. The signature (next paragraph).
7. Whatever is at `chrome-<version>` (a folder the search passed over, a file, a link, never
   followed) is removed, `chrome/` is renamed to `browsers/chrome-<version>/` and walked again
   there (removed if that fails), and the working folder goes.

Every failure removes the working folder. A cancel, the lawyer's or the host's shutdown, ends
the running tool's process tree and waits for it to exit before that removal; shutdown waits for
the download's last state. At start a thread removes every `browsers/.partial-*` a crash left
(folders with their contents, links and files themselves, never a link's target; a link at
`browsers` itself is not followed), and the first
download waits for it.

**Tools.** Each is an absolute path, never a `PATH` lookup: `/usr/bin/curl`, `/usr/bin/ditto` and
`/usr/bin/codesign` on macOS; `curl.exe` and `tar.exe` in `GetSystemDirectoryW()` (System32) on
Windows. A missing tool is `unsupported`. Each runs through `clauseo_process::spawn_in_tree`. curl
gets `-q` first (no `.curlrc`), then `--proto =https --proto-redir =https --tlsv1.2 --fail
--location --max-redirs 0 --max-filesize 419430400 --connect-timeout 30 -y 60 -Y 1` (`CURL_FLAGS`).
`--max-redirs 0` tightens the agreed `3` (security review): any redirect fails as curl's exit
47, `host_refused` (a retry would meet the same redirect). HTTPS to Google's host is the whole
integrity check while the signature does not enforce, so a redirect to another https host
must not replace it; neither URL redirects today (checked 2026-11-04). curl's exit 63 (over
the 400 MiB cap) is `size_mismatch`, any other failure `network`. Progress comes from polling the file's size and the `-D` header file every 250 ms. The tools sit behind
`InstallTools`; the tests use fakes (`HostConfig::with_browser_install_tools`).

**Signature.** macOS: `codesign --verify --deep --strict` on the app bundle, then `codesign -dv`
must show `TeamIdentifier=EQHXZ8M8AV` (Google). Windows: `WinVerifyTrust` (Authenticode) on
`chrome.exe`, without fetching revocation. The result is logged; it fails the download
(`signature`) only while `SIGNATURE_REQUIRED_MAC` or `SIGNATURE_REQUIRED_WINDOWS` is true. Both
are false: nobody has checked here whether Chrome for Testing is signed. The by-hand check below
decides, and a later PR flips the constant.

**Quarantine.** curl sets no `com.apple.quarantine` attribute, so Gatekeeper does not check the
download when Chrome starts. HTTPS to Google's host, the size check, and the logged `codesign`
result are the checks today.

**By hand.** On a Mac (arm64 and Intel) and on Windows x64:
`cargo test -p clauseo-host --lib real_download -- --ignored --nocapture` downloads the real zip
into a temporary folder. Record what it logs, then run `codesign -dv --verbose=4` on the app, or
`Get-AuthenticodeSignature` on `chrome.exe`, and note whether it passes.

#### Slots and limits (`host/src/browser/limits.rs`)

`slot_limit(total_memory_bytes)` is 2 at 8 GiB or less, 4 above. The idle timeout (600000 ms),
default timeout (30000 ms) and max output (25000 characters) are the guard's alone
(`crates/mcp`); the host keeps no copy. PR-B's `with_browser_slots`
stand-in is gone. The actor holds a `BrowserSlots` (the limit and the set of `SessionKey`s
holding one). `profile_loads::session_loads` checks it before filling the entry
(`TooManyBrowsers` when none is free; a session that already holds one keeps it, so its
`session/load` after a `session/new` counts once), and takes the slot when the entry is filled.
`launch_checks::discard`, which every way out of a session goes through (the agent's end, a failed
launch, a stopping host), frees it. Total memory comes in through
`HostConfig::with_machine_memory(bytes)`; the default, unknown, means 2. The workspace had no
memory reader to reuse (searched for `sysinfo`, `total_memory`, `MemTotal`, `hw.memsize` and
`GlobalMemoryStatus`); the desktop fills it in a later PR.

#### Where Chrome runs (`BrowserLaunch`)

`HostConfig::with_browser_launch` takes `BrowserLaunch::InJail` (the default, the only one
built) or `OutsideJail { cdp_port }`, stage C's switch, under which the entry would carry
`AGENT_BROWSER_CDP` instead of starting Chrome. `OutsideJail` is refused for now with
`UnsupportedLaunch` ("... is not built yet"), tested in `config/tests.rs`.

#### Seed and deny lists

`host/src/browser/seed_sites.rs`, `SEED_SITES`: the eight national sites, then the 25 High
Courts, each court's own site:

| Court | Host | Resolves |
|---|---|---|
| Allahabad | `allahabadhighcourt.in` | 221.134.71.211 |
| Andhra Pradesh | `aphc.gov.in` | 45.127.64.27 |
| Bombay | `bombayhighcourt.nic.in` | 164.100.229.168 |
| Calcutta | `calcuttahighcourt.gov.in` | 202.61.117.204 |
| Chhattisgarh | `highcourt.cg.gov.in` | 103.195.217.193 |
| Delhi | `delhihighcourt.nic.in` | 45.127.75.133 |
| Gauhati | `ghconline.gov.in` | 157.20.175.29 |
| Gujarat | `gujarathighcourt.nic.in` | 103.195.217.201 |
| Himachal Pradesh | `hphighcourt.nic.in` | 164.100.138.103 |
| Jammu & Kashmir and Ladakh | `jkhighcourt.nic.in` | 164.100.220.61 |
| Jharkhand | `jharkhandhighcourt.nic.in` | 164.100.211.140 |
| Karnataka | `judiciary.karnataka.gov.in` | 103.138.198.32 |
| Kerala | `highcourt.kerala.gov.in` | 103.135.131.179 |
| Madhya Pradesh | `mphc.gov.in` | 117.239.39.50 |
| Madras | `hcmadras.tn.gov.in` | 103.59.64.38 |
| Manipur | `hcmimphal.nic.in` | 164.100.188.18 |
| Meghalaya | `meghalayahighcourt.nic.in` | 103.195.218.129 |
| Orissa | `orissahighcourt.nic.in` | 103.195.219.33 |
| Patna | `patnahighcourt.gov.in` | 117.244.242.24 |
| Punjab and Haryana | `highcourtchd.gov.in` | 164.100.221.50 |
| Rajasthan | `hcraj.nic.in` | 103.195.217.203 |
| Sikkim | `hcs.gov.in` | 164.100.126.241 |
| Telangana | `tshc.gov.in` | 164.164.39.3 |
| Tripura | `thc.nic.in` | 164.100.178.45 |
| Uttarakhand | `highcourtofuttarakhand.gov.in` | 164.100.52.170 |

Checked with `getent hosts <host>` on 2026-11-03 between 01:09 and 01:12 UTC, from a Linux
machine outside India: all 25 resolve, and so do the eight national sites (`rbi.org.in` and
`indiacode.nic.in` gave IPv6 addresses only). Each host was matched to its court by the page it
serves or a court or eCommittee listing. Bombay's and Kerala's sites did not answer HTTPS from
that machine (DNS resolves); Bombay's host rests on a directory listing. Karnataka has no
dedicated domain: `judiciary.karnataka.gov.in` serves "High Court of Karnataka". Madras's older
`hcmadras.tn.nic.in` also resolves; the current `.gov.in` host is listed.

Re-checked on 2026-11-03 at 02:31 UTC after review, with `getent hosts` and `curl -sI` over
HTTPS:
- Kerala: `hckerala.gov.in` does not resolve; `highcourt.kerala.gov.in` resolves
  (103.135.131.179) and answers HTTPS 200 with the title "HIGH COURT OF KERALA". Kept.
- Madras: `hcmadras.tn.gov.in` resolves (103.59.64.38) and answers HTTPS 200 with the title
  "Madras High Court": the court's home page, which links to everything else. `mhc.tn.gov.in`
  resolves (164.100.134.181) and hosts the court's services (`/judis/` judgments and cause
  lists, case status, recruitment), but its `/` redirects to `/recruitment/` and its
  certificate chain did not verify from here (curl exit 60). Kept `hcmadras.tn.gov.in`; the
  judgments host `mhc.tn.gov.in` is not seeded, so it asks once per matter. Whether to seed it
  too is for G17.
- Allahabad: `allahabadhighcourt.in` is the only seeded host on an openly registrable domain
  (not `gov.in` or `nic.in`), so it alone could lapse or be squatted; a seed list bump must
  re-check who holds it. Guessed hosts that do
not resolve (`karnatakahighcourt.gov.in`, `nainitalhc.gov.in`, `sikkimhighcourt.nic.in`,
`tripurahighcourt.nic.in`, `manipurhighcourt.gov.in`, `orissahighcourt.gov.in`,
`highcourtofkerala.gov.in`) are not listed. Rules match exact hosts, so a site that redirects to
`www.` will ask once for that host; G17 or stage C's proxy decides whether `www.` variants are
seeded too. The unit tests never touch the network.

`host/src/browser/deny_hosts.rs`, `DENY_HOSTS`, **provisional until G17 item 5**: Chrome's
background hosts the proxy denies with no card, each with a one-line reason. agent-browser's
flags (`--disable-background-networking`, `--disable-component-update`, `--disable-sync`,
`chrome.rs:466-480`) leave the component updater (`update.googleapis.com`,
`clients2.google.com`, `redirector.gvt1.com`, `edgedl.me.gvt1.com`), Safe Browsing
(`safebrowsing.googleapis.com`, `sb-ssl.google.com`; no `--safebrowsing-disable-auto-update`),
the optimization guide (`optimizationguide-pa.googleapis.com`), autofill predictions
(`content-autofill.googleapis.com`), the variations seed (`clientservices.googleapis.com`) and
sign-in checks (`accounts.google.com`). The denial applies to browser sessions only, never to
the agent's own traffic. `accounts.google.com` in particular is provisional for G17 item 5:
denying it without a card breaks Google sign-in on every site. Stage C's proxy reads both
lists; the `browser` module is public for it.

### Bundling agent-browser

#### Bundling (desktop)

A release build ships agent-browser as a sidecar binary for each target. Nothing is committed:
the release fetches the pinned asset, checks its hash and writes it into the desktop crate.

- **The pin**, `crates/desktop/agent-browser.toml`: `version = "0.41.1"`, then one table per Rust
  target triple with `url` and `sha256` (lowercase hex) of the release's raw binary (no archive).
  A triple with no build may hold `none = true` and be left out explicitly; none does today, and the Rust
  and node pin tests refuse it, so a future `none` is a deliberate change to those tests. The
  file's header states the format. Each hash was taken from a download of the asset and matches
  the digest GitHub lists for it. `crates/desktop/src/agent_browser_pin.rs` (the
  `agent_browser_pin` test) checks the file: semver version, the six triples, each url under the
  pinned release with its asset name, each sha256 in shape.
- **The fetch**, `crates/desktop/sidecars/agent-browser.mjs`: `fetchAgentBrowser(triple,
  destPath)`, Node built-ins only. It reads the pin, throws on a triple the pin does not name,
  accepts only an https URL on `github.com/vercel-labs/agent-browser/releases/download/v<version>/`
  whose asset name is one plain segment (no `/`, no `%`), and downloads it with a 120 s timeout.
  The response must be HTTP 200 and at most 64 MB (a larger Content-Length is refused up front; a
  larger body is refused while reading). Redirects are followed; the SHA-256 of the bytes decides
  trust. A mismatch throws and writes nothing. Otherwise it writes `<destPath>.partial`, sets mode
  0755 on non-Windows and renames it into place. Its tests, `sidecars/agent-browser.test.mjs`, stub
  the network: `node --test 'crates/desktop/sidecars/*.test.mjs'` (Node 24 does not take a folder
  argument). CI does not run them, because the xtask check is frozen.
- **Stage F's script**, the release's sidecar script (not in this PR), calls `fetchAgentBrowser` once per release
  target and writes `crates/desktop/binaries/clauseo-agent-browser-<triple>[.exe]`. It is not in this PR.
  The base Tauri config has no `externalBin`, and this PR does not add one.
- **The installed name** is `clauseo-agent-browser` (`.exe` on Windows), under 0035's `clauseo-`
  prefix rule, since the `.deb` and `.rpm` put sidecars in `/usr/bin`; the desktop passes that
  path as `--agent-browser` (`crates/desktop/src/profile.rs`). agent-browser's own image-name check
  (`connection.rs:198`, `is_agent_browser_process`, which wants the name to start with
  `agent-browser`) then skips its stale-daemon kill. On Windows the guard's Job Object (#329) covers
  that; on Unix the guard ends the daemon itself ("Chrome on the claimed port").
- **Windows ARM64** has no upstream build. `aarch64-pc-windows-msvc` takes the x64 asset, which
  Windows on ARM runs under x64 emulation; the pin says so in a comment.
- **The license**: `LICENSES/Apache-2.0-agent-browser.txt`, a copy of upstream's `LICENSE` (upstream
  has no NOTICE file), listed in `LICENSES/README.md`.
- **Machine memory**: the desktop reads the machine's total physical memory once at start-up
  (macOS `hw.memsize`, Windows `GlobalMemoryStatusEx`, Linux `MemTotal`) and passes it with
  `HostConfig::with_machine_memory`, so the browser limit is 2 sessions at 8 GB or less and 4
  above. Before this, production took the 2-browser default. The read runs on the thread that
  starts the host, before `Host::start`. If it fails, nothing is passed and a warning is logged.

#### `PageVisited` (`host/src/sources.rs`, `sources/browser.rs`)

The host records one `PageVisited { via: Browser }` per completed open, built with
`PageVisited::new` (S26's caps and `logged_url`), committed after the tool call's own event:

- **The tool, by name.** Claude: `mcp__browser__agent_browser_open`, in the call's `name` or
  `title`. Codex: `mcp.browser.agent_browser_open`. codex-acp 2.2.2 titles an MCP call
  `` `mcp.${item.server}.${item.tool}` `` with kind `execute` and no `name`
  (`dist/index.js:29492-29503`); its `rawInput` is `{ server, tool, arguments }`
  (`:29520-29522`); the completed update repeats the input and carries `{ result, error }` as
  `rawOutput` (`:29507-29513`, `:29524`), with no title and no `content`.
- **The URL** is the `url` of the last `rawInput` the host had for that tool call id (top level,
  or under Codex's `arguments`), read when an update reports `completed`. A value with no `:`
  gets `https://` in front, as agent-browser does; then only `http` and `https` URLs count, so
  `about:blank` and other schemes write nothing. An input stored as a blob is passed over.
- **An error result is no visit.** A completed call whose `rawOutput` has a non-null `error`
  (Codex) or `isError: true` (at its top level or under `result`), or whose text starts with the
  guard's "Clauseo does not allow", writes nothing.
- **The title** is the tool result's text, its first line: the first text content (Claude), else
  the first text of `rawOutput`'s MCP result (`content`, or Codex's `result.content`). For an open
  agent-browser's text is the page's title (`cli/src/mcp.rs`, `response_text` takes `title` before
  `url`). A text equal to the URL gives no title. Control characters and the bidi formatting
  characters (U+202A to U+202E, U+2066 to U+2069) are taken out, so a page cannot spoof the
  sources list.
- **Only the live turn.** The thread keeps its unfinished opens across turns and agent restarts
  (`TurnState::browser_opens`, at most 64), but records a page only for a completion in the live
  turn: `Origin::Late` (after a cancel) and `Origin::Replay` (an agent's replay on `session/load`)
  are dropped by their origin first (`actor/turn/updates.rs`). The tests leave an open running,
  then send its completion late or in a replay, and see no page. A failed, cancelled or unfinished
  call writes nothing.

The scenario `crates/host/tests/scenarios/browser-visit.jsonl` (`tests/host/browser.rs`,
`a_completed_open_is_one_page_visited`) opens `https://sci.gov.in/judgments#top`, clicks, and
fails a second open: one `PageVisited` with the URL (fragment dropped), the title, the turn and
the open's tool call, after its completed update; none for the click or the failed open; and the
reopened host's log replays the one page.

#### No daemon kill by PID

The host does not kill agent-browser's daemon from its `.pid` file (above). The guard ends it, by
the pid of the process listening on its socket, not by the file ("Chrome on the claimed port").

#### Budgets

`budgets.toml` gains `browser_cold_start_p95` (3 s) and `browser_session_memory_p95` (600 MB),
both `alert` and `unmeasured`.

#### The smoke test, by hand (`crates/host/tests/browser_smoke.rs`)

`browser_smoke` is `#[ignore]`d, so normal runs skip it. It starts a logging proxy on loopback
(`CONNECT` and absolute-form HTTP; `smoke.test` resolves to a page server the test runs), starts
`clauseo-mcp browser-guard` with a temp `--cwd` and the four variables the entry sets, and speaks
MCP: `initialize`, `tools/list`, `agent_browser_open` on `https://example.com`,
`agent_browser_snapshot`, `agent_browser_open` on `http://smoke.test:<port>/`, then a WebRTC
page on `smoke.test` that gathers ICE candidates, reports them with a `fetch` to `smoke.test`,
and loads an image from `http://127.0.0.1:<port2>/pixel`; then `agent_browser_close`. It passes
when the proxy log holds only `example.com:443`, `smoke.test:<port>` and `127.0.0.1:<port2>`,
the pixel came through the proxy, no UDP `host` or `srflx` candidate was reported, and the
guard's `chrome-profile-<run id>` is gone from `--cwd` once the guard exited (the test first checks it
existed while Chrome ran). example.com's page no longer has an "Example Domain" heading, so the
snapshot check looks for its paragraph ("documentation examples"). The guard's `url` check refuses loopback hosts, so the pages use `smoke.test`.
The WebRTC page also reports `iceGatheringState` and whether its SDP has an `m=application`
line; both must show gathering ran (`complete`, true), so an empty candidate list cannot pass
because WebRTC never started. The page also `fetch`es `http://127.0.0.1:<port2>/pixel2`, and both
loopback requests must carry the proxy's marker header.

Limits: the proxy log sees only what reaches the proxy, so "no other host" cannot catch a
connection that bypasses it; only the two loopback requests, which arrive marked by the proxy,
prove the path. Hosts from `DENY_HOSTS` in the log are printed apart and do not fail the test:
they are G17 item 5's finding (which background hosts Chrome still reaches), not a leak around
the proxy.

`crates/host/tests/browser_smoke.rs` (ignored in normal runs). Run it on the Mac mini and on the
Windows VM:

1. Build `clauseo-mcp` from branch `s27-mcp-guard`: `cargo build -p clauseo-mcp --release`.
2. Get agent-browser v0.41.1 (`npm i -g agent-browser@0.41.1`, or the release binary from
   <https://github.com/vercel-labs/agent-browser/releases/tag/v0.41.1>) and Chrome.
3. Set, each to an absolute path:
   - `CLAUSEO_SMOKE_MCP`: the `clauseo-mcp` binary;
   - `CLAUSEO_SMOKE_AGENT_BROWSER`: the agent-browser native binary;
   - `CLAUSEO_SMOKE_CHROME`: the Chrome executable.
4. Run `cargo nextest run -p clauseo-host --run-ignored only -E 'test(browser_smoke)'`.

It needs the network (it opens `https://example.com`). It passes when every request Chrome made
went through the test proxy (only `example.com:443`, `smoke.test:<port>` and the loopback pixel
`127.0.0.1:<port>`), the WebRTC page reported no UDP `host` or `srflx` candidate, and the
guard's `chrome-profile-<run id>` is gone from `--cwd` after the guard exits. On failure it
keeps its folder under the temp folder and prints the proxy log.

## Deviations from the S27 brief

The host part:

- The Codex open's name is `mcp.browser.agent_browser_open` (codex-acp's title), matched
  beside Claude's `mcp__browser__agent_browser_open`; an open whose `tool_call` already says
  `completed` is recorded too.
- The slot is freed in `launch_checks::discard`, the one path every session end takes, rather than
  at a separate "session ended" hook.
- `HostConfig::with_browser_launch` returns a `Result`: `OutsideJail` is refused when the host is
  configured, not at each session.
- Chrome is searched only when `tools.browser` is on and a proxy is configured; production has no
  proxy until stage C, so it never searches yet.
- `seed_sites.rs`'s constant changes from `[&str; 8]` to `&[&str]`, and `browser` becomes a
  public module so stage C's proxy can read both lists.
- `with_browser_executable` stays as a test-support override beside `with_chrome_roots`.
- The host writes agent-browser's config file (`{}`) at every launch prep that found a Chrome;
  a failed write is reported as `BrowserNotInstalled`, the nearest existing reason.
- `BrowserLaunch::InJail` is accepted and not stored: it is what the host does anyway.
- This file is copied from PR #200's branch with the host part added; it merges with #200's copy
  when either lands.
- The smoke test is longer than planned (about 840 lines: `browser_smoke.rs` and four modules in
  `browser_smoke/`, two with an allowlisted `#![expect(clippy::disallowed_methods)]`) and has never
  run here: this machine has no agent-browser and no Chrome.

The guard:

- `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (and lower case on Unix) are set, to Clauseo's proxy;
  the brief said none. The brief's aim, never passing the agent's values, holds.
- `AGENT_BROWSER_SCREENSHOT_DIR` is added; `path` and `screenshotDir` are refused keys.
- `extraArgs` takes only the six exact tokens; the brief refused only unknown dash tokens.
- New rules: the `url` scheme check, the flag-shaped strings in typed values, batches and scalars
  refused, the 16 MiB line limit, and the close request on stdin EOF.
- The child starts through `std::process::Command`, not the `process` crate (above).
- `ARCHITECTURE.md`'s row lists `tauri, tokio, reqwest, hyper, ureq, curl, rustls, native-tls`
  under "Never depends on", not "tauri, any HTTP client": the crates-table check reads only crate
  names there. Status is `exists`, the table's word, not `active`. The dependency test also bans
  tokio.
- `serde` is not a direct dependency: `serde_json` alone does the work.
- `AGENT_BROWSER_NAMESPACE` is not set; the brief set it to the session. With it, the Unix socket
  path is too long (above).
- `AGENT_BROWSER_SOCKET_DIR` is `/tmp/clauseo-ab-<16 random>` on Unix, not `<cwd>/run`, made by
  the host and checked by the guard; Windows keeps `<cwd>\run`.
- On Unix agent-browser's session name is the guard run's own, `run-<run id>`, not the session
  id the host passes ("Each run's own names"); Windows keeps the host's.
- The ports-and-socket PR adds `--daemon-port <port>` (Windows only) to the guard, filled from
  the Windows-only `{daemon_port}` token, so the guard can check agent-browser's `.port` file;
  the brief listed only `--cdp-port`. The host does not see when the daemon starts (on the
  agent's first browser call, inside the agent's MCP child), so the check sits in the guard.
  The guard also takes an empty `.port` file as not written yet, so a reply relayed while the
  daemon writes it cannot kill a good session.
- After the security review: the guard reads the port files after every relayed line for the
  session's whole life (a match no longer ends the checks); it checks the stream server's
  `<session>.stream` too, on every OS; and on Windows agent-browser runs in a Job Object the
  guard owns, so a refusal ends the daemon and Chrome, not just the direct child. `crates/mcp`
  gains `windows` (Windows only) for it, with one `unsafe_code` expect in `job.rs`;
  `clauseo-process` has the same steps but for an async child, with dependencies this crate may
  not take.
- The host depends on `clauseo-sandbox` (`BrowserSocketDir`, `ports`) and `getrandom` (the
  Windows names); `ARCHITECTURE.md`'s row lists both.
- `AGENT_BROWSER_CONFIG` and `AGENT_BROWSER_EXECUTABLE_PATH` must name existing files, and on
  Windows not network or device paths; the brief asked only for absolute paths. Windows also
  gets `SystemRoot` set when absent.
- Rules from the security review, beyond the brief: `newTab` and `webgpu` refused unless
  `false`, `timeoutMs` capped at 300000, a path-like screenshot `selector` refused, local and
  private `url` hosts refused, the reserved id refused, the flag shape narrowed and
  `eval.script` exempted, the 5 s exit wait and kill, the 64 MiB child line cap, and a cut-off
  last child line dropped.
- `AGENTS.md` lost the Map's comment line to fit the new `crates/mcp` line within its 120 lines
  and 8 KiB; `ARCHITECTURE.md` already says planned crates are in its table.

The download (part 3):

- The brief's `Failed { message }` also carries a `code`, so the app picks its copy without
  reading English.
- The tests use fake tools (`InstallTools`), not a local stub server: curl accepts only https,
  so a stub would need a trusted certificate. The real tools have an ignored by-hand test.
- A truncated download fails the size check (`size_mismatch`), and a broken zip fails the name
  check (`unpack`). Neither leaves a folder.
- A second `InstallBrowser` while one runs is not refused, as the brief said: it answers `Done`
  and starts nothing. A refusal needs a new error code and a user-facing string, and the
  running download's `BrowserInstall` events already say one is under way.

## Consequences

- An agent-browser bump re-reads its `tools/list` schema and `extraArgs` handling: a new typed
  field that changes isolation passes the guard until it is added to the refused keys.
- The guard never forwards bytes it did not parse, so the agent cannot rely on key order or
  number formatting reaching agent-browser (an integer beyond `u64` becomes a float).
- A child line over 64 MiB ends the session: a page large enough to produce one fails rather
  than exhausting the guard's memory.
- Chrome runs from the session's start to its end, used or not, until lazy start (the next pull
  request, a release blocker). Browser state survives `agent_browser_close` until the session
  ends.
- A Chrome crash ends the browser server for the session (the guard exits 1); restart-once
  comes with lazy start.

## Not verified

- Windows: compiled with `cargo clippy --target x86_64-pc-windows-gnu`, never run.
  `CREATE_NO_WINDOW`, the case-insensitive pass-through on a real environment block, and the
  daemon's end under the Job Object are unverified.
- The guard in front of the real agent-browser: `browser_smoke` exists but has not run yet (by
  hand, on the Mac mini and the Windows VM).
- Which Chrome on a real Mac or Windows machine: only fixture folders.
- That every host in `DENY_HOSTS` is still contacted by Chrome with agent-browser's flags (G17
  item 5).
- reqwest's reading of the proxy variables is from its documented behaviour, not from a run.
- The URL host reader follows the WHATWG URL standard as written; it was not compared with
  Chrome's or the `url` crate's parser on the same inputs.
- The socket folder's owner and mode check ran on Linux only, not macOS.
- The Windows name search and the port-file checks: their logic runs as tests on Linux; the
  Windows wiring (`claim_port_at` in the jail's range, the read with
  `FILE_FLAG_OPEN_REPARSE_POINT`, the guard's Job Object) passes `cargo clippy --target
  x86_64-pc-windows-gnu` and was never run here; `tests/guard/tree.rs` runs it on CI's Windows
  job. That Chrome honours the later `--remote-debugging-port` over agent-browser's own `=0`
  was from Chrome's switch rule, not a run; it held, but broke the launch ("Chrome on the claimed
  port"). The fix ran on Linux (iris, Chrome for Testing 147); CI runs the real-Chrome test on
  merge groups and main, which it has not yet done.
- `browser_smoke` on iris (Chrome for Testing 147, after the launch fix): the pages, the guard's
  exit and the profile check ran, then it failed on Chrome background hosts outside `DENY_HOSTS`
  (`www.google.com:443`, `android.clients.google.com:443`, `mtalk.google.com:5228`). agent-browser's
  own launch reaches the first two as well, so they are G17 item 5's finding, not the fix's; its
  WebRTC and pixel checks, which come after, did not run.
- The S27 launch fix on macOS and Windows: not run. The Windows parts (Chrome in the job, the
  parent handle opened at the start, the no-replace download move with `MoveFileExW` and its
  `AlreadyExists` mapping and path normalisation, with their unit tests) pass `cargo clippy
  --target x86_64-pc-windows-gnu` only; CI's Windows job runs those tests. `LOCAL_PEERPID` on
  macOS is from nix's documentation and `cargo check`, not a run. The per-run lock files
  (`File::lock`: `flock` on macOS, `LockFileEx` on Windows) and the sweep ran on Linux only.
- The Unix parent check counts only pid 1 as a parent gone at the start; under a subreaper a
  parent that died first goes unnoticed until stdin's end ("The parent's end"). Not fixed.
- The reaped-pid lines close the window only from the guard's side: a guard killed between
  reaping Chrome after its own exit (the 250 ms watcher) and writing `reaped chrome` leaves the
  reaper the pid for microseconds. Hitting another process then needs that pid reused as a
  process group leader in between; not tested.
- The Chrome for Testing download with the real tools: run only through fakes on Linux. Not
  run: curl, `ditto` or `tar`, `codesign` and `WinVerifyTrust`, the free-space reads, and
  whether Chrome for Testing is signed (the by-hand steps above). The Windows code
  (`install/windows.rs`) was checked with clippy for `x86_64-pc-windows-msvc` in a crate of its own.

## Status

proposed

## Date

2026-11-02
