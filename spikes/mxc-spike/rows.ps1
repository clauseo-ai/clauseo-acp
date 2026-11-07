# MXC rows (G6 part two, step 2) on windows-11-arm: does tier A, MXC's process container, hold
# the whole Claude Code and Codex agents? Throwaway; branch exp-mxc-spike only.
#
# Builds Clauseo's installed layout under %LOCALAPPDATA%\ai.clauseo.desktop-mxcspike (Node 24.22.0
# ARM64, an `npm ci` of crates/limits/tests/live, the agents' homes, a state folder) and a matter
# under Documents, starts proxy-mock.mjs (loopback CONNECT proxy plus scripted model server) and
# runs every row through wxc-exec.exe (MXC v1.0.1) with UI on, proxy-only egress, the install
# read-only, the matter and state read-write. Two filesystem shapes:
#   strict  no enumeratePaths, nothing denied
#   A       enumeratePaths on each drive root (the #1109/#1676 workaround), Documents denied
#           (the matter, a read-write child of it, stays writable)
# Each row prints `ROW <id> PASS|FAIL|INFO <name> :: <evidence>`; logs and policies go to -Logs.
#
# Usage: pwsh -File rows.ps1 -Repo <checkout root> [-Logs <dir>] [-Out rows.json]
param(
    [Parameter(Mandatory)] [string]$Repo,
    [string]$Logs = '',
    [string]$Out = '',
    # Leave the layout, the matter and the proxy-mock running, for RUNBOOK.md's real turns.
    [switch]$Keep
)
$ErrorActionPreference = 'Continue'
$base = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$Work = Join-Path $base 'mxc-rows'
if (-not $Logs) { $Logs = Join-Path $Work 'logs' }
New-Item -ItemType Directory -Force $Work, $Logs, (Join-Path $Logs 'policies') | Out-Null
$Rows = [System.Collections.Generic.List[object]]::new()
$Facts = [ordered]@{}

function Section($t) { ''; "=== $t" }
function Row($id, $status, $name, $evidence) {
    $e = ($evidence -replace '\s+', ' ').Trim()
    "ROW $id $status $name :: $e"
    $Rows.Add([ordered]@{ id = $id; status = $status; name = $name; evidence = $e })
}
function Median([double[]]$xs) { if (-not $xs) { return $null }; $s = $xs | Sort-Object; $s[[int][math]::Floor(($s.Count - 1) / 2)] }

function Invoke-Timed([string]$exe, [string]$arguments, [int]$timeoutSec = 180, [hashtable]$envVars = @{}, [string]$cwd = '') {
    $psi = [System.Diagnostics.ProcessStartInfo]::new($exe, $arguments)
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    if ($cwd) { $psi.WorkingDirectory = $cwd }
    foreach ($k in $envVars.Keys) { $psi.Environment[$k] = $envVars[$k] }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try { $p = [System.Diagnostics.Process]::Start($psi) } catch {
        return [pscustomobject]@{ Exit = $null; TimedOut = $false; Ms = 0; Out = ''; Err = "start failed: $_"; Result = $null }
    }
    $p.StandardInput.Close()
    $o = $p.StandardOutput.ReadToEndAsync()
    $e = $p.StandardError.ReadToEndAsync()
    $done = $p.WaitForExit($timeoutSec * 1000)
    if (-not $done) { try { $p.Kill($true) } catch {}; $p.WaitForExit(10000) | Out-Null }
    $sw.Stop()
    $out = $o.Result
    $res = $null
    $line = ($out -split "`r?`n" | Where-Object { $_ -like 'RESULT *' } | Select-Object -Last 1)
    if ($line) { try { $res = $line.Substring(7) | ConvertFrom-Json } catch {} }
    [pscustomobject]@{ Exit = if ($done) { $p.ExitCode } else { $null }; TimedOut = -not $done; Ms = [int]$sw.Elapsed.TotalMilliseconds; Out = $out; Err = $e.Result; Result = $res }
}
function Code($r) { if ($r.TimedOut) { 'TIMEOUT' } elseif ($null -eq $r.Exit) { 'NOSTART' } else { '{0}/0x{1:X8}' -f $r.Exit, $r.Exit } }
function Tail([string]$s, [int]$n = 3) { (($s -split "`r?`n" | Where-Object { $_ -and $_ -notmatch '^\s*$' } | Select-Object -Last $n) -join ' | ') }

# ---------------------------------------------------------------- set-up
Section 'Set-up'
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$Facts.os = "$($cv.ProductName) $($cv.DisplayVersion) $($cv.CurrentBuildNumber).$($cv.UBR) $arch $($cv.InstallationType)"
$Facts.session = (Get-Process -Id $PID).SessionId
$Facts.integrity = ((whoami /groups | Select-String 'Mandatory Level' | Select-Object -First 1).ToString().Trim() -split '\s{2,}')[0]
"OS: $($Facts.os); session $($Facts.session); $($Facts.integrity)"
$a = if ($arch -eq 'Arm64') { 'arm64' } else { 'x64' }

# MXC v1.0.1 (same zip and hash as probe.ps1 and G6).
$zip = Join-Path $Work 'mxc-release-binaries.zip'
$want = '0E832239454BA7FCD338FF56218A287EEF4C0A01C1484084B869F55FEFF8ABE9'
if (-not (Test-Path $zip) -or (Get-FileHash $zip -Algorithm SHA256).Hash -ne $want) {
    Invoke-WebRequest -Uri 'https://github.com/microsoft/mxc/releases/download/v1.0.1/mxc-release-binaries.zip' -OutFile $zip -UseBasicParsing
}
if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $want) { 'MXC zip hash mismatch: stopping'; exit 1 }
$Wxc = Join-Path $Work "$a\wxc-exec.exe"
if (-not (Test-Path $Wxc)) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $z = [IO.Compression.ZipFile]::OpenRead($zip)
    foreach ($entry in $z.Entries) {
        if ($entry.FullName.StartsWith("$a/") -and $entry.Name -and $entry.FullName -notmatch '/symbols/') {
            $dest = Join-Path $Work $entry.FullName
            New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dest, $true)
        }
    }
    $z.Dispose()
}
"wxc-exec: $Wxc"

# Clauseo's layout (crates/agents/src/node.rs:274-289, receipt.rs:26-35, settings/src/locations.rs).
$Data = Join-Path $env:LOCALAPPDATA 'ai.clauseo.desktop-mxcspike'
$Runtime = Join-Path $Data "agents\runtime\node-v24.22.0-win-$a"
$Install = Join-Path $Data 'agents\live\install\spike'
$ClaudeHome = Join-Path $Data 'agents\claude-acp\home'
$CodexHome = Join-Path $Data 'agents\codex-acp\home'
$State = Join-Path $Data 'agent-state\matter-a'
$Tmp = Join-Path $State 'tmp'
$Docs = [Environment]::GetFolderPath('MyDocuments')
if (-not $Docs) { $Docs = Join-Path $env:USERPROFILE 'Documents' }
$DocsCreated = -not (Test-Path $Docs)
$Matters = Join-Path $Docs 'Clauseo-mxcspike'
$Matter = Join-Path $Matters 'matter-a'
$Sibling = Join-Path $Matters 'matter-b'
$DocSecret = Join-Path $Docs 'mxcspike-secret.txt'
$Kit = Join-Path $Work 'kit'
$Git = Join-Path $env:ProgramFiles 'Git'
New-Item -ItemType Directory -Force $Install, $ClaudeHome, $CodexHome, $State, $Tmp, $Matter, $Sibling, $Kit | Out-Null
Set-Content -Path (Join-Path $Matter 'real.txt') -Value 'matter file'
Set-Content -Path (Join-Path $Sibling 'secret.txt') -Value 'other matter secret'
Set-Content -Path $DocSecret -Value 'documents secret'
Copy-Item (Join-Path $PSScriptRoot 'inside.js'), (Join-Path $PSScriptRoot 'proxy-mock.mjs'), (Join-Path $PSScriptRoot 'acp-driver.mjs') $Kit -Force
"data: $Data"
"matter: $Matter"

# Node 24.22.0, the pinned archive and hash (crates/agents/src/snapshot/embedded.rs).
$nodeZipName = "node-v24.22.0-win-$a.zip"
$nodeSha = @{ arm64 = '3A5C209A5BC59E65B3BC0FA13ED5E166A26BD3D55332847F498B76B037408D61'; x64 = 'F9683A19877E12A9B167305C8DB71040D812876359DE21F0D2E97C3F321F80C3' }[$a]
$Node = Join-Path $Runtime 'node.exe'
if (-not (Test-Path $Node)) {
    $nz = Join-Path $Work $nodeZipName
    Invoke-WebRequest -Uri "https://nodejs.org/dist/v24.22.0/$nodeZipName" -OutFile $nz -UseBasicParsing
    if ((Get-FileHash $nz -Algorithm SHA256).Hash -ne $nodeSha) { 'Node zip hash mismatch: stopping'; exit 1 }
    Expand-Archive -Path $nz -DestinationPath (Split-Path $Runtime) -Force
}
"node: $Node $((& $Node --version))"

# The adapters at Clauseo's pins: an npm ci of crates/limits/tests/live (package.json and lock).
Copy-Item (Join-Path $Repo 'crates\limits\tests\live\package.json'), (Join-Path $Repo 'crates\limits\tests\live\package-lock.json') $Install -Force
if (-not (Test-Path (Join-Path $Install 'node_modules'))) {
    $npmCli = Join-Path $Runtime 'node_modules\npm\bin\npm-cli.js'
    $r = Invoke-Timed $Node "`"$npmCli`" ci --ignore-scripts --no-audit --no-fund --cache `"$(Join-Path $Work 'npm-cache')`"" 900 @{} $Install
    "npm ci: $(Code $r) in $($r.Ms) ms; $(Tail ($r.Out + $r.Err) 2)"
}
$ClaudeExe = Join-Path $Install "node_modules\@anthropic-ai\claude-agent-sdk-win32-$a\claude.exe"
$CodexExe = (Get-ChildItem (Join-Path $Install 'node_modules\@openai') -Recurse -Filter codex.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\bin\\codex\.exe$' } | Select-Object -First 1).FullName
$ClaudeAcp = Join-Path $Install 'node_modules\@agentclientprotocol\claude-agent-acp\dist\index.js'
$CodexAcp = Join-Path $Install 'node_modules\@agentclientprotocol\codex-acp\dist\index.js'
"claude.exe: $ClaudeExe ($(Test-Path $ClaudeExe))"
"codex.exe: $CodexExe"
$Bash = Join-Path $Git 'bin\bash.exe'
"git bash: $Bash ($(Test-Path $Bash))"

# The proxy and mock, on a free loopback port.
$l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $l.Start(); $P = $l.LocalEndpoint.Port; $l.Stop()
$MockLog = Join-Path $Logs 'proxy-mock.jsonl'
$proxy = Start-Process -FilePath $Node -PassThru `
    -ArgumentList "`"$Kit\proxy-mock.mjs`" port=$P allow=example.com `"matter=$Matter`" `"log=$MockLog`"" `
    -RedirectStandardOutput (Join-Path $Logs 'proxy-mock.out.txt') -RedirectStandardError (Join-Path $Logs 'proxy-mock.err.txt')
for ($i = 0; $i -lt 50 -and -not ((Test-Path $MockLog) -and (Select-String -Path $MockLog -Pattern 'listening' -Quiet)); $i++) { Start-Sleep -Milliseconds 200 }
"proxy+mock on 127.0.0.1:$P (pid $($proxy.Id))"
$Token = 'spiketoken-not-a-secret'

@"
model = "gpt-mock"
model_provider = "mock"
approval_policy = "never"
sandbox_mode = "danger-full-access"

[model_providers.mock]
name = "mock"
base_url = "http://127.0.0.1:$P/v1"
wire_api = "responses"
env_key = "MOCK_API_KEY"
"@ | Set-Content -Encoding utf8 (Join-Path $CodexHome 'config.toml')

# ---------------------------------------------------------------- policies
$roots = @($Matter, $State, $Runtime, $Install, $Kit, $Git) | ForEach-Object { [IO.Path]::GetPathRoot($_) } | Sort-Object -Unique
$Cfg = @{
    strict = @{ name = 'strict'; enum = @(); deny = @() }
    A      = @{ name = 'A'; enum = @($roots); deny = @($Docs) }
    B      = @{ name = 'B'; enum = @($roots); deny = @() }
}
$Rw = @($Matter, $State, $ClaudeHome, $CodexHome)
$Ro = @($Runtime, $Install, $Kit) + $(if (Test-Path $Git) { @($Git) } else { @() })

function Base-Env {
    $keep = 'SystemRoot', 'windir', 'SystemDrive', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE', 'HOMEDRIVE', 'HOMEPATH', 'USERNAME', 'USERDOMAIN', 'COMPUTERNAME', 'PROCESSOR_ARCHITECTURE', 'NUMBER_OF_PROCESSORS', 'ProgramFiles', 'ProgramData', 'ComSpec', 'PATHEXT'
    $m = [ordered]@{}
    foreach ($k in $keep) { $v = [Environment]::GetEnvironmentVariable($k); if ($v) { $m[$k] = $v } }
    $m.PATH = "$Runtime;$Git\cmd;$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
    $m.TEMP = $Tmp; $m.TMP = $Tmp
    # Clauseo's proxy variables, token in the userinfo; MXC is expected to replace them.
    $m.HTTPS_PROXY = "http://clauseo:$Token@127.0.0.1:$P"
    $m.HTTP_PROXY = "http://clauseo:$Token@127.0.0.1:$P"
    $m.CLAUSEO_TOKEN_PROBE = 'set-by-host'
    $m
}

function New-Policy([string]$name, [string]$cmdline, $cfg, [hashtable]$envExtra = @{}, [string]$cwd = $Matter) {
    $envMap = Base-Env
    foreach ($k in $envExtra.Keys) { $envMap[$k] = $envExtra[$k] }
    $pol = [ordered]@{
        version = '1.0.0'
        containment = 'processcontainer'
        process = [ordered]@{ commandLine = $cmdline; cwd = $cwd; env = @($envMap.Keys | ForEach-Object { "$_=$($envMap[$_])" }); timeout = 600000 }
        filesystem = [ordered]@{ readwritePaths = @($Rw); readonlyPaths = @($Ro); deniedPaths = @($cfg.deny) }
        network = [ordered]@{ egress = @{ default = 'deny' }; ingress = [ordered]@{ default = 'allow'; hostLoopback = 'allow' } }
        runtimeConfig = @{ networkProxy = "http://127.0.0.1:$P" }
        ui = [ordered]@{ disable = $false; clipboard = 'none'; injection = $false }
    }
    if ($cfg.enum.Count -gt 0) { $pol.processContainer = @{ filesystem = @{ enumeratePaths = @($cfg.enum) } } }
    $pf = Join-Path $Logs "policies\$name.json"
    $pol | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 $pf
    $pf
}

function Contained([string]$name, [string]$cmdline, $cfg, [hashtable]$envExtra = @{}, [int]$timeout = 180, [string]$cwd = $Matter) {
    $pf = New-Policy $name $cmdline $cfg $envExtra $cwd
    $r = Invoke-Timed $Wxc "`"$pf`"" $timeout
    Set-Content -Path (Join-Path $Logs "$name.out.txt") -Value $r.Out
    Set-Content -Path (Join-Path $Logs "$name.err.txt") -Value $r.Err
    $r
}

function JsE([string]$code) { "`"$Node`" -e `"$code`"" }

# ---------------------------------------------------------------- row 0: Node starts
Section 'Row 0: Node starts inside'
foreach ($c in 'strict', 'A') {
    $r = Contained "r0-node-$c" (JsE "console.log('NODE_OK '+process.version+' '+process.arch)") $Cfg[$c]
    Row "0-$c" $(if ($r.Out -match 'NODE_OK') { 'PASS' } else { 'FAIL' }) "node -e starts ($c)" "$(Code $r) $($r.Ms) ms; $(Tail $r.Out 1); $(Tail $r.Err 2)"
}

# ---------------------------------------------------------------- row 1: realpath (#1464)
Section 'Row 1: realpath (microsoft/mxc#1464)'
$rp = "const fs=require('fs');const t=process.env.MXC_T;const o={};try{o.native=fs.realpathSync.native(t)}catch(e){o.native='ERR '+e.code}try{o.js=fs.realpathSync(t)}catch(e){o.js='ERR '+e.code}fs.promises.realpath(t).then(v=>{o.promises=v},e=>{o.promises='ERR '+e.code}).finally(()=>console.log('RESULT '+JSON.stringify(o)))"
foreach ($c in 'strict', 'A') {
    $r = Contained "r1-realpath-$c" (JsE $rp) $Cfg[$c] @{ MXC_T = (Join-Path $Matter 'real.txt') }
    $o = $r.Result
    $ok = $o -and ($o.native -notlike 'ERR*') -and ($o.promises -notlike 'ERR*')
    Row "1-$c" $(if ($ok) { 'PASS' } else { 'FAIL' }) "realpathSync.native and promises.realpath on a matter file ($c)" "$(Code $r); native=$($o.native); promises=$($o.promises); js=$($o.js); $(if (-not $o) { Tail $r.Err 2 })"
}

# ---------------------------------------------------------------- row 2: Git Bash (#1061)
Section 'Row 2: Git Bash (microsoft/mxc#1061)'
if (-not (Test-Path $Bash)) {
    Row '2' 'INFO' 'Git Bash' "not installed at $Bash"
} else {
    $ctl = Invoke-Timed $Bash "-c `"echo ok > gitbash-outside.txt`"" 60 @{} $Matter
    "outside control: $(Code $ctl), file: $((Get-Content (Join-Path $Matter 'gitbash-outside.txt') -ErrorAction SilentlyContinue))"
    foreach ($c in 'strict', 'A') {
        $f = Join-Path $Matter "gitbash-$c.txt"
        $r = Contained "r2-gitbash-$c" "`"$Bash`" -c `"echo ok > gitbash-$c.txt`"" $Cfg[$c]
        $content = (Get-Content $f -ErrorAction SilentlyContinue) -join ''
        Row "2-$c" $(if ($content -eq 'ok') { 'PASS' } else { 'FAIL' }) "bash.exe -c 'echo ok > m/x' ($c)" "$(Code $r) $($r.Ms) ms; file=$(if ($content) { $content } else { 'missing' }); $(Tail ($r.Out + "`n" + $r.Err) 3)"
    }
}

# ---------------------------------------------------------------- row 3: delete (#1676)
Section 'Row 3: delete in the matter (microsoft/mxc#1676)'
foreach ($c in 'strict', 'A') {
    $f1 = Join-Path $Matter "del-cmd-$c.txt"; $f2 = Join-Path $Matter "unlink-$c.txt"
    Set-Content $f1 'x'; Set-Content $f2 'x'
    $r1 = Contained "r3-del-$c" "`"$env:ComSpec`" /d /c del /q `"$f1`"" $Cfg[$c]
    $r2 = Contained "r3-unlink-$c" (JsE "require('fs').unlinkSync(process.env.MXC_T);console.log('UNLINK_OK')") $Cfg[$c] @{ MXC_T = $f2 }
    $g1 = -not (Test-Path $f1); $g2 = -not (Test-Path $f2)
    Row "3-$c" $(if ($g1 -and $g2) { 'PASS' } else { 'FAIL' }) "cmd del and fs.unlinkSync in the matter ($c)" "del: $(Code $r1), file gone=$g1 $(Tail ($r1.Out + "`n" + $r1.Err) 1); unlinkSync: $(Code $r2), file gone=$g2 $(Tail $r2.Err 1)"
}

# ---------------------------------------------------------------- row 4: network (#1666)
Section 'Row 4: network in proxy mode (microsoft/mxc#1666)'
$r = Contained 'r4-net-A' "`"$Node`" `"$Kit\inside.js`" net example.com example.org" $Cfg.A @{} 120
$o = $r.Result
if (-not $o) {
    Row '4' 'FAIL' 'network probe' "inside.js did not report: $(Code $r); $(Tail $r.Err 3)"
} else {
    $leak = ($o.dns_lookup -like 'resolved*') -or ($o.dns_resolve4_direct -like 'resolved*')
    Row '4a' $(if ($leak) { 'FAIL' } else { 'PASS' }) 'DNS refused inside (fails if #1666 reproduces)' "dns.lookup(example.com): $($o.dns_lookup); random name: $($o.dns_lookup_random); c-ares UDP 53: $($o.dns_resolve4_direct)"
    Row '4b' $(if ($o.direct_tcp -ne 'CONNECTED') { 'PASS' } else { 'FAIL' }) 'direct TCP 1.1.1.1:443 refused' "$($o.direct_tcp)"
    $httpsOk = ($o.proxy_https -match '^HTTP/1\.[01] [23]\d\d') -and ($o.proxy_refused -match ' 403 ')
    Row '4c' $(if ($httpsOk) { 'PASS' } else { 'FAIL' }) 'HTTPS via the proxy to an allowed host; others refused' "example.com: $($o.proxy_connect) then $($o.proxy_https); example.org: $($o.proxy_refused)"
    $connects = @(Get-Content $MockLog | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.kind -eq 'connect' })
    $auth = ($connects | ForEach-Object { "$($_.host):$($_.verdict):auth-$($_.proxy_auth)" }) -join ', '
    $survives = "$($o.env.HTTPS_PROXY)" -match [regex]::Escape($Token)
    Row '4d' 'INFO' 'Clauseo proxy token survives MXC' "survives=$survives; HTTPS_PROXY inside=$($o.env.HTTPS_PROXY); HTTP_PROXY=$($o.env.HTTP_PROXY); https_proxy=$($o.env.https_proxy); NO_PROXY=$($o.env.NO_PROXY); CLAUSEO_TOKEN_PROBE=$($o.env.CLAUSEO_TOKEN_PROBE); proxy saw: $auth"
}
$curl = Join-Path $env:SystemRoot 'System32\curl.exe'
if (Test-Path $curl) {
    $r1 = Contained 'r4-curl-proxy' "`"$curl`" -sS -o NUL -w `"%{http_code}`" https://example.com" $Cfg.A @{} 60
    $r2 = Contained 'r4-curl-direct' "`"$curl`" -sS -o NUL -w `"%{http_code}`" --noproxy * --max-time 8 https://example.com" $Cfg.A @{} 60
    Row '4e' $(if (($r1.Out -match '200') -and ($r2.Out -notmatch '200')) { 'PASS' } else { 'FAIL' }) 'curl.exe (Schannel) via env proxy works, --noproxy refused' "via proxy: $(Code $r1) http=$(Tail $r1.Out 1) $(Tail $r1.Err 1); direct: $(Code $r2) http=$(Tail $r2.Out 1) $(Tail $r2.Err 1)"
}

# ---------------------------------------------------------------- row 5: isolation, tree, start-up
Section 'Row 5: isolation, tree kill, start-up'
$targets = @($DocSecret, (Join-Path $Sibling 'secret.txt'), $Sibling, $Docs, $env:USERPROFILE, (Join-Path $Matter 'real.txt'))
$r = Contained 'r5-read-A' "`"$Node`" `"$Kit\inside.js`" read $(($targets | ForEach-Object { "`"$_`"" }) -join ' ')" $Cfg.A
$o = $r.Result
if ($o) {
    $v = @{}; foreach ($t in $targets) { $v[$t] = $o.$t }
    $ok = ($v[$DocSecret] -like 'refused*') -and ($v[(Join-Path $Sibling 'secret.txt')] -like 'refused*') -and ($v[(Join-Path $Matter 'real.txt')] -like 'READ*')
    Row '5a-A' $(if ($ok) { 'PASS' } else { 'FAIL' }) 'Documents file and sibling matter refused, own matter read (A)' "Documents\mxcspike-secret.txt: $($v[$DocSecret]); matter-b\secret.txt: $($v[(Join-Path $Sibling 'secret.txt')]); matter-a\real.txt: $($v[(Join-Path $Matter 'real.txt')])"
    Row '5b-A' 'INFO' 'folder listings under A (enumeratePaths on the drive root is recursive, per #1676)' "matter-b: $($v[$Sibling]); Documents: $($v[$Docs]); USERPROFILE: $($v[$env:USERPROFILE])"
} else {
    Row '5a-A' 'FAIL' 'reads outside refused (A)' "inside.js did not report: $(Code $r); $(Tail $r.Err 3)"
}
$typeCmd = "`"$env:ComSpec`" /d /c type `"$DocSecret`" & type `"$(Join-Path $Sibling 'secret.txt')`" & dir /b `"$Sibling`""
$r = Contained 'r5-read-strict' $typeCmd $Cfg.strict
$leaked = ($r.Out -match 'documents secret') -or ($r.Out -match 'other matter secret')
Row '5a-strict' $(if (-not $leaked) { 'PASS' } else { 'FAIL' }) 'Documents file and sibling matter refused (strict, cmd type)' "$(Code $r); stdout=$(Tail $r.Out 3); stderr=$(Tail $r.Err 3)"

# Tree: cmd -> node -> PING -t inside; the host then kills wxc-exec only, as Clauseo's host would on stop.
$pf = New-Policy 'r5-tree' "`"$env:ComSpec`" /d /s /c `"`"$Node`" `"$Kit\inside.js`" tree`"" $Cfg.A
$psi = [System.Diagnostics.ProcessStartInfo]::new($Wxc, "`"$pf`"")
$psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true; $psi.RedirectStandardInput = $true
$tp = [System.Diagnostics.Process]::Start($psi)
$tp.StandardInput.Close()
$null = $tp.StandardError.ReadToEndAsync()
$ready = $null; $deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline -and -not $ready) {
    $t = $tp.StandardOutput.ReadLineAsync()
    $left = [int][math]::Max(1, ($deadline - (Get-Date)).TotalMilliseconds)
    if (-not $t.Wait($left)) { break }
    if ($null -eq $t.Result) { break }
    if ($t.Result -match 'TREE_READY') { $ready = $t.Result }
}
Start-Sleep -Milliseconds 800
$procs = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CreationDate
$tree = @(); $frontier = @($tp.Id)
while ($frontier.Count) {
    $kids = @($procs | Where-Object { $frontier -contains $_.ParentProcessId -and $_.ProcessId -ne $tp.Id -and ($tree.ProcessId -notcontains $_.ProcessId) })
    $tree += $kids; $frontier = @($kids.ProcessId)
}
$k0 = [Diagnostics.Stopwatch]::StartNew()
try { $tp.Kill($false) } catch {}
$tp.WaitForExit(10000) | Out-Null
Start-Sleep -Seconds 5
$alive = @($tree | Where-Object { $q = Get-CimInstance Win32_Process -Filter "ProcessId=$($_.ProcessId)" -ErrorAction SilentlyContinue; $q -and $q.CreationDate -eq $_.CreationDate })
$names = ($tree | ForEach-Object { "$($_.Name):$($_.ProcessId)" }) -join ' '
$need = @('cmd.exe', 'node.exe', 'PING.EXE') | Where-Object { ($tree.Name | ForEach-Object { $_.ToUpper() }) -notcontains $_.ToUpper() }
Row '5c' $(if ($ready -and -not $need -and -not $alive) { 'PASS' } else { 'FAIL' }) 'tree cmd -> node -> ping killed when wxc-exec is killed' "ready=$([bool]$ready) ($ready); tree: $names; missing: $($need -join ','); alive 5 s after kill: $(($alive | ForEach-Object { "$($_.Name):$($_.ProcessId)" }) -join ' ')"
foreach ($x in $alive) { Stop-Process -Id $x.ProcessId -Force -ErrorAction SilentlyContinue }

# Start-up added: the same command outside and inside (A), warm, medians.
$m = (JsE "process.stdout.write('m')")
$outside = 1..5 | ForEach-Object { (Invoke-Timed $Node "-e `"process.stdout.write('m')`"" 60).Ms }
$pfT = New-Policy 'r5-timing' $m $Cfg.A
$inside = 1..5 | ForEach-Object { (Invoke-Timed $Wxc "`"$pfT`"" 120).Ms }
$Facts.node_ms_outside = @($outside); $Facts.node_ms_inside = @($inside)
Row '5d' 'INFO' 'start-up added: node -e outside vs inside, wall ms to exit' "outside median $(Median $outside) ms ($($outside -join ',')); inside median $(Median $inside) ms ($($inside -join ',')); added ~$((Median $inside) - (Median $outside)) ms"

# ---------------------------------------------------------------- row 6: whole agents
Section 'Row 6: whole agents inside'
$agentEnv = @{
    CLAUDE_CONFIG_DIR = $ClaudeHome
    ANTHROPIC_API_KEY = 'sk-ant-mock-not-a-key'
    ANTHROPIC_BASE_URL = "http://127.0.0.1:$P"
    DISABLE_AUTOUPDATER = '1'
    DISABLE_TELEMETRY = '1'
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1'
    CLAUDE_CODE_GIT_BASH_PATH = $Bash
    CODEX_HOME = $CodexHome
    MOCK_API_KEY = 'mock-not-a-key'
}
$agentCfg = $Cfg.A
foreach ($c in 'A', 'B') {
    $r1 = Contained "r6-claude-version-$c" "`"$ClaudeExe`" --version" $Cfg[$c] $agentEnv 120
    $r2 = Contained "r6-codex-version-$c" "`"$CodexExe`" --version" $Cfg[$c] $agentEnv 120
    $ok = ($r1.Exit -eq 0) -and ($r2.Exit -eq 0)
    Row "6a-$c" $(if ($ok) { 'PASS' } else { 'FAIL' }) "claude --version and codex --version inside ($c)" "claude: $(Code $r1) $($r1.Ms) ms $(Tail $r1.Out 1) $(if ($r1.Exit -ne 0) { Tail $r1.Err 2 }); codex: $(Code $r2) $($r2.Ms) ms $(Tail $r2.Out 1) $(if ($r2.Exit -ne 0) { Tail $r2.Err 2 })"
    if ($ok) { $agentCfg = $Cfg[$c]; break }
}
$o1 = Invoke-Timed $ClaudeExe '--version' 60
$o2 = Invoke-Timed $CodexExe '--version' 60
Row '6a-outside' 'INFO' 'the same --version outside' "claude: $(Code $o1) $($o1.Ms) ms $(Tail $o1.Out 1); codex: $(Code $o2) $($o2.Ms) ms $(Tail $o2.Out 1)"
"agents run under config $($agentCfg.name)"

function Clear-Outputs { foreach ($f in 'claude-bash.txt', 'claude-write.txt', 'codex-shell.txt') { Remove-Item (Join-Path $Matter $f) -Force -ErrorAction SilentlyContinue } }
function Mock-Since([int]$skip) { @(Get-Content $MockLog | Select-Object -Skip $skip | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.kind -eq 'mock' }) }

Clear-Outputs
$skip = (Get-Content $MockLog).Count
$r = Contained 'r6-claude-turn' "`"$ClaudeExe`" -p `"Run the spike test.`" --model claude-haiku-5-5 --allowedTools Bash,Write --permission-mode acceptEdits --output-format json" $agentCfg $agentEnv 300
$bashFile = (Get-Content (Join-Path $Matter 'claude-bash.txt') -ErrorAction SilentlyContinue) -join ''
$writeFile = (Get-Content (Join-Path $Matter 'claude-write.txt') -ErrorAction SilentlyContinue) -join ''
$mk = Mock-Since $skip
$results = ($mk | ForEach-Object { $_.results } | Where-Object { $_ } | ForEach-Object { "$($_.id) error=$($_.is_error): $($_.text)" } | Select-Object -Unique) -join ' || '
Row '6b-bash' $(if ($bashFile -match 'hi') { 'PASS' } else { 'FAIL' }) 'claude -p turn: Bash tool writes m/claude-bash.txt' "$(Code $r) $($r.Ms) ms; file=$(if ($bashFile) { $bashFile } else { 'missing' }); model calls=$($mk.Count); tool results: $results"
Row '6b-write' $(if ($writeFile -match 'Write tool') { 'PASS' } else { 'FAIL' }) 'claude -p turn: Write tool writes m/claude-write.txt' "file=$(if ($writeFile) { $writeFile } else { 'missing' }); stdout: $(Tail $r.Out 1); stderr: $(Tail $r.Err 2)"

$skip = (Get-Content $MockLog).Count
$r = Contained 'r6-codex-turn' "`"$CodexExe`" exec --skip-git-repo-check -C `"$Matter`" `"Run the spike test.`"" $agentCfg $agentEnv 300
$cf = (Get-Content (Join-Path $Matter 'codex-shell.txt') -ErrorAction SilentlyContinue) -join ''
$mk = Mock-Since $skip
Row '6c' $(if ($cf -match 'hi') { 'PASS' } else { 'FAIL' }) 'codex exec turn: shell call writes m/codex-shell.txt' "$(Code $r) $($r.Ms) ms; file=$(if ($cf) { $cf } else { 'missing' }); model calls=$($mk.Count) tools offered=$(($mk | Select-Object -First 1).tools -join ','); tool output: $(($mk | ForEach-Object { $_.outputs } | Where-Object { $_ }) -join ' || '); stderr: $(Tail $r.Err 2)"

# ACP: Clauseo's real launch shape (node.exe + the adapter's dist/index.js), driven over stdio.
foreach ($ad in @(@{ id = 'claude-acp'; js = $ClaudeAcp; files = @('claude-bash.txt', 'claude-write.txt') }, @{ id = 'codex-acp'; js = $CodexAcp; files = @('codex-shell.txt') })) {
    Clear-Outputs
    $pf = New-Policy "r6-acp-$($ad.id)" "`"$Node`" `"$($ad.js)`"" $agentCfg $agentEnv
    $r = Invoke-Timed $Node "`"$Kit\acp-driver.mjs`" `"$Wxc`" `"$pf`" `"$Matter`" 240000" 300
    Set-Content -Path (Join-Path $Logs "r6-acp-$($ad.id).out.txt") -Value $r.Out
    $o = $r.Result
    $files = $ad.files | ForEach-Object { "$_=$(Test-Path (Join-Path $Matter $_))" }
    $ok = $o -and ($o.stop_reason -eq 'end_turn') -and -not ($ad.files | Where-Object { -not (Test-Path (Join-Path $Matter $_)) })
    Row "6d-$($ad.id)" $(if ($ok) { 'PASS' } else { 'FAIL' }) "ACP turn through $($ad.id) (node + adapter inside)" "$(if ($o) { "outcome=$($o.outcome) steps=$($o.steps -join '; ') non-JSON stdout=$($o.non_json_stdout) permissions=$($o.permissions) tools=$($o.tool_calls -join ',') error=$($o.error)" } else { "driver: $(Code $r) $(Tail $r.Err 3)" }); files: $($files -join ' ')"
}

# ---------------------------------------------------------------- summary and clean-up
Section 'Summary'
if (-not $Keep) { try { Stop-Process -Id $proxy.Id -Force -ErrorAction SilentlyContinue } catch {} }
$sum = [ordered]@{ facts = $Facts; rows = $Rows }
$json = $sum | ConvertTo-Json -Depth 8
if ($Out) { $json | Set-Content -Encoding utf8 $Out }
$Rows | ForEach-Object { "{0,-12} {1,-5} {2}" -f $_.id, $_.status, $_.name }
if ($env:GITHUB_STEP_SUMMARY) {
    $lines = @("### MXC rows: $($Facts.os), session $($Facts.session)", '', '| Row | Result | Check | Evidence |', '|---|---|---|---|')
    $lines += $Rows | ForEach-Object { "| $($_.id) | $($_.status) | $($_.name) | $(($_.evidence -replace '\|', '/').Substring(0, [math]::Min(400, $_.evidence.Length))) |" }
    $lines -join "`n" | Add-Content -Encoding utf8 $env:GITHUB_STEP_SUMMARY
}
if ($Keep) {
    "kept: data $Data, matters $Matters, $DocSecret, proxy-mock pid $($proxy.Id) on 127.0.0.1:$P, policies in $Logs\policies, wxc-exec $Wxc"
    exit 0
}
# Remove what this script made outside the work folder.
Remove-Item -Recurse -Force $Data, $Matters -ErrorAction SilentlyContinue
Remove-Item -Force $DocSecret -ErrorAction SilentlyContinue
if ($DocsCreated) { Remove-Item -Recurse -Force $Docs -ErrorAction SilentlyContinue }
exit 0
