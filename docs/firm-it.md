# Clauseo for firm IT

How to deploy, lock down and support Clauseo on Windows, and on Macs under MDM. Each fact names its
source: a file in this repo, a decision record, a test run or a vendor page. When a fact changes, the
pull request that changes it changes this page too.

Written for Clauseo 0.1.x (2026-11). Nothing here is signed yet: see [section 2](#2-signing).

## 1. Which installer

Each release on GitHub ([decision 0035](decisions/0035-release-builds.md), "Artefacts by tier") has:

| File | For | Installs to |
|---|---|---|
| `Clauseo_<v>_x64-machine-setup.exe` | **Firms.** Per machine, needs an administrator, never updates itself | `C:\Program Files\Clauseo` |
| `Clauseo_<v>_x64-setup.exe` | People who install it themselves. Per user, no administrator, updates itself | `%LOCALAPPDATA%\Clauseo` |
| `Clauseo_<v>_arm64-machine-setup.exe`, `Clauseo_<v>_arm64-setup.exe` | Windows on ARM. Tier 2: a release may ship without them | as above |
| `Clauseo_<v>_aarch64.dmg`, `Clauseo_<v>_x64.dmg` | Macs (Apple silicon; Intel is tier 2) | `/Applications` |
| `Clauseo_<v>_amd64.deb`, `Clauseo-<v>-1.x86_64.rpm` | Linux (tier 2). Updates come through your package manager only | |
| `SHA256SUMS.txt` | The SHA-256 of every file above | |

Both Windows installers ship the same `Clauseo.exe` and use the stock NSIS pages, in English only
(decision 0035, "PR 2"). Seen on Windows Server 2025 (build 26100) on 2026-11-03, with the
0.0.0 dry-run installers:

- The per-machine installer raises a UAC prompt, installs `Clauseo.exe` and `uninstall.exe` in
  `C:\Program Files\Clauseo` and writes
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo` (64-bit view) with `DisplayName` and
  `Publisher` `Clauseo`, `DisplayVersion` `<v>`, `InstallLocation` `"C:\Program Files\Clauseo"` and
  `UninstallString` `"C:\Program Files\Clauseo\uninstall.exe"` (both in quotes), `DisplayIcon`,
  `MainBinaryName`, `NoModify` and `NoRepair` 1, and `EstimatedSize`. It also writes
  `HKLM\SOFTWARE\Clauseo\Clauseo`, whose default value is the install folder; uninstalling leaves
  that key. It makes two shortcuts for all users,
  `C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Clauseo.lnk` and
  `C:\Users\Public\Desktop\Clauseo.lnk`, also with `/S`. It writes nothing under `HKCU`. The app it starts at the end runs un-elevated (medium integrity).
- The per-user installer raises no UAC prompt, installs in `%LOCALAPPDATA%\Clauseo` and writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo`.
- Uninstalling asks one question, an unticked box: "Also delete conversations, saved versions and
  settings". Left unticked, each user's data folder (`%LOCALAPPDATA%\ai.clauseo.desktop`) stays.
  The silent `/S` uninstall asks nothing and always keeps it.

### Intune (Win32 app)

| Field | Value |
|---|---|
| Install command | `Clauseo_<v>_x64-machine-setup.exe /S` |
| Uninstall command | `"C:\Program Files\Clauseo\uninstall.exe" /S` |
| Install behaviour | System |
| Detection, registry | Key `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo`, value `DisplayVersion`, string comparison, equals `<v>`; "Associated with a 32-bit app on 64-bit clients": No |
| Detection, file (instead) | `C:\Program Files\Clauseo`, file `Clauseo.exe`, exists. If you compare the file's version instead, `Clauseo.exe`'s File version is `<v>`, read as `<v>.0` |

Write the uninstall path out in full rather than `%ProgramFiles%`: Intune's management extension
is a 32-bit process, where `%ProgramFiles%` means `C:\Program Files (x86)`. `/S` is NSIS's silent
switch. `uninstall.exe /S` hands over to a copy of itself in `%TEMP%` and returns before the files
are gone. On our test machine the stub returned after 0.46 s and the folder was gone 0.26 s later.
Windows PowerShell's `Start-Process -Wait` waits for that copy too. Either way it leaves each
user's `%LOCALAPPDATA%\ai.clauseo.desktop` in place.

Measured on Windows Server 2025 (build 26100), 2026-11-03, with the 0.0.0 dry-run installer, from an
elevated PowerShell: install exit 0 in about 2 s, with no window and no UAC prompt; uninstall exit
0; both detection forms find the install. Not yet run through Intune itself. If you try the install
line by hand on a copy you downloaded with a browser, run `Unblock-File` on it first. Otherwise
Windows shows "Open File - Security Warning" (Unknown Publisher) and waits, even from an elevated
prompt, until someone presses Run.

Never run Clauseo elevated: WebView2 can fail to start when its data folder belongs to another
user ([tauri#13926](https://github.com/tauri-apps/tauri/issues/13926)).

## 2. Signing

- **Windows: unsigned** until SignPath approves the project. Windows shows "Unknown Publisher" in
  the download warning and the UAC prompt (seen on the test machine above), SmartScreen may warn,
  and AppLocker or WDAC publisher rules cannot name Clauseo yet. Once signed, the publisher will be
  "SignPath Foundation".
- **macOS: signed ad hoc**, not yet with a Developer ID or notarised, until the Apple developer
  account exists (decision 0035, "Context"). So there is no Team ID yet for a privacy (PPPC)
  profile; when there is one, it changes if the Apple account moves to an organisation.
- **Check every download** against `SHA256SUMS.txt` from the same release:
  `sha256sum -c SHA256SUMS.txt --ignore-missing` on Linux or macOS (`shasum -a 256 -c` on older
  macOS), or `Get-FileHash .\Clauseo_<v>_x64-machine-setup.exe` in PowerShell and compare by eye.
  Every asset also has a GitHub build provenance attestation
  (`gh attestation verify <file> --repo clauseo-ai/clauseo-acp`; decision 0035, "Consequences").

## 3. AppLocker and WDAC

- Allow `%PROGRAMFILES%\Clauseo\*` by **path**. Only administrators can write there, which is why
  firms with these rules need the per-machine installer: they usually block programs in
  user-writable folders such as `%LOCALAPPDATA%`.
- Do not use **hash** rules: every release changes every hash. **Publisher** rules wait for
  signing (section 2).
- Coming (stage D): Clauseo will download Node and the agents into each user's data folder,
  `%LOCALAPPDATA%\ai.clauseo.desktop\`, which default AppLocker rules block. Allow that path, or
  use the IT-installed runtime under Program Files once its policy key lands (`install.bundle`,
  section 6, "Coming").
- Coming (stage C): on Windows, Clauseo's sandbox runs each agent as a separate local user
  (task S15). A policy that forbids local accounts leaves agents running without it, shown in the
  app as "Not protected".

## 4. WebView2

Clauseo draws its window with Microsoft Edge WebView2, Evergreen.

- **Install the Evergreen runtime per machine in your image.** If it is missing, the per-user
  installer fetches it silently, per user (seen on the test machine: about 50 s, no window). It
  installs a per-user Edge Update too (`%LOCALAPPDATA%\Microsoft\EdgeUpdate`, with two scheduled
  tasks `MicrosoftEdgeUpdateTaskUser<SID>Core` and `…UA`), which keeps that runtime updated. On
  non-persistent VDI a per-user runtime is gone at sign-out. Microsoft's guide:
  [WebView2 for enterprises](https://learn.microsoft.com/microsoft-edge/webview2/concepts/enterprise).
- **Minimum version:** 155.0.0.0 (`minimumWebview2Version` in
  `crates/desktop/tauri.conf.json`). Below it, or with no runtime, Clauseo shows a dialog
  ("Clauseo needs Microsoft Edge WebView2 to open", "Download WebView2") instead of a window, and
  exits (decision 0035, "PR 2").
- **No Fixed Version** runtime is shipped or needed.
- **Keep the runtime updating.** Its updates are separate from Edge's: the Edge Update policies
  "Update (WebView)" and `UpdatesSuppressed` control them. A frozen Edge does not freeze WebView2,
  unless you freeze both. Please do not.
- **Group policies firms touch**, under `HKLM\SOFTWARE\Policies\Microsoft\Edge\WebView2\`, keyed by
  the exe name **`Clauseo.exe`**
  ([WebView2 policies](https://learn.microsoft.com/deployedge/microsoft-edge-webview-policies)):
  `BrowserExecutableFolder` (points one app at a Fixed Version folder), `ReleaseChannels` and
  `ChannelSearchKind` (leave Clauseo on Stable), `DowngradeVersion` (roll the runtime back for one
  app), `HttpAllowlist` and the compatibility toggles. Edge's browser policies do not apply to
  WebView2.
- **AV, EDR and DLP:** scope exclusions to the runtime's binaries and the user data folder,
  `%LOCALAPPDATA%\ai.clauseo.desktop\` (its `EBWebView` folder is WebView2's). Never put that
  folder on a network share, and keep DLP from flagging its cookie and cache writes, or expect a
  blank window or lost state (Microsoft,
  [Prevent security tools from blocking WebView2 apps](https://learn.microsoft.com/microsoft-edge/webview2/concepts/security-tools)).
- **Data stays local:** everything Clauseo keeps per user is in `%LOCALAPPDATA%\ai.clauseo.desktop`
  (`crates/settings/src/locations.rs`, `Locations::resolve_for`), never in the roaming profile.
  New matters go in `Documents\Clauseo` unless `matters.default_location` says otherwise.

## 5. Citrix and RDS

- **Exclude WebView2 from Citrix API hooks:** add `msedgewebview2.exe` to the `ExcludedImageNames`
  value under each of these keys:
  - `HKLM\SOFTWARE\Citrix\CtxHook`
  - `HKLM\SOFTWARE\Citrix\CtxHook64`
  - `HKLM\SOFTWARE\WOW6432Node\Citrix\CtxHook`
  - `HKLM\SOFTWARE\WOW6432Node\Citrix\CtxHook64`

  Hooking breaks WebView2's start-up or makes it spin
  ([WebView2Feedback #1082](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1082)). We
  have not yet tested this on a Citrix VDA.
- **Profiles:** keep `%LOCALAPPDATA%\ai.clauseo.desktop` in the user's persistent profile
  (an FSLogix profile container keeps it). It holds the conversations and saved versions.
- **One instance per user session:** a second launch in the same session brings back the first
  window (`tauri-plugin-single-instance`, in `crates/desktop/src/lib.rs`). Its lock is per
  session, so two users on one server should each get their own; not yet tested on RDS.
- **No GPU:** WebView2 falls back to software rendering. If the app feels slow there, set
  `ui.lite` to `on` (section 6), which drops its heavier effects.

## 6. Clauseo's policy keys

**Where policy goes, strongest first** (a key the first source sets is not taken from a later one;
[decision 0009](decisions/0009-policy-layer.md); `system_policy_sources` in
[`crates/settings/src/policy.rs`](../crates/settings/src/policy.rs)):

- **Windows:** registry values under `HKLM\SOFTWARE\Policies\Clauseo\Clauseo` (64-bit view), one
  value per key, named by the dotted key (`ui.theme`); then `C:\ProgramData\Clauseo\policy.json`.
- **macOS:** managed preferences in the domain `ai.clauseo.desktop`, forced values only (an MDM
  profile; a user's `defaults write` is not policy); then
  `/Library/Application Support/Clauseo/policy.json`.
- **Linux:** `/etc/clauseo/policy.json`.

Nothing in `HKCU` or a user-writable file counts as policy. Policy is read once at start-up: a
change applies on the next start.

**What a policy value does.** It replaces the user's value and shows as locked in the app: Settings
shows the policy's value with its choices greyed out and the line "Managed by your organisation"
(measured on Windows Server 2025, 2026-11-03, with `ui.theme` = `dark`). A value
of the wrong type for a known key is a warning and **still locks the key, at its default**. A
source that cannot be read at all (a `policy.json` that does not parse, a registry read that
fails) is skipped and **locks each security key off**: `mcp.servers` empty, `tools.web_search`,
`tools.browser`, `privacy.counts` and `privacy.crash_reports` false, `install.bundle` and
`install.registry` unset, and no `plans.<id>` at all. The two `privacy` keys can
only be turned off by policy: a policy `true` for one is ignored with a warning, and the lawyer
keeps their own switch. A key this version does not know is only a warning, so one profile can
serve mixed versions. (`Leaf::parse`, `refuse` and `SECURITY_KEYS` in
[`crates/settings/src/leaf.rs`](../crates/settings/src/leaf.rs); `lock_security_keys` in
[`crates/settings/src/effective.rs`](../crates/settings/src/effective.rs).)

**Formats.**

- **Registry:** `REG_DWORD` for a number, or a boolean as 0 or 1; `REG_SZ` for a string, or JSON
  text for an object (`agents.<id>`, `plans.<id>`, `mcp.servers`); `REG_MULTI_SZ` for a list of strings (no key in
  the table below takes a list yet). Value names ignore case; an agent id keeps
  its case ([`crates/settings/src/policy/registry.rs`](../crates/settings/src/policy/registry.rs)).
- **`policy.json`:** the shape of the user's `settings.json`: sections `matters`, `agents`, `mcp`,
  `ui`, `tools`, `privacy`, `plans` and `install`, each an object; comments and trailing commas are allowed. Sample:
  [`docs/firm-it/policy.sample.json`](firm-it/policy.sample.json) (Windows paths).
- **macOS profile:** a payload whose `PayloadType` is `ai.clauseo.desktop`, with the dotted keys as
  its keys, except agents and plans: one key `agents`, a dictionary from agent id to launch spec, and
  one key `plans`, a dictionary from agent id to plan. Sample:
  [`docs/firm-it/ai.clauseo.desktop.mobileconfig`](firm-it/ai.clauseo.desktop.mobileconfig)
  (read by [`crates/settings/src/policy/managed_prefs.rs`](../crates/settings/src/policy/managed_prefs.rs)).

Both samples set **every** key, to show the format; each key you leave in is locked for every user.
Delete the ones you do not mean to lock. `crates/settings/tests/firm_it.rs` loads both through
Clauseo's readers with no warning, and fails when this table misses a key.

| Key | Type | Default | Registry type | Example |
|---|---|---|---|---|
| `matters.default_location` | absolute path | none: `Documents\Clauseo` | `REG_SZ` | `D:\Matters` (one path for every user; no `%VAR%` expansion) |
| `agents.<id>` | object: `command` (absolute path), `args` (list), `env` (map) | none | `REG_SZ`, JSON | value `agents.claude` = `{"command": "C:\\Program Files\\Firm Tools\\claude-agent-acp.exe", "args": []}`. Replaces the user's whole entry for that agent; never put a secret in `env` |
| `mcp.servers` | object: server name (1 to 64 of `A-Z a-z 0-9 _ -`) to `command`, `args`, `env`, `enabled` (default false) | empty | `REG_SZ`, JSON | `{"firm-docs": {"command": "C:\\Program Files\\Firm Tools\\firm-docs-mcp.exe", "enabled": true}}`. **Security key.** Replaces the user's whole list; the name `browser` is reserved |
| `ui.theme` | `system`, `light`, `dark` | `system` | `REG_SZ` | `dark` |
| `ui.font_size` | whole number, 14 to 20 (points) | 16 | `REG_DWORD` | 18 |
| `ui.density` | `comfortable`, `compact` | `comfortable` | `REG_SZ` | `compact` |
| `ui.language` | `en`, `hi` | `en` | `REG_SZ` | `en` |
| `ui.lite` | `automatic`, `on`, `off` | `automatic` | `REG_SZ` | `on` (RDS hosts without a GPU) |
| `ui.motion` | `system`, `reduce`, `full` | `system` | `REG_SZ` | `reduce` |
| `tools.web_search` | boolean | true | `REG_DWORD` 0 or 1 | 0: agents may not search the web or read web pages. **Security key** |
| `tools.browser` | boolean | true on macOS; false on Windows and Linux | `REG_DWORD` 0 or 1 | 0: agents do not get Clauseo's browser. **Security key** |
| `privacy.counts` | boolean | true | `REG_DWORD` 0 or 1 | 0: Clauseo keeps and sends no anonymous usage counts, and deletes what it kept. Policy can only turn it off (1 is ignored with a warning). **Security key** |
| `privacy.crash_reports` | boolean | false | `REG_DWORD` 0 or 1 | 0: Clauseo sends no crash reports. Policy can only turn it off (1 is ignored with a warning). **Security key** |
| `plans.<id>` | object: `endpoint` (`https` URL with a host; no user name, password, query or fragment), `provider` (`anthropic`, `bedrock`, `vertex`), `env` (map) | none: the plan login | `REG_SZ`, JSON | value `plans.claude-acp` = `{"endpoint": "https://ai-gateway.firm.example/anthropic", "provider": "anthropic"}`. **Security key**: replaces the user's whole plan for that agent; an unreadable source leaves no plan at all. An `env` name holding `KEY`, `TOKEN`, `SECRET` or `PASSWORD` (any case) is dropped with a warning: keep secrets in your own tools (an AWS profile, Google's application default credentials) |
| `install.bundle` | absolute path | none: download | `REG_SZ` | `D:\Clauseo\bundle`: an offline bundle folder. **Security key**: unset if a source is unreadable |
| `install.registry` | `https` URL with a host; no user name, password, query or fragment | none: the public hosts | `REG_SZ` | `https://npm.firm.example/`: a firm npm mirror. **Security key**: unset if a source is unreadable |

Sources: the key list and kinds are `KEYS` in `crates/settings/src/leaf.rs`; the values and
defaults are in [`crates/settings/src/schema.rs`](../crates/settings/src/schema.rs).

**Example: lock the theme to dark on one Windows machine** (elevated PowerShell):

```powershell
New-Item -Path 'HKLM:\SOFTWARE\Policies\Clauseo\Clauseo' -Force | Out-Null
New-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Clauseo\Clauseo' -Name 'ui.theme' `
  -PropertyType String -Value 'dark' -Force | Out-Null
```

In Intune, set the same value with a custom OMA-URI or a remediation script; ADMX templates come
later.

### Coming

Planned keys, not in any build yet. Each moves into the table above in the pull request that adds
it.

| Key | Task | Note |
|---|---|---|
| `update.mode` (`auto`, `notify`, `none`) | R23 | `none` for managed fleets; locks at `none` if a policy source is unreadable. Per-machine installs never update themselves anyway |
| `sandbox.required`, `sandbox.allow_unprotected_auto`, `network.upstream_proxy`, `network.ca_certs`, `network.allowed_domains` | R17 | Security keys |

## 7. Hosts Clauseo reaches

For firewalls and proxies. Clauseo's own window talks to no host: its content security policy
allows only the app's own content and its bridge (`app.security.csp` in
`crates/desktop/tauri.conf.json`).

| Host | Why | When |
|---|---|---|
| `github.com`, then `release-assets.githubusercontent.com` | Downloads and, for per-user installs, updates (`latest.json` on GitHub Releases, decision 0035) | A release asset URL on `github.com` answers 302 to `release-assets.githubusercontent.com` (`curl -sI`, 2026-11-03). The updater is not on yet: `plugins.updater.endpoints` is empty in `crates/desktop/tauri.conf.json` (task R23). Per-machine installs never update themselves |
| `go.microsoft.com` (`/fwlink/p/?LinkId=2124703`), then `msedge.sf.dl.delivery.mp.microsoft.com` | WebView2 bootstrapper | Only when the runtime is missing at install, or when a user presses "Download WebView2". The fwlink answers 301 to that host (`curl -sI`, 2026-11-03). The bootstrapper then fetches the runtime through Edge Update from `config.edge.skype.com`, `msedge.api.cdp.microsoft.com` and `msedge.f.tlu.dl.delivery.mp.microsoft.com` (DNS-Client log, measured on Windows Server 2025, 2026-11-03; the same three per user and per machine). A runtime in your image avoids them all |
| Edge Update's hosts (`config.edge.skype.com`, `msedge.api.cdp.microsoft.com`, `msedge.f.tlu.dl.delivery.mp.microsoft.com` seen for installs) | WebView2 runtime updates | Microsoft's, as for Edge |
| `api.anthropic.com`, `claude.ai`, `platform.claude.com` | Claude Code, signed in with a Claude plan | While the agent works. An API key needs only `api.anthropic.com` |
| `api.openai.com`, `chatgpt.com`, `auth.openai.com` | Codex, signed in with ChatGPT | While the agent works. An API key needs only `api.openai.com` |
| Sites the agent searches and opens | Web search and Clauseo's browser | Only while `tools.web_search` or `tools.browser` is on |

Opening a downloaded installer also makes Windows check it with SmartScreen
(`nav.smartscreen.microsoft.com`, `checkappexec.microsoft.com`,
`data-edge.smartscreen.microsoft.com`). Those are Windows' hosts, not Clauseo's.

The agents' hosts are Clauseo's starting list for its egress proxy (`builtin_allowlist` in
`crates/sandbox/src/proxy/allowlist.rs`), inferred and not yet measured. The proxy chains to
your firm's proxy: it reads `HTTPS_PROXY` and `HTTP_PROXY`, then the OS's proxy settings, PAC
included, and stores no proxy password ([decision 0039](decisions/0039-egress-proxy.md),
"Upstream").

Coming:

- `t.clauseo.chat`: usage counts and crash reports (tasks S21 to S23), only when they are on, and
  firms can force them off (`privacy.*`). HTTPS POSTs with `User-Agent: Clauseo`; they follow
  `HTTPS_PROXY`, then the OS's proxy settings, but not a PAC file yet, and never Clauseo's egress
  proxy ([decision 0036](decisions/0036-counts.md), "The desktop").
- `nodejs.org` and `registry.npmjs.org`: Node and the agents' own downloads (task S17), unless
  `install.bundle` or `install.registry` points at your copy.
- `data.clauseo.chat`: Clauseo's optional paid data connectors, only for a firm that adds one.

## Later

ADMX and ADML templates, a firm bundle zip, an MSI and a `.pkg` for Jamf come when a firm asks. Claude
Code's and Codex's own managed settings are the vendors' (their business plans), not Clauseo's.
