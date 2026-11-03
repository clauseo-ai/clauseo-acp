# 0039: The egress proxy

## Context

A jailed agent reaches the network only through a loopback proxy in `sandbox`, one per agent
launch; the browser uses the same proxy in every tier (S16, stage C). The proxy decides by host
name, never looks inside TLS, refuses private addresses, chains to a firm's proxy, and turns each
refusal into a `Denial` for the "Allow this site?" card (R18). S16's brief lists what was decided
for it; this record says how each was applied, which crates the proxy uses and why, and where the
code departs from the brief.

## Decision

- **One proxy per launch, two listeners** on `127.0.0.1` sharing one list and one sink
  (`start_proxy`, `ProxyHandle::port` and `browser_port`). Both bind through
  `sandbox::ports::bind_loopback`: port 0 on macOS and Linux; on Windows a free port in 61080 to
  61179 from a random offset, with `SO_EXCLUSIVEADDRUSE` set before each bind (`socket2` makes the
  socket; `setsockopt` from `windows`' `Win32_Networking_WinSock`). Stage E's connector relay
  binds through the same function.
- **Runtime.** One thread per proxy, `clauseo-proxy`, runs `async_io::block_on` over an
  `async_executor::LocalExecutor`. DNS and the upstream lookup run on `blocking`'s pool. When the
  handle drops, the thread stops and the executor drops every task, which closes both listeners
  and every tunnel; the drop joins the thread. No Tokio. A sink or `record` callback runs on
  that thread and must not block: one that blocks stalls the launch's every connection, and the
  handle's drop, which joins the thread, never returns.
- **Token.** 32 bytes from `getrandom`, as hex; the agent listener requires exactly one
  `Proxy-Authorization: Basic base64("clauseo:<hex>")`, decoded and compared in constant time
  (`subtle`) over the whole `user:token`; 407 carries `Proxy-Authenticate: Basic
  realm="clauseo"`. The token is redacted in every `Debug`, never logged, never sent upstream.
- **Checks, in order:** head (`httparse`; 10 s, 8 KB, 100 headers; 431, 408 or 400; CRLF line
  ends only, and `httparse` must read the whole head, so a bare LF gets 400 at once), token
  (agent listener only; the 407's log line names the host asked for), form and scheme (405,
  `Scheme`; 400 with no denial when there is no host to name, or for a `host:port` target on a
  method other than CONNECT), silent list (403, no denial), port (403, `Port`: 443 for CONNECT,
  80 for `http://`), a non-public IP literal or `localhost` name (403, `PrivateAddress`;
  amended 2026-11-03, below), allowlist (403, `NotAllowed`, `shared_hosting` from
  `proxy/shared_hosts.rs`), route (under the connect timeout; a lookup that takes longer goes
  direct), address check (DNS under the connect timeout; 504), then a tunnel slot (64 per launch;
  the next gets 503 and no denial) and the connect (10 s; 502, or 504 on the timeout).
- **Caps and timeouts.** Per listener: 64 client connections that have not finished their head
  (the next is closed unanswered), so connections to the token-free browser listener cannot lock
  out the agent's. Per launch: 192 client and origin sockets together (64 heads plus a client and
  an origin socket per tunnel). Across every proxy in the host process (a static counter): 1024
  sockets, or half the process's soft open-file limit when that is smaller, read at
  `start_proxy` with `rustix`'s `getrlimit` (Windows has no such limit and keeps 1024). A client past a socket cap is closed unanswered; an origin connection past one gets
  the client 503. Clients are counted in the accept loop and released when their task ends. A
  tunnel or forwarded request with no byte moved either way for 15 minutes is closed (agents keep
  long streaming responses open, so not shorter; it is watched in eighths, so it closes between 15
  and about 17 minutes after the last byte). After any error answer the proxy ends its write
  half and reads and discards what the client still sends, up to 64 KB or 1 s, before it closes:
  closing with unread bytes sends a reset, and on Windows the client then loses the answer. The
  client's socket slot stays held until then.
- **Names.** One helper reads list entries and requests alike: one trailing dot dropped, IDN to
  ASCII and case folded through `url`'s host parser, labels checked against DNS's shape, IPv4 in
  any form the URL standard reads. `*.domain` matches names under `domain`, not `domain` itself.
  An IP literal matches only an exact entry and still goes through the address check; a
  non-public literal or `localhost` is refused before the list (amended 2026-11-03, below).
- **Address check.** After DNS a host is refused if any of its addresses is not public (the list
  is in `proxy/address.rs`), and the proxy connects only to those addresses, in order, never
  resolving again. Through an upstream the name still goes to the upstream unresolved, but the
  proxy resolves it locally too (through the same network seam, under the connect timeout) and
  refuses it with `PrivateAddress` if any local answer is not public; an IP literal is checked
  as on the direct path. When local DNS fails or times out the name goes upstream unchecked, so
  a firm whose desktops resolve names only through the proxy keeps working. That is a known gap,
  and whoever controls the name's DNS can choose it: answer the desktop's query with a failure or
  too slowly, and the upstream's with a private or loopback address. So is split DNS (the
  upstream may resolve the name to another address than the desktop did, and may re-resolve on
  every request). Whether to refuse the fail-open branch when the upstream is on loopback (a local
  agent proxy such as Zscaler or px, which shares the desktop's resolver), or to make it a firm
  setting, is G10's call. The upstream's own address is the firm's
  configuration (possibly `127.0.0.1`) and is not checked. This is stricter than the brief,
  which checks only IP literals through an upstream; approved as built by the orchestrator on
  2026-11-02 (Rohan may override), with G10 deciding the loopback case.
- **Plain `http://`.** One request per client connection, rewritten to origin-form (absolute-form
  to an upstream), `Host` written from the URL, `Connection: close`; `Proxy-Authorization`,
  `Proxy-Authenticate`, `Proxy-Connection`, `Connection`, `Keep-Alive`, `TE`, `Trailer`,
  `Upgrade` and every header `Connection` names are dropped. The body is copied unchanged up to
  its end, by `Content-Length` or by reading the chunked framing as it passes; a request with
  both, with lengths that disagree, or with a coding other than `chunked` gets 400. A chunk-size
  line is hex digits, optional spaces or tabs, then CRLF or `;` and an extension with no control
  bytes; anything else gets 400 (when no response byte has gone back yet). The response is piped
  back raw; through an upstream its status is read first, and a 407 gives the client 502 and
  `Denial::UpstreamAuth`, as for CONNECT.
- **Upstream.** `Route::Http` gets `CONNECT host:443` with `Host` and no credentials: 200 opens the
  tunnel, 407 gives the client 502 and `Denial::UpstreamAuth`, anything else 502. Clauseo stores
  no proxy password. Discovery is `proxy/upstream.rs`: per scheme, `HTTPS_PROXY` for CONNECT and
  `HTTP_PROXY` for plain `http://` (each with its lower-case form and `NO_PROXY`), then the OS
  asked about an `https://` or `http://` URL to match (macOS's separate HTTP proxy setting and
  PAC scripts that branch on the scheme apply). OS answers are cached 5 minutes, at most 1024 of
  them, the oldest dropped first. A miss runs one OS lookup per scheme, host and port (other
  callers for the same key wait for its answer), and at most 4 at once per proxy; a miss past
  that goes direct for that request and is not cached. On Windows the whole WinHTTP lookup, WPAD
  included, is bounded to 10 s on its own thread, and at most 4 such threads live at once in the
  process (a lookup left running past its bound keeps its slot), so hung lookups cannot pile up
  threads; past that the lookup fails and goes direct. `upstream_for` (stage E's connector relay) returns `Result<Route, Error>` and refuses
  a firm URL that `start_proxy` refuses, so the relay fails visibly rather than going around the
  firm's proxy. `Upstream`'s `Debug` shows only a fixed URL's scheme, host and port.
- **allow and revoke** change a live exact-host list, read on every request. `revoke` removes the
  exact host whether it came from the start list or `allow`, and closes each open tunnel whose
  host the list no longer allows; a `*.domain` entry that still covers the host keeps it and its
  tunnels. The list and the tunnel table share one lock, so a tunnel opened during a `revoke` is
  either closed by it or refused.
- **Denials** go through the caller's sink wrapped in S14's `CappedSink`, ticked once a second.
  Refusals are logged with `tracing` by host and port only.
- **Connectivity.** No state until the first connect; a success is `Reachable`; three failures in
  a row (DNS, connect refused or timed out, to an origin or an upstream) are `Unreachable`;
  `DenialSink::upstream` is called on a change only. Refusals, 407s and bad heads do not count.
- **Starting allowlists** and `LoginKind` as the brief's table, each host marked inferred until
  G8b. `Allowlist::firm_suffix` refuses an empty, single-label or IP-literal domain, a Public
  Suffix List entry (private section included, `psl`), a domain with a PSL rule under it, and a
  domain on, under or above a shared-hosting entry. The `psl` crate cannot list its rules, so a
  rule under a domain is found by asking for the suffix of a probe name under it (which finds a
  `*.domain` rule) and from a checked-in list of parents (`PUBLIC_SUFFIX_PARENTS` in
  `proxy/shared_hosts.rs`: `core.windows.net`, `windows.net`, `cloudflarestorage.com`,
  `firebaseio.com`, `dropboxusercontent.com`, `run.app`, `on.aws`, `onmicrosoft.com`,
  `nip.io`, `sslip.io`). A named PSL rule under a domain on neither list is not caught; the list
  grows when one turns up.
- **`--example proxy`** prints `{"port","browser_port","token"}`, logs every request as a JSON
  line (`ProxyConfig::record`), denials on stderr, and runs until killed.

Crates, each in the root `[workspace.dependencies]` table: `async-executor` 1.14 (the thread's
executor), `blocking` 1.7 (DNS and the lookup off the I/O thread), `httparse` 1.10 (heads; small,
no allocation, fuzzed upstream), `getrandom` 0.3.4, `base64` 0.22.1 and `subtle` 2.6 (the token),
`psl` 2 (2.1.246, the only new package in `Cargo.lock`; MIT or Apache-2.0), `socket2` 0.6.5
(Windows only), `rustix` 1.1.5 with `process` (Unix only: the soft open-file limit through a
safe `getrlimit`; already in `Cargo.lock` through `async-io`).
`objc2-cf-network` 0.3.2 (macOS, CFNetwork proxy settings and PAC; features `std` and `CFProxySupport`).
`windows` gains the WinHTTP features `Win32_Foundation` and `Win32_Networking_WinHttp` (Windows only).
`async-io`, `futures`, `url`, `tracing` and `windows` were already workspace
dependencies. All but `psl` and its `psl-types` were already in `Cargo.lock`.

Departures from the brief:

- `proxy/mod.rs` is `src/proxy.rs` (the workspace denies `clippy::mod_module_files`); request
  heads are in `proxy/head.rs`, byte copying and body framing in `proxy/relay.rs`, the network
  seam in `proxy/net.rs`, the accept loops and checks in `proxy/server.rs`.
- The address check refuses a host if any resolved address is not public, not only the one
  connected to, and also refuses `198.18/15`, `240/4`, `fec0::/10`, Teredo `2001::/32` and local
  NAT64 `64:ff9b:1::/48`, and checks the IPv4 inside `::/96`, `::ffff:0:0:0/96`, `64:ff9b::/96`
  and `2002::/16`.
- `ProxyConfig` has a fifth field, `record: Option<Record>`, the per-request callback the example
  logs.
- `firm_suffix` also refuses a domain on or under the shipped shared-hosting list, since the
  Public Suffix List lacks some of them (`amazonaws.com` itself) and a wildcard there allows
  strangers' sites.
- Two `Error` variants: `ProxyConfig(String)` (a refused allowlist entry or upstream URL) and
  `ProxyStart(io::Error)`, both `agent.launch_failed`.
- Each proxy runs on its own thread, `clauseo-proxy`, not on the host's I/O threads as the
  brief says: the proxy lives in `sandbox`, which has no handle on the host's executor, and a
  thread per launch keeps one launch's proxy from stalling the host's other work.
- Socket caps and an idle timeout beyond the brief's 64 tunnels (see Caps and timeouts), so one
  launch cannot take the host's file descriptors.
- Through an upstream, names are also resolved locally for the address check (see Address
  check); the brief checks only IP literals there.
- The Windows port allocator's error, when every port in 61080 to 61179 fails, names the range and
  says another program or a Windows port reservation (Hyper-V, WSL or Docker; `netsh int ipv4
  show excludedportrange protocol=tcp`) may hold it.

## Consequences

- Each launch has its own token, ports and sink, so a refusal maps to one matter and launch, and
  another launch's agent cannot use this proxy even where the port rule is open
  (`Loopback::Any`).
- An intranet site on a private address is refused on the direct path, and through an upstream
  when the desktop's own DNS gives a private answer; through an upstream it passes when local DNS
  gives no answer, or when the upstream resolves the name differently (split DNS). Non-443 HTTPS
  endpoints are refused. G10 decides whether firms need exceptions.
- A tunnel where one side half-closes passes the end on and stays open the other way until that
  side closes, `revoke` closes it, 15 minutes pass with no byte, or the handle drops.
- **Requirement on R17:** the host raises its soft `RLIMIT_NOFILE` at start, before the first
  `start_proxy`. launchd starts a GUI app with a soft limit of 256 open files; the proxies then
  cap themselves at 128 sockets, under one launch's 192, so a second busy launch is refused. The
  proxy does not raise the limit, since `sandbox` does not own the host process.
- The proxy decides by host name and never looks inside TLS, so a CONNECT to an allowed host can
  carry a TLS session whose SNI or HTTP `Host` names another site served from the same address
  (domain fronting). Accepted with the no-interception decision.
- The allowlists are inferred until G8b measures them; changes need Rohan's approval.
- The "May depend on" cell of the `sandbox` row in `ARCHITECTURE.md` names the new crates.

Amended 2026-11-03: port claims. The browser needs loopback ports that agent-browser's daemon and
Chrome bind later, in the jail, so they cannot be listeners the host holds. `ports::claim_port`
returns a `PortClaim`: on macOS and Linux the port a bind of `127.0.0.1:0` got (closed before it
returns), on Windows a free port in 61080 to 61179 tested with the same `SO_EXCLUSIVEADDRUSE` bind.
Each claim sits in a process-wide registry until it drops, and `bind_loopback`, `claim_port` and
`claim_port_at` never hand out a registered port (on macOS and Linux a bind that lands on one is
asked again, the rejected listeners held open meanwhile; on Windows registered ports are skipped).
`ports::claim_port_at(port)` is for agent-browser's Windows daemon port, whose number is a hash of
its session name: it refuses a port outside 61080 to 61179 on Windows (`InvalidInput`), and gives
`AddrInUse` when the port is registered or a test bind fails, so the caller draws a new session
name; it checks the registry before its test bind, so it never binds a claimed port. Interim,
until agent-browser takes a port option upstream. The registry keeps this process's claims apart
only; another process can still take a claimed port before its owner binds it, and the launch
then fails with no fallback. On macOS and Linux a bind of port 0 learns its port only after the
kernel picked it: if the kernel hands back a claimed port (it hands out only ports nobody holds),
it is held for an instant under the registry lock and the bind asked again, and a launch binding
that exact port in that instant fails, but cannot take it over. On Windows the candidate loop skips
claimed ports before any bind.

Amended 2026-11-03: private-address refusals come before the allowlist. On both listeners and in
both forms (CONNECT and absolute-form `http://`), a host that is an IP literal in any form the
parser reads (dotted, hex, bracketed IPv6) whose address the address check calls non-public, or
`localhost` or a name under it (any case, a trailing dot dropped), is refused with 403 and
`Denial::Host { why: PrivateAddress }` right after the port check, before the allowlist, and is
never resolved. Before, an unlisted private literal was `NotAllowed`, whose card offers to allow
it, and a listed `localhost` was resolved and checked only by its answers. No allowlist entry can
let a private literal or `localhost` through now. A public literal off the list stays
`NotAllowed`, and a listed one still goes through the address check.

## Status

proposed

## Date

2026-11-02
