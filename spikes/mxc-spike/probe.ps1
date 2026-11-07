# MXC probe (G6 part two, step 1): can this Windows machine run tier A, MXC's process container
# (BaseContainer through PSEC)? Throwaway; branch exp-mxc-spike only.
#
# Prints, in order: the OS build and UBR, whether the build meets MXC's documented floor, whether
# processmodel.dll and the PSEC exports exist, what `wxc-exec --probe` (MXC v1.0.1) reports, a
# contained `cmd` and `node` smoke run when the tier is base-container, and Codex 0.162.1's own
# strict MXC check (`windows.sandbox = "mxc"`), the check S15a's tier A is planned behind.
# Clauseo has no MXC probe of its own yet: crates/sandbox/src/detect.rs says tier A is not built.
#
# Usage: pwsh -File probe.ps1 [-Out summary.json] [-Work <scratch dir>]
# Writes only under -Work (default: $env:RUNNER_TEMP or $env:TEMP, subfolder mxc-probe).
param(
    [string]$Out = '',
    [string]$Work = ''
)
$ErrorActionPreference = 'Continue'
if (-not $Work) {
    $base = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
    $Work = Join-Path $base 'mxc-probe'
}
New-Item -ItemType Directory -Force $Work | Out-Null
$summary = [ordered]@{}

function Section($t) { ''; "=== $t" }

# Runs a program with a raw command-line string, a hard timeout and captured output.
function Invoke-Timed([string]$exe, [string]$arguments, [int]$timeoutSec = 180, [hashtable]$envVars = @{}) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new($exe, $arguments)
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    foreach ($k in $envVars.Keys) { $psi.Environment[$k] = $envVars[$k] }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $p.StandardInput.Close()
    $o = $p.StandardOutput.ReadToEndAsync()
    $e = $p.StandardError.ReadToEndAsync()
    $done = $p.WaitForExit($timeoutSec * 1000)
    if (-not $done) { try { $p.Kill($true) } catch {} ; $p.WaitForExit(10000) | Out-Null }
    $sw.Stop()
    [pscustomobject]@{
        Exit = if ($done) { $p.ExitCode } else { $null }
        TimedOut = -not $done
        Ms = [int]$sw.Elapsed.TotalMilliseconds
        Out = $o.Result
        Err = $e.Result
    }
}

function Show($r) {
    "exit: $(if ($r.TimedOut) { 'TIMED OUT' } else { '{0} (0x{1:X8})' -f $r.Exit, $r.Exit }) in $($r.Ms) ms"
    if ($r.Out) { '--- stdout'; $r.Out.TrimEnd() }
    if ($r.Err) { '--- stderr'; $r.Err.TrimEnd() }
}

# ---------------------------------------------------------------- OS
Section 'OS build'
[Environment]::OSVersion | Format-List | Out-String | ForEach-Object { $_.Trim() }
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$build = [int]$cv.CurrentBuildNumber
$ubr = [int]$cv.UBR
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$session = (Get-Process -Id $PID).SessionId
$il = ((whoami /groups | Select-String 'Mandatory Level' | Select-Object -First 1).ToString().Trim() -split '\s{2,}')[0]
"ProductName:      $($cv.ProductName)"
"EditionID:        $($cv.EditionID)"
"InstallationType: $($cv.InstallationType)"
"DisplayVersion:   $($cv.DisplayVersion)"
"Build.UBR:        $build.$ubr"
"OS architecture:  $arch (process: $([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture))"
"Session:          $session ($il)"
$summary.product = "$($cv.ProductName)"
$summary.edition = "$($cv.EditionID)"
$summary.installation_type = "$($cv.InstallationType)"
$summary.display_version = "$($cv.DisplayVersion)"
$summary.build = $build
$summary.ubr = $ubr
$summary.arch = $arch
$summary.session = $session
$summary.integrity = $il

# MXC's table (docs/backends/process-container/os-version-support.md, Windows 11 only):
# process isolation from 26100.9278 / 26200.9278 / 26300.9550 / 28000.2804.
Section 'Documented floor (Windows 11 client builds only)'
$floor = @{ 26100 = 9278; 26200 = 9278; 26300 = 9550; 28000 = 2804 }
if ($cv.InstallationType -ne 'Client') {
    $floorMet = $null
    "not listed: InstallationType is $($cv.InstallationType); MXC lists Windows 11 client builds only (G6 row 1: Server 2025 26100.33650 has no processmodel.dll)"
} elseif ($floor.ContainsKey($build)) {
    $floorMet = $ubr -ge $floor[$build]
    "needs $build.$($floor[$build]) or later: $(if ($floorMet) { 'MET' } else { 'NOT MET' })"
} else {
    $floorMet = $null
    "build $build is not in MXC's table"
}
$summary.floor_met = $floorMet

# ---------------------------------------------------------------- PSEC
Section 'processmodel.dll and the PSEC exports'
$dll = Join-Path $env:SystemRoot 'System32\processmodel.dll'
$hasDll = Test-Path $dll
"System32\processmodel.dll exists: $hasDll"
if ($hasDll) { "version: $((Get-Item $dll).VersionInfo.FileVersion)" }
Add-Type -Namespace MxcProbe -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr LoadLibraryExW(string name, IntPtr file, uint flags);
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Ansi)]
public static extern IntPtr GetProcAddress(IntPtr module, string name);
'@
$exports = [ordered]@{}
foreach ($mod in 'processmodel.dll', 'kernelbase.dll') {
    $h = [MxcProbe.Native]::LoadLibraryExW($mod, [IntPtr]::Zero, 0x800) # LOAD_LIBRARY_SEARCH_SYSTEM32
    if ($h -eq [IntPtr]::Zero) {
        "LoadLibraryExW($mod) failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        continue
    }
    foreach ($fn in 'CreateProcessSecurityEnvironment', 'QueryProcessSecurityEnvironmentSupport', 'CloseProcessSecurityEnvironment') {
        $found = [MxcProbe.Native]::GetProcAddress($h, $fn) -ne [IntPtr]::Zero
        "$mod!${fn}: $found"
        if ($found) { $exports[$fn] = $mod }
    }
}
$summary.processmodel_dll = $hasDll
$summary.psec_exports = $exports

# ---------------------------------------------------------------- wxc-exec
Section 'wxc-exec --probe (MXC v1.0.1 release binaries)'
$zip = Join-Path $Work 'mxc-release-binaries.zip'
$want = '0E832239454BA7FCD338FF56218A287EEF4C0A01C1484084B869F55FEFF8ABE9'
if (-not (Test-Path $zip) -or (Get-FileHash $zip -Algorithm SHA256).Hash -ne $want) {
    $url = 'https://github.com/microsoft/mxc/releases/download/v1.0.1/mxc-release-binaries.zip'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    "downloaded in $([int]$sw.Elapsed.TotalSeconds) s"
}
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
"SHA-256 $hash $(if ($hash -eq $want) { '(matches G6)' } else { '(MISMATCH: stopping)' })"
$wxcDir = Join-Path $Work ($(if ($arch -eq 'Arm64') { 'arm64' } else { 'x64' }))
$wxc = Join-Path $wxcDir 'wxc-exec.exe'
if ($hash -eq $want) {
    if (-not (Test-Path $wxc)) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $z = [IO.Compression.ZipFile]::OpenRead($zip)
        $prefix = (Split-Path $wxcDir -Leaf) + '/'
        foreach ($entry in $z.Entries) {
            if ($entry.FullName.StartsWith($prefix) -and $entry.Name -and $entry.FullName -notmatch '/symbols/') {
                $dest = Join-Path $Work $entry.FullName
                New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dest, $true)
            }
        }
        $z.Dispose()
    }
    "wxc-exec: $wxc"
    $r = Invoke-Timed $wxc '--probe' 120
    Show $r
    $probe = $null
    try { $probe = $r.Out | ConvertFrom-Json } catch {}
    if ($probe) {
        $summary.wxc_tier = $probe.tier
        $summary.wxc_needs_dacl = $probe.needsDaclAugmentation
        $summary.wxc_probes = $probe.probes
        $summary.wxc_error = $probe.error
    } else {
        $summary.wxc_tier = $null
        $summary.wxc_probe_raw = ($r.Out + $r.Err).Trim()
    }
}
$tierA = ($summary.wxc_tier -eq 'base-container') -and (-not $summary.wxc_needs_dacl)

# ---------------------------------------------------------------- smoke runs
Section 'Contained smoke runs (only on base-container)'
if (-not $tierA) {
    "skipped: wxc-exec chose tier '$($summary.wxc_tier)', not base-container without DACL changes"
} else {
    $m = Join-Path $Work 'matter'
    New-Item -ItemType Directory -Force $m | Out-Null
    function Policy($cmdline, [string[]]$ro, [bool]$ui) {
        [ordered]@{
            version = '1.0.0'
            containment = 'processcontainer'
            process = [ordered]@{ commandLine = $cmdline; cwd = $m; timeout = 120000 }
            filesystem = [ordered]@{ readwritePaths = @($m); readonlyPaths = @($ro) }
            network = [ordered]@{ egress = @{ default = 'deny' }; ingress = [ordered]@{ default = 'deny'; hostLoopback = 'deny' } }
            ui = [ordered]@{ disable = (-not $ui); clipboard = 'none'; injection = $false }
        } | ConvertTo-Json -Depth 6
    }
    $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
    foreach ($ui in $false, $true) {
        $pf = Join-Path $Work "smoke-cmd-ui-$ui.json"
        Policy "`"$cmd`" /d /c echo MXC_OK> smoke.txt & type smoke.txt" @() $ui | Set-Content -Encoding utf8 $pf
        "--- cmd, ui enabled: $ui"
        $r = Invoke-Timed $wxc "`"$pf`"" 180
        Show $r
        $summary["smoke_cmd_ui_$($ui.ToString().ToLower())"] = [ordered]@{ ok = ($r.Out -match 'MXC_OK'); exit = $r.Exit; ms = $r.Ms }
    }
    $node = (Get-Command node.exe -ErrorAction SilentlyContinue).Source
    if ($node) {
        $nodeDir = Split-Path $node
        $out0 = Invoke-Timed $node "-e `"console.log('NODE_OK')`"" 60
        "--- node outside MXC: $($out0.Ms) ms, exit $($out0.Exit)"
        $summary.node_outside_ms = $out0.Ms
        foreach ($ui in $true, $false) {
            $pf = Join-Path $Work "smoke-node-ui-$ui.json"
            Policy "`"$node`" -e `"console.log('NODE_OK '+process.version+' '+process.cwd())`"" @($nodeDir) $ui | Set-Content -Encoding utf8 $pf
            "--- node, ui enabled: $ui (microsoft/mxc#1629: ~28 s with UI on, 0xC0000142 with UI off)"
            $r = Invoke-Timed $wxc "`"$pf`"" 180
            Show $r
            $summary["smoke_node_ui_$($ui.ToString().ToLower())"] = [ordered]@{ ok = ($r.Out -match 'NODE_OK'); exit = $r.Exit; ms = $r.Ms }
        }
    } else {
        'node.exe not on PATH: node smoke skipped'
    }
}

# ---------------------------------------------------------------- Codex's strict check
Section 'Codex 0.162.1 strict MXC check (windows.sandbox = "mxc")'
$cx = Join-Path $Work 'codex'
New-Item -ItemType Directory -Force $cx | Out-Null
$npm = (Get-Command npm.cmd -ErrorAction SilentlyContinue).Source
if (-not $npm) {
    'npm not on PATH: skipped'
    $summary.codex_mxc = 'skipped: no npm'
} else {
    $r = Invoke-Timed $env:ComSpec "/d /s /c `"`"$npm`" install --prefix `"$cx`" --no-audit --no-fund @openai/codex@0.162.1`"" 600
    "npm install: exit $($r.Exit) in $($r.Ms) ms"
    $codex = Get-ChildItem -Path (Join-Path $cx 'node_modules') -Recurse -Filter codex.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $codex) {
        Show $r
        'codex.exe not found after npm install'
        $summary.codex_mxc = 'skipped: codex.exe not installed'
    } else {
        "codex.exe: $($codex.FullName)"
        $ch = Join-Path $Work 'codex-home'
        $ws = Join-Path $Work 'codex-ws'
        New-Item -ItemType Directory -Force $ch, $ws | Out-Null
        $r = Invoke-Timed $codex.FullName "-c `"windows.sandbox=\`"mxc\`"`" sandbox -P :workspace -C `"$ws`" -- cmd /c echo MXC_OK" 180 @{ CODEX_HOME = $ch }
        Show $r
        $summary.codex_mxc = if ($r.Out -match 'MXC_OK') { 'MXC_OK' } else { (($r.Err + $r.Out).Trim() -split "`n" | Select-Object -Last 1) }
    }
}

# ---------------------------------------------------------------- verdict
Section 'Verdict'
$summary.clauseo_probe = 'none: tier A is not built (crates/sandbox/src/detect.rs:127); ARM64 is tier C until tested (detect.rs:131, G6 row 13)'
$summary.tier_a_possible = [bool]($hasDll -and $tierA -and ($summary.codex_mxc -eq 'MXC_OK'))
"tier A possible here: $($summary.tier_a_possible)"
$json = $summary | ConvertTo-Json -Depth 8 -Compress
"SUMMARY $json"
if ($Out) { $json | Set-Content -Encoding utf8 $Out }
if ($env:GITHUB_STEP_SUMMARY) {
    @(
        "### MXC probe: $($summary.product) $build.$ubr ($arch, $($summary.installation_type))",
        '',
        "| Question | Answer |", '|---|---|',
        "| Documented floor met | $floorMet |",
        "| processmodel.dll | $hasDll |",
        "| wxc-exec tier | $($summary.wxc_tier) (needs DACL: $($summary.wxc_needs_dacl)) |",
        "| Codex strict MXC check | $($summary.codex_mxc) |",
        "| cmd in container (UI off / on) | $($summary.smoke_cmd_ui_false.ok) $($summary.smoke_cmd_ui_false.ms) ms / $($summary.smoke_cmd_ui_true.ok) $($summary.smoke_cmd_ui_true.ms) ms |",
        "| node in container (UI on / off) | $($summary.smoke_node_ui_true.ok) $($summary.smoke_node_ui_true.ms) ms / $($summary.smoke_node_ui_false.ok) $($summary.smoke_node_ui_false.ms) ms; outside $($summary.node_outside_ms) ms |",
        "| Session | $session ($il) |",
        "| **Tier A possible** | **$($summary.tier_a_possible)** |"
    ) -join "`n" | Add-Content -Encoding utf8 $env:GITHUB_STEP_SUMMARY
}
exit 0
