# Undoes what probe.ps1 left behind. Run it through cleanup.cmd; no admin rights needed for
# anything the probe can create without them. ASCII only (Windows PowerShell 5.1).
#
# It removes, in order:
#   1. new AppContainer profiles the probe run created (DeleteAppContainerProfile, then the
#      HKCU ...\AppContainer\Mappings\<SID> key if it is still there)
#   2. new HKCU ...\ProcessSandboxes\Mappings\<SID> keys
#   3. new %LOCALAPPDATA%\Packages\<name> folders of those profiles
#   4. new %LOCALAPPDATA%\mxc, %LOCALAPPDATA%\Microsoft\MXC or %LOCALAPPDATA%\mxc-hyperlight folders
#   5. new loopback exemptions and firewall rules (these need admin to remove; it says so)
#   6. the work folder: the 542 MB download, the extracted binaries and the smoke files
# "New" means listed in results\left-behind.json, which the probe writes by comparing snapshots
# taken before and after its runs. Entries whose name looks like an installed app's package
# (it contains "_", as Store package family names do) are reported and left alone: something
# else on the machine may have installed an app during the run.
param(
    [string]$Results = '',
    [string]$Work = ''
)
$ErrorActionPreference = 'Continue'
if (-not $Results) { $Results = Join-Path $PSScriptRoot 'results' }
if (-not $Work) { $Work = Join-Path $env:TEMP 'mxc-probe-kit' }
$AcKey = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings'
$PsKey = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\ProcessSandboxes\Mappings'
Add-Type -Namespace MxcProbeKit -Name Userenv -MemberDefinition @'
[DllImport("userenv.dll", CharSet = CharSet.Unicode)]
public static extern int DeleteAppContainerProfile(string name);
'@

$listFile = Join-Path $Results 'left-behind.json'
$left = $null
if (Test-Path $listFile) {
    $left = Get-Content $listFile -Raw | ConvertFrom-Json
    "using $listFile"
} else {
    "no $listFile (the probe did not finish): removing the work folder, and only MXC-named entries (sandbox-*)"
}

function Is-PackageLike([string]$name) { return $name -like '*_*' }

# 1 and 2: registry
$ac = @(); $ps = @()
if ($left) {
    $ac = @($left.appcontainer_mappings)
    $ps = @($left.process_sandbox_mappings)
} else {
    if (Test-Path $AcKey) {
        $ac = @(Get-ChildItem $AcKey | ForEach-Object { $mon = (Get-ItemProperty -Path $_.PSPath -Name Moniker -ErrorAction SilentlyContinue).Moniker; if ($mon -like 'sandbox-*') { '{0}|{1}' -f $_.PSChildName, $mon } })
    }
    if (Test-Path $PsKey) {
        $ps = @(Get-ChildItem $PsKey | ForEach-Object { $idv = (Get-ItemProperty -Path $_.PSPath -Name Identity -ErrorAction SilentlyContinue).Identity; if ($idv -like 'sandbox-*') { '{0}|{1}' -f $_.PSChildName, $idv } })
    }
}
foreach ($entry in $ac) {
    if (-not $entry) { continue }
    $sid, $moniker = $entry -split '\|', 2
    if (Is-PackageLike $moniker) { "kept AppContainer $moniker ($sid): looks like an installed app's package"; continue }
    if ($moniker) {
        $hr = [MxcProbeKit.Userenv]::DeleteAppContainerProfile($moniker)
        'DeleteAppContainerProfile({0}): 0x{1:X8}' -f $moniker, $hr
    }
    $k = Join-Path $AcKey $sid
    if (Test-Path $k) { Remove-Item -Recurse -Force $k; "removed key $k" }
}
foreach ($entry in $ps) {
    if (-not $entry) { continue }
    $sid = ($entry -split '\|', 2)[0]
    $k = Join-Path $PsKey $sid
    if (Test-Path $k) { Remove-Item -Recurse -Force $k; "removed key $k" }
}

# 3 and 4: folders under LOCALAPPDATA
if ($left) {
    foreach ($name in @($left.packages_folders)) {
        if (-not $name) { continue }
        if (Is-PackageLike $name) { "kept Packages\$name : looks like an installed app's package"; continue }
        $f = Join-Path $env:LOCALAPPDATA "Packages\$name"
        if (Test-Path $f) { Remove-Item -Recurse -Force $f; "removed $f" }
    }
    foreach ($name in @($left.mxc_folders)) {
        if (-not $name) { continue }
        $f = Join-Path $env:LOCALAPPDATA $name
        if (Test-Path $f) { Remove-Item -Recurse -Force $f; "removed $f" }
    }
    # 5: firewall rules and loopback exemptions (admin only)
    foreach ($rule in @($left.firewall_rules)) {
        if (-not $rule -or $rule -like 'not readable*') { continue }
        try { Remove-NetFirewallRule -Name $rule -ErrorAction Stop; "removed firewall rule $rule" } catch { "could not remove firewall rule $rule (needs admin): Remove-NetFirewallRule -Name '$rule'" }
    }
    foreach ($sid in @($left.loopback_exemptions)) {
        if (-not $sid) { continue }
        $o = & CheckNetIsolation.exe LoopbackExempt -d "-p=$sid" 2>&1 | Out-String
        "loopback exemption $sid : $($o.Trim())"
    }
}

# 6: the work folder
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work; "removed $Work" } else { "no $Work" }
''
"done. The results folder ($Results) is kept; delete the kit folder to remove it too."
exit 0
