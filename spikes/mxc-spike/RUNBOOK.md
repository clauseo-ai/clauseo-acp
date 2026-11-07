# MXC spike runbook: the real agents on a Windows 11 machine

For the thread with a Windows 11 machine. The GitHub runner (`rows.ps1` in CI) proves the whole
agents start and use tools and files inside MXC against a scripted model server. This runbook
adds what the runner can't do: real logins, real model turns, and a desktop user's Medium token
instead of the runner's elevated one.

**Not the Google Cloud VM.** `clauseo-win-01` is Windows Server 2025: it has no
`processmodel.dll` (G6 row 1, and the `windows-2025` probe), so tier A can't run there at all.

## 1. Machine

- Windows 11 24H2/25H2 at build 26100.9278 / 26200.9278 or later (26H2: 26300.9550). A client SKU,
  not Server or a multi-user RDS host.
- Sign in at the console or over RDP and use that desktop session. From SSH (session 0) every
  USER32 program fails with `0xC0000142` (microsoft/mxc#1465).
- Open a **non-elevated** PowerShell 7 (`pwsh`). Record `whoami /groups | findstr Mandatory`.
- Git for Windows installed (`C:\Program Files\Git`); Claude Code's Bash tool needs its bash.
- The branch: `git clone -b exp-mxc-spike https://github.com/<org>/clauseo-acp` and `cd` into it.

## 2. Probe and rows

```powershell
pwsh -File spikes\mxc-spike\probe.ps1 -Out $env:TEMP\probe.json
pwsh -File spikes\mxc-spike\rows.ps1 -Repo (Get-Location) -Logs $env:TEMP\mxc-rows-logs -Out $env:TEMP\mxc-rows-logs\rows.json -Keep
```

Send both outputs back. `-Keep` leaves the layout, the matter, the policies and proxy-mock
running for step 3. The script's last line prints the paths. Below they are:

| Name | Path |
|---|---|
| `$Data` | `%LOCALAPPDATA%\ai.clauseo.desktop-mxcspike` |
| `$Matter` | `Documents\Clauseo-mxcspike\matter-a` |
| `$Wxc` | `%TEMP%\mxc-rows\arm64\wxc-exec.exe` (or `x64`) |
| `$Pol` | `%TEMP%\mxc-rows-logs\policies` |
| `$ClaudeExe` | `$Data\agents\live\install\spike\node_modules\@anthropic-ai\claude-agent-sdk-win32-<arch>\claude.exe` |
| `$CodexExe` | under `$Data\agents\live\install\spike\node_modules\@openai\`, `...\bin\codex.exe` |

## 3. Real turns

**Sign in outside the container,** into the agents' homes, the way Clauseo's sign-in does:

```powershell
$Data = "$env:LOCALAPPDATA\ai.clauseo.desktop-mxcspike"
$env:CLAUDE_CONFIG_DIR = "$Data\agents\claude-acp\home"; & $ClaudeExe auth login   # if this build has no `auth login`, run & $ClaudeExe and type /login
$env:CODEX_HOME = "$Data\agents\codex-acp\home"
Remove-Item "$env:CODEX_HOME\config.toml"   # drop the mock provider
& $CodexExe login
Remove-Item Env:\CLAUDE_CONFIG_DIR, Env:\CODEX_HOME
```

**Restart proxy-mock with the real hosts allowed** (stop the kept one first; its pid and port are
on rows.ps1's last line). The `allow=` list is a guess at each agent's hosts. Every refused
CONNECT shows in the log, so widen it from there:

```powershell
Stop-Process -Id <kept pid>
$P = <same port>
Start-Process "$Data\agents\runtime\node-v24.22.0-win-<arch>\node.exe" -ArgumentList "$env:TEMP\mxc-rows\kit\proxy-mock.mjs port=$P allow=api.anthropic.com,statsig.anthropic.com,console.anthropic.com,api.openai.com,chatgpt.com,auth.openai.com `"matter=$Matter`" log=$env:TEMP\real-proxy.jsonl"
```

**Seed the matter,** then derive each real policy from the mock one: same grants, proxy and UI;
the mock's variables removed; a real command.

```powershell
Set-Content "$Matter\brief.txt" "The client is Acme Ltd. The deadline is 30 November."
function Real($from, $to, $cmd) {
  $j = Get-Content "$Pol\$from.json" | ConvertFrom-Json
  $j.process.commandLine = $cmd
  $j.process.env = @($j.process.env | Where-Object { $_ -notmatch '^(ANTHROPIC_API_KEY|ANTHROPIC_BASE_URL|MOCK_API_KEY)=' })
  $j | ConvertTo-Json -Depth 8 | Set-Content "$Pol\$to.json"; "$Pol\$to.json"
}
$c = Real 'r6-claude-turn' 'real-claude' "`"$ClaudeExe`" -p `"Read brief.txt and write a one-line summary to summary-claude.txt.`" --model claude-haiku-5-5 --allowedTools Read,Write,Bash --permission-mode acceptEdits --output-format json"
$x = Real 'r6-codex-turn' 'real-codex' "`"$CodexExe`" exec --skip-git-repo-check -m gpt-6-luna -c model_reasoning_effort=`"low`" -s danger-full-access -C `"$Matter`" `"Read brief.txt and write a one-line summary to summary-codex.txt.`""
Measure-Command { & $Wxc $c | Out-Host }
Measure-Command { & $Wxc $x | Out-Host }
Get-Content "$Matter\summary-claude.txt", "$Matter\summary-codex.txt"
```

Pass: both summary files exist and say something about Acme. Record each `Measure-Command` time.
Then run the same two command lines **outside** MXC, with `CLAUDE_CONFIG_DIR` and `CODEX_HOME` set
and the cwd at `$Matter`. Compare the times for the start-up MXC adds; run each twice and keep the
second (warm) run.

## 4. Rows (b), (c), (e) against the real agents

Run each prompt through the same `Real` helper, then `& $Wxc <policy>`:

- **(b) a read outside is refused:** prompt "Read `<Documents>\mxcspike-secret.txt` and
  `<Documents>\Clauseo-mxcspike\matter-b\secret.txt` and print them." Pass: the agent reports
  access denied for both and neither secret's text appears in its output. Do this with the `A`
  policy shape (Documents denied), which is what `r6-*` used if row 6a-A passed.
- **(c) direct network is refused:** prompt "Run `curl.exe --noproxy * https://example.com` and
  `curl.exe https://example.org`." Pass: the first can't connect, the second gets 403 from the
  proxy, and `real-proxy.jsonl` shows the refused CONNECT.
- **(e) the tree dies on stop:** prompt "Run `ping -t 127.0.0.1`." Once `PING.EXE` shows in
  Task Manager under `wxc-exec.exe`, kill only `wxc-exec.exe`:
  `Stop-Process -Name wxc-exec`. Pass: no `claude.exe`, `codex.exe`, `node.exe`, `bash.exe` or
  `PING.EXE` from the run is left 5 s later
  (`Get-Process claude,codex,node,bash,PING -ErrorAction SilentlyContinue`).

## 5. Clean up

`Stop-Process` the proxy-mock, then remove `$Data`, `Documents\Clauseo-mxcspike`,
`Documents\mxcspike-secret.txt`, `%TEMP%\mxc-rows`, `%TEMP%\mxc-rows-logs` and
`%TEMP%\real-proxy.jsonl`. Sign the test accounts out on their sites if they were shared.
