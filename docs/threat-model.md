# Threat model

A stub. It says what Clauseo protects, where the trust boundaries are, and what stops an attacker
at each one. Each control has a status: `built` means code and a check exist on `main`; `designed`
means the design is decided and the code is not written yet. The record of the Rust-to-webview
boundary is [0001](decisions/0001-rust-webview-boundary.md).

## Assets

- **The lawyer's documents.** Every file in every matter folder, and the threads about them.
- **Agent credentials.** The sign-in tokens Claude Code and Codex keep for the lawyer. Clauseo
  never reads them.
- **Clauseo's version store.** The checkpoints of each matter. Undo and restore depend on it, so
  an agent must not be able to rewrite it.

## Trust boundaries

1. **Webview to host.** The React UI runs in the system webview and may render text an attacker
   wrote (agent output, document contents). Everything it sends to Rust is untrusted.
2. **Agent to host.** The agent and its adapter run as a separate process and speak ACP over
   stdio. A prompt injection in a document can steer the agent, so its messages are untrusted.
3. **Agent to the machine.** The sandbox stands between the agent process and the lawyer's other
   files.
4. **Agent to the network.** The egress proxy is the agent's only route out.

## Webview

**What an attacker controls.** Script in the page, through an XSS in rendered agent output or in a
converted document. From there they can call any IPC command the page can reach.

**What stops them.**

- `built`: The release context names one capability, `main`, which grants only the thirteen bridge
  commands, one permission each ([0012](decisions/0012-desktop-bridge.md),
  [0019](decisions/0019-app-stream-to-the-window.md), [0026](decisions/0026-open-and-reveal.md)), and the two core
  window commands of Tauri's drag script, granted in every build: start a drag, and maximise or
  restore on a double-click ([0015](decisions/0015-window-frame.md)). The frame adds
  `frame.json` at setup on Windows and Linux (`window_frame::grant_caption_commands`): minimise,
  maximise or restore, close, and whether the window is maximised. The `e2e` and `marks`
  builds add `e2e.json` and `marks.json` at setup (`e2e::grant_scoreboard`), and their permissions
  exist only in those builds; the `capabilities` check reads all four files and fails on
  `remote`, `core:default`, `fs:`, `shell:`, `http:`, `opener:` or `dialog:`
- `built`: Tauri's window commands take a `label`, so those granted reach every window of the
  app; `main` is the only one, and a test fails when a second window is built
  ([0015](decisions/0015-window-frame.md))
- `built`: A pinned CSP: no remote images or connections, no `unsafe-eval`, `object-src 'none'`,
  `base-uri 'none'`, Trusted Types enforced with one allowed policy, `clauseo-sanitize` (defence in
  depth: engines without support ignore it); the `tauri-config` check pins the string
- `built`: An `AppManifest` in `build.rs`, so app commands go through the access check; it lists
  the thirteen commands in release (the `e2e` and `marks` builds add theirs), and a test checks it
  against the code and the capabilities
- `built`: No global Tauri object and a frozen prototype (`tauri-config` check); no asset protocol
  (`tauri-config` check, and `deny.toml` bans tauri's `protocol-asset` feature); no devtools in
  release builds (`deny.toml` bans the feature; Tauri turns devtools on in debug builds)
- `built`: Rust validates every command (types, opaque ids, allowed values); folders and files are
  named by opaque ids the bridge or the host issued, and folders to show by their matter, never
  paths
- `built`: `dispatch` and `query` are allow-lists that name every host command and read, with no
  catch-all, so one the host adds is refused by the build until the bridge admits it
  ([0019](decisions/0019-app-stream-to-the-window.md))
- `built`: A minimal command surface: dispatch, query, subscribe (a stream, or the app-wide events,
  which take nothing from the page), acknowledge, answer a permission, pick a folder
  ([0012](decisions/0012-desktop-bridge.md)), open a link or a matter's document, and show a
  matter's folder or the app data folder ([0026](decisions/0026-open-and-reveal.md))
- `built` for "always allow", the folder picker and external links: escalations only through
  native dialogs drawn by Rust, never a webview confirm; `designed` for the rest
- `built`: On Windows, when a known folder its own folder dialog shows sits on a share that does
  not answer (the dialog would block about 21 s), `pick_folder_or_choose` opens an in-app chooser
  instead (`crates/desktop/src/chooser.rs`). The page moves only by indices into listings the
  desktop made, or to a typed path: a drive path (no `..`, `\\?\`, `\\.\`, relative path, alternate
  stream or device name, and through no link, hidden or system folder), or a share only on a server
  this machine already uses (a mapped drive's or a known folder's; never one with `@` or `%`, a
  loopback or an administrative `c$` share), so script cannot make the app connect to a server of its choosing.
  Choosing ends in a native confirm with the full path and Cancel focused, then the bridge's
  unchanged `pick_folder` issues the ref: script never gets a folder admitted without the lawyer.
  Known gap: on such a machine, script in the page can, without the lawyer seeing it, open up to 20
  sessions a run (at most 3 a minute; past that the pick draws the native dialog), each of up to
  300 listings of 2,000 folder names, and so read local folder names under the drives and home,
  the user's name under `C:\Users` among them (no files, hidden, system or link folders), and
  list shares on servers the machine already uses. The CSP keeps fetch, XHR, WebSocket, images
  and forms on the app's own origins, and `window.open` is denied (wry's default with no
  new-window handler), but an XSS can still carry those names off the machine by navigating the
  window to an outside address (no navigation guard: neither window builder sets `on_navigation`,
  `crates/desktop/src/lib.rs` `open_main_window`) and likely through a `dns-prefetch` link, which
  no CSP directive covers
- `built`: Links go through one `open_external`: https or http to a public host (no credentials,
  no localhost, loopback, private or link-local address, no overlong name), or mailto to plain
  `local@domain` addresses with at most a subject and a body, after a native confirm that names the
  host in ASCII (`xn--` for an international name) or the addresses. Cancel is the default button and
  only the open button accepts (Escape and close decline), one confirm shows at a time and none for a second after a decline; declined is
  `transport.unauthorized` ([0026](decisions/0026-open-and-reveal.md))
- `built`: `open_file` opens only a file the host issued a ref for, inside the folder of the matter
  that owns it (checked by the host and again by the desktop on the resolved path), a regular file
  (with no execute bit, on Linux) of a listed document type, at most one a second; on macOS in the type's
  default app, named, so a file swapped for a link opens as a document, never as a program. `reveal`
  shows a folder selected in its parent and never launches it. Known gap: a process that can write
  the matter folder (the agent, until the sandbox) can swap the file or a folder on its path for a
  link between the check and the open, and so open a document of a listed type outside the matter;
  and script in the page can open the matter's documents without a click
  ([0026](decisions/0026-open-and-reveal.md))
- `built`: The webview never navigates away from the app: both windows refuse any top-level
  navigation but the app's own origin, `about:blank` and `about:srcdoc` (and `devUrl` in a
  development build only), so page script cannot send data out with `location.href` or a
  `meta refresh` (`crates/desktop/src/navigation.rs`); every link click is also cancelled in the
  renderer
- `built`: On Windows the webview resolves no host name: both windows start WebView2 with
  `--host-resolver-rules="MAP * ~NOTFOUND"` (`crates/desktop/src/navigation/browser_args.rs`), so neither a
  refused navigation's host nor a `<link rel=dns-prefetch>` or `preconnect` target reaches a DNS
  query; on a Windows VM no `msedgewebview2` process made any lookup for any of the four. The
  app's pages need no name: they come from `https://tauri.localhost` through interception, and
  every network request is made by Rust. A set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, which
  WebView2 takes in place of the app's arguments, gets the rule added at its end as
  `main` starts (Chromium takes the last of a repeated switch, so it wins over the variable's own
  rules). Residual: an `AdditionalBrowserArguments` policy value for the app's exe under `HKCU`
  or `HKLM` `Software\Policies\Microsoft\Edge\WebView2` is documented as replacing the app's
  arguments, which would drop the rule (WebView2 157 on the Windows VM merged them instead, and
  the rule held, 2026-11-07); only something already running with the lawyer's rights, or the firm's admin, can
  set one, and the app logs a warning naming the hive (never the value) when one is set. Known
  gap: on macOS (WKWebView) a refused navigation's host and
  `dns-prefetch`/`preconnect` targets can still put a name of the page's choosing into a DNS
  query (and, for preconnect, a TLS handshake), with no WKWebView API to turn that off, to be
  closed before the Mac release
- `designed`: Markdown renders without raw HTML; DOMPurify only on the few HTML-string paths

## Agents

**What an attacker controls.** The agent's behaviour, through a prompt injection in a document,
a web page or a tool result. The agent can read and write whatever the sandbox allows, and send
any ACP message, including permission requests with misleading text.

**What stops them.**

- `designed`: The sandbox wraps the whole agent process (adapter and agent), not only its shell
  commands
- `designed`: Auto mode is on by default only where the sandbox is active; in tier C the adapters'
  ask modes apply and the matter shows Not protected
- `designed`: A human approves only escalations: paths outside the matter, new web domains, sandbox
  off, full-access modes, "always allow", all through native dialogs drawn by Rust or a denial card that
  opens one
- `built`: Permission answers are checked against the options offered, in the bridge and again in
  the host; `designed`: options are labelled by kind, not by the agent's text (the permission prompt)
- `designed`: ACP message sizes are capped; Clauseo advertises no file or terminal access to the
  adapter
- `designed`: The host checkpoints the matter before and after every turn, so any step can be undone
- `designed`: Adapters are installed pinned by version and integrity hash

## Sandbox

**What an attacker controls.** Code running as the agent process: shell commands, file tools and
anything the agent starts.

**What stops them.**

- `designed`: An OS jail per tier: tier A is Seatbelt (macOS) or MXC (Windows); tier B is a
  sandbox user on Windows; tier C has no sandbox, and Linux stays at tier C (bubblewrap is Later)
- `built` (macOS): Seatbelt through `/usr/bin/sandbox-exec`, with a profile generated from the
  policy in which every path is a parameter, never profile text
  ([0038](decisions/0038-sandbox.md)); `designed`: the host runs every agent through it (R17)
- `built` (macOS): The agent reads and writes only the matter folder, its state folder and the
  matter's notes folder; it reads and runs its own runtime, reads listed system locations (nothing
  under `/Users`, `/private/var/folders`, `/private/tmp` or `/Volumes`), and reaches the network
  only through the proxy's loopback port; other matters, their state and notes, the data dir and
  the version store are denied after every allow
- `built` (macOS): Its credential file is linked into the state folder and only the link is
  allowed; for Claude only the store's own files and lock folders are allowed, and the folder
  cannot be listed; `designed`: the link itself and sign-in outside the sandbox (R17). Limit: the
  link is a hard link, which an agent's temp-file-and-rename save breaks (the lawyer's copy keeps a
  stale token); stage D's split rule handles it, and G7 row 17 records how Claude saves its store
- `built` (macOS): The matter, state and notes folders themselves cannot be removed or replaced,
  and `build()` refuses one that is a symbolic link, so an agent cannot point its next launch at
  another folder; paths allowed again after the hidden paths (state, notes, credential link,
  runtime, read) cannot equal or hold the home folder, the matter, a hidden path or `/`
- `built` (macOS): Any `.git` already in the matter, the folder a `gitdir:` file names, files over
  the size cap, and `.codex/`, `.claude/`, `.agents/`, `.clauseo/`, `.mcp.json`, `CLAUDE.md`,
  `CLAUDE.local.md` and `AGENTS.md` at the matter root (existing or not, in any letter case) are
  read-only, and their folders cannot be renamed; `CLAUDE.md` cannot be written in place at any
  depth, but a folder moved into the matter that already holds one (made in the state folder, notes
  or `TMPDIR`) is not stopped, and a nested `CLAUDE.local.md` is writable
- `built` (macOS): A symbolic link in the matter that leads outside it is denied, and so is where it
  leads; so is every name in the matter of a file that also has a name outside it, `.git`
  included (the walk counts names per inode), and a writable name of a file whose other name is
  read-only. Past 256 link rules the nearest folders are denied instead, and a matter that still
  does not fit is refused at launch. A file whose every name is in the matter stays readable
- `built` (macOS): No Unix socket can be made or reached except in the browser socket folder the
  host makes for one launch (`/tmp/clauseo-ab-<id>`: one atomic `mkdir` at 0700, checked with
  `lstat`, removed at the end, stale ones swept at host start); nothing else in `/tmp` is
  writable, the folder itself cannot be removed, renamed or replaced from the jail, and only
  regular files and sockets can be made in it; on some macOS versions a swap (`renamex_np`,
  `RENAME_SWAP`) with a matter path can bring a folder in, with whatever it holds, and the
  removal at drop never follows a link. Its lock file, held for the launch, keeps another host's
  sweep away. Gap: the rules match paths, not owners, so a jailed process that outlived
  its session could meet a folder another local user made again at that path; the host drops
  the folder only after the launch's process tree has exited, and names are random
- `built` (macOS): A refused file read or write reaches a denial sink through the unified log
  watcher; `designed`: the denial event and card, and widening access restarts the agent only if
  the lawyer accepts (R18)
- `built` (macOS, Linux, tier C): Codex's own sandbox denies its shell the data folder's
  Clauseo-only paths, every other session's run folder and every other matter's notes, later ones
  included ([0030](decisions/0030-launch-profile.md)). Gaps: `run.old-*` when the data folder's
  path holds a glob character; none of it on Windows, where Codex's unelevated sandbox
  can't enforce a read deny (until S15)

## Egress proxy

**What an attacker controls.** Network requests from inside the sandbox, aimed at sending the
lawyer's documents somewhere.

**What stops them.**

- `designed`: The sandbox allows no network except Clauseo's local proxy; raw sockets are refused
- `built`: One proxy per agent launch on one or two loopback listeners. It carries CONNECT to
  port 443 and plain `http://` to port 80 of a host on the launch's allowlist, and refuses every
  other form, scheme and port; `designed`: the allowlists measured per agent and login kind (G8b)
- `built`: The agent listener takes only its launch's token (32 random bytes, compared in
  constant time, never logged or sent upstream); the browser listener takes no token, is bound
  only for a launch with the browser (never on tier B), and is held to its matter by the jail's
  port rule
- `built`: Loopback ports for the browser are claimed per session (`ports::claim_port`, and
  `claim_port_at` for agent-browser's Windows daemon port) in a process-wide registry, so no two
  launches of this host get the same port. On macOS and Linux a port is retired once its claim
  or listener ends, never handed to a later launch of the same host process, since a jailed
  child that called `setsid` outlives the tree kill with its jail's rule for that port. Gaps: the
  retired set is in memory, so after a host restart a survivor of the last run can meet a later
  launch on its old port; and another process, such as another user's host, can bind a claimed
  port before the browser does (it binds at its first tool call), and the agent's jail then
  reaches that listener. S15's listener rule closes the second on Windows; on macOS both are
  residual risks
- `built`: Before the allowlist it refuses an IP literal that is not public and `localhost` or
  any name under it, on both listeners, so no allowlist entry lets one through
- `built`: After DNS it refuses a host if any address is loopback, private, link-local, shared,
  unspecified or multicast (IPv4 inside IPv6 included), and connects only to the addresses it
  checked. Through an upstream proxy the name goes on unresolved, and is refused if the
  desktop's own DNS gives any non-public answer. Gap: when local DNS gives no answer (a desktop
  that resolves only through the proxy) the name goes on unchecked, and whoever controls the
  name's DNS can choose that branch (fail or stall the desktop's query, answer the upstream's
  with a private address); the upstream may also resolve it differently (split DNS, or rebinding
  on each request). Refusing that branch when the upstream is on loopback is G10's call
- `built`: It decides by host name and never intercepts TLS: it holds no CA. Gap: a CONNECT to
  an allowed host can carry TLS whose SNI or `Host` names another site on the same address
  (domain fronting)
- `built`: A firm upstream proxy is chained with no credentials (a 407 from it is reported, for
  CONNECT and plain `http://` alike): the firm's setting, then the host's `HTTPS_PROXY` or
  `HTTP_PROXY`, then the OS proxy settings and PAC (CFNetwork on macOS, WinHTTP with WPAD on
  Windows)
- `built`: One launch cannot take the host's file descriptors: per listener, 64 connections
  still sending their head; per launch, 192 sockets; across every launch, 1024 sockets or half
  the soft open-file limit; at most 4 OS proxy lookups at once; tunnels idle for 15 minutes are
  closed; `designed`: the host raising its open-file limit at start (R17)
- `built`: A blocked domain becomes a denial (rate-capped per launch); `designed`: the denial
  event and an "Allow this site?" card that opens a native dialog
