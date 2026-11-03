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
browser-guard)` on stderr and exits 2. Dependencies: `serde_json` only. Plain threads, no async
runtime, no HTTP client; `crates/mcp/tests/guard/deps.rs` walks `Cargo.lock` and fails if
reqwest, hyper, hyper-util, h2, ureq, curl, isahc, attohttpc, rustls, native-tls, openssl,
socket2, mio, tokio, async-std, smol or tauri becomes reachable.

**The command line.** Exactly `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --
<args...>`, in that order. `<file>` must be an absolute path to an existing file and `<folder>`
an absolute path to an existing folder. The child's arguments are passed exactly as given.
Anything else exits 2 with a reason and the mode's usage line.

**The environment.** The child gets exactly this map (`env.rs`), nothing inherited:

| Variable | Value |
|---|---|
| `AGENT_BROWSER_CONFIG`, `AGENT_BROWSER_EXECUTABLE_PATH` | from the guard's environment, set by the host; each must be an existing file at an absolute path; on Windows a path starting with two slashes or backslashes (a network share or device path) or `\??` is refused before anything reads it |
| `AGENT_BROWSER_PROXY` | from the guard's environment; exactly `http://127.0.0.1:<port>`, port 1 to 65535, no leading zero |
| `AGENT_BROWSER_SESSION` | from the guard's environment; 1 to 64 of `A-Z a-z 0-9 _ -` |
| `AGENT_BROWSER_PROXY_BYPASS` | `<-loopback>`, so loopback goes through the proxy too |
| `AGENT_BROWSER_ARGS` | `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` |
| `AGENT_BROWSER_IDLE_TIMEOUT_MS` | `600000` |
| `AGENT_BROWSER_MAX_OUTPUT` | `25000` (characters; see below) |
| `AGENT_BROWSER_DEFAULT_TIMEOUT` | `30000` |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (and lower case on Unix) | the validated proxy URL; no `NO_PROXY` |
| `AGENT_BROWSER_DOWNLOAD_PATH`, `_SCREENSHOT_DIR` | `<cwd>/downloads`, `<cwd>/screenshots` |
| `AGENT_BROWSER_SOCKET_DIR` | Unix: `/tmp/clauseo-ab-<first 16 characters of the session>`; Windows: `<cwd>\run` |
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

**The Unix socket folder.** `<cwd>/run` can be too long for a socket path, so on Unix the socket
folder is `/tmp/clauseo-ab-<first 16 characters of the session>` (`browser_guard/socket.rs`).
The guard makes it with mode 0700 through its write module. If it exists already, it must be a
real folder (`symlink_metadata`, not a link) with mode 0700 and the same owner uid as `--cwd`;
otherwise the guard exits 2 with a stderr line. Then `<socket folder>/<session>.sock` must be at
most 103 bytes (it is at most 102 with a 64-character session), else exit 2. On Windows the
socket folder stays `<cwd>\run`: agent-browser uses loopback TCP there, and the folder holds only
`.port` and `.pid` files. Two sessions whose ids share their first 16 characters share the folder,
each with its own socket file. The host removes the folder at session end.

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
start through the `process` crate: this crate may depend only on serde and serde_json, the child
is one fixed program, and the host starts the guard itself through the `process` crate, whose
tree kill reaches the guard and agent-browser. One `#[expect]` at `spawn_child` in
`crates/mcp/src/browser_guard/relay.rs` carries it. The guard exits with the child's exit code, or
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

The close is best-effort because agent-browser's daemon detaches. On Unix it is started with
`setsid` (`connection.rs:947-951`): it leaves the guard's process group and is reparented, so
the `process` crate's group kill and any process-tree kill miss it, and **the daemon and Chrome
outlive a kill of the guard**. The host does **not** kill the daemon from the `<session>.pid`
file: a PID read from a file can name a process the system reused, and AGENTS.md forbids killing
by such a PID. The guard's close request on stdin EOF (above) is the normal end, and the idle
timeout the backstop; how often a daemon outlives its session is a follow-up, measured in G17
item 6 (host part, "No daemon kill by PID"). On
Windows it is started with `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`
(`connection.rs:963-983`), which does not leave a Job Object, so the job kill should still end
it (not verified). A daemon nobody kills ends after the idle timeout, 10 minutes.

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
  lstat-checked, removed after the launch's process tree exits) and the jail may use only that
  folder, not create it. Until the ports-and-socket follow-up lands, the guard's own `mkdir`
  there is refused in the jail.
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
  `DevToolsActivePort`, and downloads (the host reads neither yet; whoever adds the read keeps
  this rule).
- **Stage C's `BrowserSocketDir` and `ports::claim_port` merged in #257.** The ports-and-socket
  follow-up moves the socket folder's creation to the host
  (`AGENT_BROWSER_SOCKET_DIR = BrowserSocketDir::path()`) and pins the CDP, stream and Windows
  daemon ports.
- **A daemon that outlives its session** (a kill of the guard on Unix) ends only at the idle
  timeout; the host does not kill it by PID (above). Follow-up: G17 item 6.

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

The Chrome for Testing download is added by a later S27 PR (part 3).

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

Added by a later S27 PR.

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

The host does not kill agent-browser's daemon from its `.pid` file (above): the guard's
`agent_browser_close` on stdin EOF and the 10-minute idle timeout end it. A follow-up, measured in
G17 item 6.

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
the pixel came through the proxy, no UDP `host` or `srflx` candidate was reported, and no
`agent-browser-chrome-*` temp profile is left under `--cwd` (the test first checks one existed
while Chrome ran). The guard's `url` check refuses loopback hosts, so the pages use `smoke.test`.
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
`127.0.0.1:<port>`), the WebRTC page reported no UDP `host` or `srflx` candidate, and no
`agent-browser-chrome-*` temp profile is left under `--cwd` after the close. On failure it
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
- `AGENT_BROWSER_SOCKET_DIR` is `/tmp/clauseo-ab-<16 characters>` on Unix, not `<cwd>/run`, made
  and checked by the guard; Windows keeps `<cwd>\run`.
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

## Consequences

- An agent-browser bump re-reads its `tools/list` schema and `extraArgs` handling: a new typed
  field that changes isolation passes the guard until it is added to the refused keys.
- The guard never forwards bytes it did not parse, so the agent cannot rely on key order or
  number formatting reaching agent-browser (an integer beyond `u64` becomes a float).
- A child line over 64 MiB ends the session: a page large enough to produce one fails rather
  than exhausting the guard's memory.
- A Unix daemon outlives a kill of the guard until the idle timeout ends it (10 minutes); the
  host does not kill it by PID. G17 item 6 measures how often that happens.

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

## Status

proposed

## Date

2026-11-02
