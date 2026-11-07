# MXC probe kit: can this Windows machine run MXC's process container (Clauseo's tier A)?
# Run it through run-probe.cmd. It needs no admin rights; run it from a normal (non-elevated)
# window. It prints and saves, in order: whether the window is elevated, the OS build and UBR,
# whether the build meets MXC's documented floor, whether processmodel.dll and the PSEC exports
# exist, what `wxc-exec --probe` (MXC v1.0.1, fetched from Microsoft's GitHub release and checked
# against a pinned SHA-256) reports, contained cmd and node smoke runs when the tier is
# base-container, and what the run left behind (compared with a snapshot taken before).
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File probe.ps1 [-Results <dir>] [-Work <dir>]
# Works in Windows PowerShell 5.1 and PowerShell 7. ASCII only on purpose (5.1 reads BOM-less
# files as ANSI).
param(
    [string]$Results = '',
    [string]$Work = ''
)
$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch {}
if (-not $Results) { $Results = Join-Path $PSScriptRoot 'results' }
if (-not $Work) { $Work = Join-Path $env:TEMP 'mxc-probe-kit' }
New-Item -ItemType Directory -Force $Results, $Work | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
try { Start-Transcript -Path (Join-Path $Results "probe-log-$stamp.txt") | Out-Null } catch {}
$summary = [ordered]@{ kit = 'mxc-probe-kit 1'; started = (Get-Date).ToString('s'); work = $Work }

function Section($t) { ''; "=== $t" }
# UTF-8 without a BOM: wxc-exec's JSON parser and non-ASCII user names both need it (5.1's utf8 adds a BOM).
function Write-Utf8([string]$path, [string]$text) { [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false))) }

function Invoke-Timed([string]$exe, [string]$arguments, [int]$timeoutSec = 180) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo($exe, $arguments)
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try { $p = [System.Diagnostics.Process]::Start($psi) } catch {
        return [pscustomobject]@{ Exit = $null; TimedOut = $false; Ms = 0; Out = ''; Err = "start failed: $_" }
    }
    $p.StandardInput.Close()
    $o = $p.StandardOutput.ReadToEndAsync()
    $e = $p.StandardError.ReadToEndAsync()
    $done = $p.WaitForExit($timeoutSec * 1000)
    if (-not $done) { & taskkill.exe /T /F /PID $p.Id 2>&1 | Out-Null; $p.WaitForExit(10000) | Out-Null }
    $sw.Stop()
    $exit = $null
    if ($done) { $exit = $p.ExitCode }
    [pscustomobject]@{ Exit = $exit; TimedOut = (-not $done); Ms = [int]$sw.Elapsed.TotalMilliseconds; Out = $o.Result; Err = $e.Result }
}

function Show($r) {
    if ($r.TimedOut) { $code = 'TIMED OUT' } elseif ($null -eq $r.Exit) { $code = 'did not start' } else { $code = '{0} (0x{1:X8})' -f $r.Exit, $r.Exit }
    "exit: $code in $($r.Ms) ms"
    if ($r.Out) { '--- stdout'; $r.Out.TrimEnd() }
    if ($r.Err) { '--- stderr'; $r.Err.TrimEnd() }
}

# What the run could leave behind, read before and after. Reading these needs no admin.
$AcKey = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings'
$PsKey = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\ProcessSandboxes\Mappings'
function Get-Snapshot {
    $s = [ordered]@{}
    $s.appcontainer_mappings = @()
    if (Test-Path $AcKey) {
        $s.appcontainer_mappings = @(Get-ChildItem $AcKey -ErrorAction SilentlyContinue | ForEach-Object {
                $mon = (Get-ItemProperty -Path $_.PSPath -Name Moniker -ErrorAction SilentlyContinue).Moniker
                '{0}|{1}' -f $_.PSChildName, $mon
            })
    }
    $s.process_sandbox_mappings = @()
    if (Test-Path $PsKey) {
        $s.process_sandbox_mappings = @(Get-ChildItem $PsKey -ErrorAction SilentlyContinue | ForEach-Object {
                $id = (Get-ItemProperty -Path $_.PSPath -Name Identity -ErrorAction SilentlyContinue).Identity
                '{0}|{1}' -f $_.PSChildName, $id
            })
    }
    $s.packages_folders = @(Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Packages') -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
    $s.mxc_folders = @('mxc', 'Microsoft\MXC', 'mxc-hyperlight' | Where-Object { Test-Path (Join-Path $env:LOCALAPPDATA $_) })
    $s.firewall_rules = @()
    try { $s.firewall_rules = @(Get-NetFirewallRule -ErrorAction Stop | ForEach-Object { $_.Name }) } catch { $s.firewall_rules = @("not readable: $($_.Exception.Message)") }
    $lo = & CheckNetIsolation.exe LoopbackExempt -s 2>&1 | ForEach-Object { "$_" }
    $s.loopback_exemptions = @($lo | Where-Object { $_ -match 'S-1-15-' } | ForEach-Object { ($_ -replace '.*(S-1-15-[0-9-]+).*', '$1') })
    $s
}

# ---------------------------------------------------------------- elevation
Section 'Elevation'
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$elevated = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$il = ((whoami /groups | Select-String 'Mandatory Level' | Select-Object -First 1).ToString().Trim() -split '\s{2,}')[0]
$session = (Get-Process -Id $PID).SessionId
"elevated: $elevated ($il), session $session"
if ($elevated) {
    'NOTE: this window is elevated. The probe needs no admin rights, and Clauseo runs agents unelevated;'
    'results from an elevated window may differ. If you can, close it and run run-probe.cmd by double-click.'
}
if ($session -eq 0) { 'NOTE: session 0 (SSH or a service): programs that load USER32 fail in MXC there (microsoft/mxc#1465). Use a desktop session.' }
$summary.elevated = $elevated
$summary.integrity = $il
$summary.session = $session

# ---------------------------------------------------------------- OS
Section 'OS build'
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$build = [int]$cv.CurrentBuildNumber
$ubr = [int]$cv.UBR
$arch = 'X64'
try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { $arch = 'Arm64' } }
"ProductName:      $($cv.ProductName)"
"EditionID:        $($cv.EditionID)"
"InstallationType: $($cv.InstallationType)"
"DisplayVersion:   $($cv.DisplayVersion)"
"Build.UBR:        $build.$ubr"
"OS architecture:  $arch"
$summary.product = "$($cv.ProductName)"
$summary.edition = "$($cv.EditionID)"
$summary.installation_type = "$($cv.InstallationType)"
$summary.display_version = "$($cv.DisplayVersion)"
$summary.build = $build
$summary.ubr = $ubr
$summary.arch = $arch

# MXC's table (docs/backends/process-container/os-version-support.md, Windows 11 only).
Section 'Documented floor (Windows 11 client builds only)'
$floor = @{ 26100 = 9278; 26200 = 9278; 26300 = 9550; 28000 = 2804 }
$floorMet = $null
if ($cv.InstallationType -ne 'Client') {
    "not listed: InstallationType is $($cv.InstallationType); MXC lists Windows 11 client builds only"
} elseif ($floor.ContainsKey($build)) {
    $floorMet = $ubr -ge $floor[$build]
    if ($floorMet) { $verdict = 'MET' } else { $verdict = 'NOT MET (Windows Update may bring it)' }
    "needs $build.$($floor[$build]) or later: $verdict"
} else {
    "build $build is not in MXC's table"
}
$summary.floor_met = $floorMet

# ---------------------------------------------------------------- PSEC
Section 'processmodel.dll and the PSEC exports'
$dll = Join-Path $env:SystemRoot 'System32\processmodel.dll'
$hasDll = Test-Path $dll
"System32\processmodel.dll exists: $hasDll"
if ($hasDll) { 'version: ' + (Get-Item $dll).VersionInfo.FileVersion }
Add-Type -Namespace MxcProbeKit -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr LoadLibraryExW(string name, IntPtr file, uint flags);
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Ansi)]
public static extern IntPtr GetProcAddress(IntPtr module, string name);
'@
$exports = [ordered]@{}
foreach ($mod in 'processmodel.dll', 'kernelbase.dll') {
    $h = [MxcProbeKit.Native]::LoadLibraryExW($mod, [IntPtr]::Zero, 0x800)
    if ($h -eq [IntPtr]::Zero) {
        "LoadLibraryExW($mod) failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        continue
    }
    foreach ($fn in 'CreateProcessSecurityEnvironment', 'QueryProcessSecurityEnvironmentSupport', 'CloseProcessSecurityEnvironment') {
        $found = [MxcProbeKit.Native]::GetProcAddress($h, $fn) -ne [IntPtr]::Zero
        "${mod}!${fn}: $found"
        if ($found) { $exports[$fn] = $mod }
    }
}
$summary.processmodel_dll = $hasDll
$summary.psec_exports = $exports

Section 'Snapshot before (registry, Packages, MXC folders, firewall rules, loopback exemptions)'
$before = Get-Snapshot
"AppContainer mappings $($before.appcontainer_mappings.Count), process-sandbox mappings $($before.process_sandbox_mappings.Count), Packages folders $($before.packages_folders.Count), firewall rules $($before.firewall_rules.Count), loopback exemptions $($before.loopback_exemptions.Count)"

# ---------------------------------------------------------------- wxc-exec
Section 'wxc-exec --probe (MXC v1.0.1 release binaries)'
$zip = Join-Path $Work 'mxc-release-binaries.zip'
$want = '0E832239454BA7FCD338FF56218A287EEF4C0A01C1484084B869F55FEFF8ABE9'
$url = 'https://github.com/microsoft/mxc/releases/download/v1.0.1/mxc-release-binaries.zip'
if (-not (Test-Path $zip) -or (Get-FileHash $zip -Algorithm SHA256).Hash -ne $want) {
    "downloading $url (542 MB) to $zip"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try { Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing } catch { "download failed: $($_.Exception.Message)" }
    "download took $([int]$sw.Elapsed.TotalSeconds) s"
}
$hash = ''
if (Test-Path $zip) { $hash = (Get-FileHash $zip -Algorithm SHA256).Hash }
$tierA = $false
if ($hash -ne $want) {
    "SHA-256 '$hash' does not match the pinned $want : stopping here, nothing run"
    $summary.wxc_tier = $null
    $summary.wxc_error = 'zip missing or hash mismatch'
} else {
    "SHA-256 $hash (matches the pin)"
    if ($arch -eq 'Arm64') { $sub = 'arm64' } else { $sub = 'x64' }
    $wxc = Join-Path $Work "$sub\wxc-exec.exe"
    if (-not (Test-Path $wxc)) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $z = [IO.Compression.ZipFile]::OpenRead($zip)
        foreach ($entry in $z.Entries) {
            if ($entry.FullName.StartsWith("$sub/") -and $entry.Name -and $entry.FullName -notmatch '/symbols/') {
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
    $tierA = ($summary.wxc_tier -eq 'base-container') -and (-not $summary.wxc_needs_dacl)

    # ------------------------------------------------------------ smoke runs
    Section 'Contained smoke runs (only on base-container)'
    if (-not $tierA) {
        "skipped: wxc-exec chose tier '$($summary.wxc_tier)', not base-container without DACL changes"
    } else {
        $m = Join-Path $Work 'matter'
        New-Item -ItemType Directory -Force $m | Out-Null
        function Policy([string]$cmdline, [string[]]$ro, [bool]$ui) {
            $pol = [ordered]@{
                version = '1.0.0'
                containment = 'processcontainer'
                process = [ordered]@{ commandLine = $cmdline; cwd = $m; timeout = 120000 }
                filesystem = [ordered]@{ readwritePaths = @($m); readonlyPaths = @($ro) }
                network = [ordered]@{ egress = @{ default = 'deny' }; ingress = [ordered]@{ default = 'deny'; hostLoopback = 'deny' } }
                ui = [ordered]@{ disable = (-not $ui); clipboard = 'none'; injection = $false }
            }
            $pol | ConvertTo-Json -Depth 6
        }
        $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
        foreach ($ui in $false, $true) {
            $pf = Join-Path $Work "smoke-cmd-ui-$ui.json"
            Write-Utf8 $pf (Policy "`"$cmd`" /d /c echo MXC_OK> smoke.txt & type smoke.txt" @() $ui)
            "--- cmd, UI enabled: $ui"
            $r = Invoke-Timed $wxc "`"$pf`"" 180
            Show $r
            $summary["smoke_cmd_ui_$($ui.ToString().ToLower())"] = [ordered]@{ ok = ($r.Out -match 'MXC_OK'); exit = $r.Exit; ms = $r.Ms }
        }
        $nodeCmd = Get-Command node.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($nodeCmd) {
            $node = $nodeCmd.Source
            if (-not $node) { $node = $nodeCmd.Path }
            $nodeDir = Split-Path $node
            $out0 = Invoke-Timed $node "-e `"console.log('NODE_OK')`"" 60
            "--- node outside MXC: $($out0.Ms) ms, exit $($out0.Exit)"
            $summary.node_outside_ms = $out0.Ms
            foreach ($ui in $true, $false) {
                $pf = Join-Path $Work "smoke-node-ui-$ui.json"
                Write-Utf8 $pf (Policy "`"$node`" -e `"console.log('NODE_OK '+process.version)`"" @($nodeDir) $ui)
                "--- node, UI enabled: $ui (microsoft/mxc#1629: Node needs UI on; 0xC0000142 with UI off)"
                $r = Invoke-Timed $wxc "`"$pf`"" 180
                Show $r
                $summary["smoke_node_ui_$($ui.ToString().ToLower())"] = [ordered]@{ ok = ($r.Out -match 'NODE_OK'); exit = $r.Exit; ms = $r.Ms }
            }
        } else {
            'node.exe is not on PATH: node smoke skipped (optional)'
            $summary.node = 'not on PATH'
        }
    }
}

# ---------------------------------------------------------------- what it left behind
Section 'Left behind (snapshot after minus before)'
$after = Get-Snapshot
$left = [ordered]@{ work_folder = $Work }
foreach ($k in $before.Keys) {
    $new = @($after[$k] | Where-Object { $before[$k] -notcontains $_ })
    $left[$k] = $new
    $shown = 'nothing new'
    if ($new.Count) { $shown = $new -join ', ' }
    "${k}: $shown"
}
"work folder (download, binaries, smoke files): $Work"
Write-Utf8 (Join-Path $Results 'left-behind.json') ($left | ConvertTo-Json -Depth 4)
$summary.left_behind = $left

# ---------------------------------------------------------------- verdict
Section 'Verdict'
$summary.tier_a_possible = [bool]($hasDll -and $tierA)
"MXC process container (tier A) possible here: $($summary.tier_a_possible)"
$summary.finished = (Get-Date).ToString('s')
Write-Utf8 (Join-Path $Results "probe-summary-$stamp.json") ($summary | ConvertTo-Json -Depth 8)
"results: $Results"
"to undo: run cleanup.cmd (it reads $Results\left-behind.json)"
try { Stop-Transcript | Out-Null } catch {}
exit 0
