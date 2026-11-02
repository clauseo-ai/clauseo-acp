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
runtime, no HTTP client; `crates/mcp/tests/guard/deps.rs` walks `cargo metadata` and fails if
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
outlive a kill of the guard**. The host's session end must therefore kill the daemon from the
`<session>.pid` file in the socket folder (S27's host PR does this), then remove the folder. On
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
- **The jail allows `/tmp/clauseo-ab-*`.** On Unix the guard and agent-browser make and use the
  socket folder `/tmp/clauseo-ab-<16 characters>`, outside `--cwd`; the jail must let them
  create and use it, and should keep the agent's own tools out of it (the daemon's socket has no
  authentication, `daemon.rs:548`).
- **`--cwd` sits outside what the agent can write.** The guard checks its folders only at start
  (`browser_guard.rs`); an agent that can write `--cwd` could later swap one for a link.
- **The host kills the daemon at session end** from the `.pid` file in the socket folder (above;
  S27's host PR).

### The host's Chrome choice and download

Added by a later S27 PR.

### Bundling agent-browser

Added by a later S27 PR.

### PageVisited

Added by a later S27 PR.

## Deviations from the S27 brief

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
- A Unix daemon outlives a kill of the guard until the host kills it from its `.pid` file or the
  idle timeout ends it.

## Not verified

- Windows: compiled with `cargo clippy --target x86_64-pc-windows-gnu`, never run.
  `CREATE_NO_WINDOW`, the case-insensitive pass-through on a real environment block, and the
  daemon's end under the Job Object are unverified.
- The guard in front of the real agent-browser (the smoke test is a later S27 PR).
- reqwest's reading of the proxy variables is from its documented behaviour, not from a run.
- The URL host reader follows the WHATWG URL standard as written; it was not compared with
  Chrome's or the `url` crate's parser on the same inputs.
- The socket folder's owner and mode check ran on Linux only, not macOS.

## Status

proposed

## Date

2026-11-02
