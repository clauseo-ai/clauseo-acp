# Clauseo for firm IT

<!-- The app links to the `<a id>` anchors in this file (sandbox-policy, sandbox-logon-right,
sandbox-other-account, sandbox-setup, sandbox-required, unsafe-site-override, sandbox-tool-missing,
sandbox-admin-copy, sandbox-leftover-task); keep their ids when you edit. -->

How to deploy, lock down and support Clauseo on Windows, and on Macs under MDM. Each fact names its
source: a file in this repo, a decision record, a test run or a vendor page. When a fact changes, the
pull request that changes it changes this page too.

Written for Clauseo 0.1.x (2026-11). Nothing here is signed with a publisher certificate yet: see
[section 2](#2-signing).

## 1. Which installer

Each release on GitHub ([decision 0035](decisions/0035-release-builds.md), "Artefacts by tier") has:

| File | For | Installs to |
|---|---|---|
| `Clauseo_<v>_x64-machine-setup.exe` | **Firms.** Per machine, needs an administrator, never updates itself | `C:\Program Files\Clauseo` |
| `Clauseo_<v>_x64-setup.exe` | People who install it themselves. Per user, no administrator, updates itself | `%LOCALAPPDATA%\Clauseo` |
| `Clauseo_<v>_arm64-machine-setup.exe`, `Clauseo_<v>_arm64-setup.exe` | Windows on ARM, in every release. The x64 per-machine installer still installs there, under emulation, and names the arm64 one in its details list | as above |
| `Clauseo_<v>_aarch64.dmg`, `Clauseo_<v>_x64.dmg` | Macs (Apple silicon; Intel is tier 2) | `/Applications` |
| `Clauseo_<v>_amd64.deb`, `Clauseo-<v>-1.x86_64.rpm` | Linux (tier 2). Updates come through your package manager only | |
| `SHA256SUMS.txt` | The SHA-256 of every file above | |

The install guides for the people at the firm, with each warning and the clicks past it:
[Windows](install/windows.md) and [macOS](install/macos.md).

Both Windows installers ship the same `Clauseo.exe` and use the stock NSIS pages, in English and
Hindi (decision 0035): the installer follows the language chosen in the app
(`HKCU\Software\Clauseo\UiLanguage`, `en` or `hi`) when it is set, else Windows' display
language, and the uninstaller that value as it was at the last install or update. Seen on
Windows Server 2025 (build 26100) on 2026-11-03, with the 0.0.0 dry-run installers:

- The per-machine installer raises a UAC prompt, installs `Clauseo.exe` and `uninstall.exe` in
  `C:\Program Files\Clauseo` and writes
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo` (64-bit view) with `DisplayName` and
  `Publisher` `Clauseo`, `DisplayVersion` `<v>`, `InstallLocation` `"C:\Program Files\Clauseo"` and
  `UninstallString` `"C:\Program Files\Clauseo\uninstall.exe" _?=C:\Program Files\Clauseo` (both
  paths in quotes but the one after `_?=`, which takes the rest of the line), `QuietUninstallString`
  `"C:\Program Files\Clauseo\uninstall.exe" /S _?=C:\Program Files\Clauseo`, `DisplayIcon`,
  `MainBinaryName`, `NoModify` and `NoRepair` 1, and `EstimatedSize`. It also writes
  `HKLM\SOFTWARE\Clauseo\Clauseo`, whose default value is the install folder; the per-machine
  uninstaller deletes that key, and `HKLM\SOFTWARE\Clauseo` when nothing else is left in it
  (an update keeps both). It makes two shortcuts for all users,
  `C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Clauseo.lnk` and
  `C:\Users\Public\Desktop\Clauseo.lnk`, also with `/S`. It writes nothing under `HKCU`. The app it starts at the end runs un-elevated (medium integrity).
- The per-user installer installs in `%LOCALAPPDATA%\Clauseo` and writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo`. At the end of an
  interactive install it says a UAC prompt is next, then raises one, to set up Safe workspace (S15
  tier B): elevated, it writes the sandbox's set-up program from the installer itself into
  `C:\Program Files\Clauseo\Sandbox` and runs it from there. A standard user needs an
  administrator to type their password into the prompt; Safe workspace is set up for the user who
  ran the installer. Declined or failed, Clauseo is installed and works without it, and
  running the installer again sets it up. A silent (`/S`) or passive (`/P`) per-user install
  raises no prompt and skips it; Clauseo's updates never prompt. The per-machine installer, already
  elevated, writes and records the set-up program in the same folder on every install and update,
  but sets nobody up: no sandbox users, tasks or network rules. Each lawyer's Safe workspace is set
  up later, from Clauseo's first-run step, which raises a UAC prompt; a standard user needs an
  administrator to type their password into it.

**Exit codes** of the installers, besides NSIS's 0 (done), 1 (cancelled) and 2 (stopped by an
error). Only the last code set is reported, so an install that set several reports one of them.

What a silent (`/S`) install can return:

| Code | Meaning |
|---|---|
| 20 | Per-machine install: installed, but Windows security removed the set-up program this install wrote (see [section 10](#10-if-windows-security-removed-clauseos-set-up-program)) |
| 22 | Per-user install: installed, but Safe workspace is not set up (no prompt under `/S`) |
| 23 | Per-machine install: installed, but placing the set-up program in `C:\Program Files\Clauseo\Sandbox` failed or didn't finish (still running after 10 minutes) |
| 24 | Per-machine install: installed, but the set-up program wasn't placed because Clauseo was running in the session that ran the installer |
| 25 | Per-machine install: nothing installed, because the installer couldn't make its protected temporary folder in `C:\Program Files\Clauseo` |
| 28 | Install: nothing usable installed, because Windows Security (or another antivirus) removed or blocked `Clauseo.exe` while it was being written; allow Clauseo in Windows Security's Protection history, then install again |

A 23 after a 10-minute wait means the placing may still be running; it finishes on its own. A
`/S` run as SYSTEM (Intune) runs in session 0, which sees no user's Clauseo window, so it doesn't return 24 for a user's running Clauseo.

What an interactive install can also return:

| Code | Meaning |
|---|---|
| 20 | Installed, but Windows security removed the set-up program this install wrote (see [section 10](#10-if-windows-security-removed-clauseos-set-up-program)) |
| 24 | Per-user install: installed, but Safe workspace is not set up because Clauseo was running |
| 28 | Install: nothing usable installed, because Windows Security (or another antivirus) removed or blocked `Clauseo.exe` while it was being written; allow Clauseo in Windows Security's Protection history, then install again |

An interactive per-machine install never sets 23: it shows a box when placing the set-up program failed, and a
line in its details list when it was still running. It shows a box with 24 and 25 too, but not
with 25 under `/P` or `/UPDATE`. With 28 every mode shows a box except `/S`, and the install
stops as failed: no finish page, and nothing to start.

A passive (`/P`) install shows no box and sets no exit code for Safe workspace's outcomes, except
the per-machine installer's 24, which every mode sets. Both installers set 28 in every mode,
`/P` included, with its box.

What a per-machine uninstall can also return: 25 and 26 in any mode (a box unless `/S`; for 25,
unless `/P` or `/UPDATE` too), 27 under `/S` only:

| Code | Meaning |
|---|---|
| 25 | Per-machine uninstall: nothing uninstalled, because the uninstaller couldn't make its protected temporary folder in `C:\Program Files\Clauseo` |
| 26 | Per-machine uninstall: nothing uninstalled, because it wasn't run from its install folder, as when an option was put after `_?=` (see below) |
| 27 | Per-machine `/S` uninstall: uninstalled, but the protection stays, since the set-up program in `C:\Program Files\Clauseo\Sandbox` failed its check (see [section 10](#10-if-windows-security-removed-clauseos-set-up-program)) |

A `/S` uninstall can also return 21: uninstalled, but the protection stays, since the set-up
program was gone (see [section 10](#10-if-windows-security-removed-clauseos-set-up-program)). A
silent per-user uninstall never returns 27: it skips the protection's removal.

The per-machine installer, and the per-user installer's elevated step, keep their temporary files,
plug-ins included, in a new `ns….tmp` folder in `C:\Program Files\Clauseo`, which only
Administrators and SYSTEM can open, and delete it when they finish. The per-machine uninstaller
keeps its own plug-ins there too. Earlier versions put these folders in
`C:\Program Files\Clauseo\installer-temp`, which stayed behind empty; the installer and the
uninstaller remove an empty leftover `installer-temp`.

A per-machine uninstall runs `uninstall.exe` where it is installed (the `_?=` in its registry
commands), not from a copy in `%TEMP%`. `_?=` takes the rest of the line as the install folder, so
use `QuietUninstallString` as it is, or put any option before `_?=`: an option after it makes the
uninstaller refuse with 26 and remove nothing. Started any other way (by hand, without `_?=`), it
refuses with 26 too. `uninstall.exe` may stay in the install folder for a few seconds after the
uninstall ends, until a hidden `cmd` it starts deletes it; if the install folder's path holds one
of `% ^ & | < > " !`, it stays until someone deletes it. It's harmless either way. Installs
registered by an earlier version keep their old `UninstallString` until reinstalled.

- Uninstalling asks one question, an unticked box: "Also delete conversations, saved versions and
  settings". Left unticked, each user's data folder (`%LOCALAPPDATA%\ai.clauseo.desktop`) stays.
  The silent `/S` uninstall asks nothing and always keeps it. Ticked, `Clauseo.exe` deletes the
  uninstalling user's data folder and `%APPDATA%\ai.clauseo.desktop` without following links: a
  junction or symbolic link inside them is removed as a link, and what it points at is left as it
  is. A folder moved with `CLAUSEO_DATA_DIR` is never touched. The `sandbox` folder is kept
  unless removing the sandbox's set-up succeeded, and the files the sandbox set aside from a
  matter (`sandbox\set-aside`) are always kept. A message names the kept folder, or the folder to
  delete by hand when something could not be deleted and nothing was kept.
- Where Clauseo's Windows protection was set up (task S15), uninstalling first removes it, elevated
  (`clauseo-sandbox-setup.exe --remove`): the local sandbox users `ClauseoSb01` to `ClauseoSb32`,
  their profiles, the group `ClauseoSandboxUsers`, the key `HKLM\SOFTWARE\Clauseo\Sandbox` and the
  access rules Clauseo put on the lawyer's folders. A per-user uninstall asks for one UAC prompt
  and removes it only for the Windows user who set it up; anyone else's uninstall leaves it. A
  silent per-user uninstall (`/S`) asks for nothing, so it leaves the protection in place and
  keeps the `sandbox` folder, saying so in its details list. To remove the protection, reinstall
  Clauseo and uninstall it interactively, or, before the silent uninstall (which deletes the tool
  with the rest of the install folder), run `clauseo-sandbox-setup.exe --remove --per-machine`
  from the install folder at an elevated prompt: it removes every sandbox user on the machine,
  whoever runs it. Run by an administrator other than the lawyer, it takes Clauseo's own entries
  off each matter folder, but the low integrity labels inherited below each matter folder stay;
  clear them with the two `icacls` commands under "Intune (Win32 app)" below. The
  per-machine uninstall removes it whoever uninstalls, the silent `/S` as SYSTEM included. The
  uninstaller waits for it at most 2 minutes, then carries on; the removal finishes on its own, and
  `clauseo-sandbox-setup.exe` may stay in the install folder until it has. If the removal doesn't
  finish (a folder it can't reach, say), the `sandbox` folder and the key stay: reinstall
  Clauseo, then uninstall it again, and the new removal finishes the job. If Windows security
  removed `clauseo-sandbox-setup.exe`, see [section 10](#10-if-windows-security-removed-clauseos-set-up-program).
  If it was deleted with the install folder, see
  [Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand).
- After an uninstall, `C:\Program Files\Clauseo\Sandbox` or the scheduled tasks
  `\Clauseo\SandboxRefresh` and `\Clauseo\SandboxStop` may occasionally stay behind; the
  Windows Application event log names the step that couldn't finish. An administrator can delete
  them by hand (see
  [Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand)), or
  reinstall Clauseo and then uninstall it.

### Intune (Win32 app)

| Field | Value |
|---|---|
| Install command | `Clauseo_<v>_x64-machine-setup.exe /S` |
| Uninstall command | `"C:\Program Files\Clauseo\uninstall.exe" /S _?=C:\Program Files\Clauseo` (the registry's `QuietUninstallString`) |
| Install behaviour | System |
| Detection, registry | Key `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo`, value `DisplayVersion`, string comparison, equals `<v>`; "Associated with a 32-bit app on 64-bit clients": No |
| Detection, file (instead) | `C:\Program Files\Clauseo`, file `Clauseo.exe`, exists. If you compare the file's version instead, `Clauseo.exe`'s File version is `<v>`, read as `<v>.0` |

Write the uninstall path out in full rather than `%ProgramFiles%`: Intune's management extension
is a 32-bit process, where `%ProgramFiles%` means `C:\Program Files (x86)`. `/S` is NSIS's silent
switch and must come before `_?=`, which takes the rest of the line as the install folder and runs
the uninstaller there; it returns once it has finished, `uninstall.exe` itself aside (not yet
measured on Windows). Without `_?=` this version refuses with 26. Clauseo installed by an earlier
version: use `"C:\Program Files\Clauseo\uninstall.exe" /S`, which hands over to a copy of itself in
`%TEMP%` and returns before the files are gone. On our test machine the stub returned after 0.46 s
and the folder was gone 0.26 s later. Windows PowerShell's `Start-Process -Wait` waits for that
copy too. Either way it leaves each
user's `%LOCALAPPDATA%\ai.clauseo.desktop` in place. As SYSTEM it removes the protection's
sandbox users, group and key as well (above), allowing up to 2 minutes more, and the access rules
Clauseo put on each matter folder it recorded.

It doesn't go through the files inside a lawyer's matters, so the copies Windows made of those
rules below each matter folder stay: entries naming the deleted sandbox users, and the low
integrity label the matter had while Clauseo protected it. They give nothing to any account that
still exists. To clear them, an administrator runs two commands on the matter folder:

1. `icacls "D:\Matters\Example" /reset /T /C` removes the stale entries. It resets the folder and
   everything below it to the permissions it inherits, so don't use it on a matter where your firm
   set permissions on anything inside.
2. `icacls "D:\Matters\Example" /setintegritylevel (OI)(CI)M` puts back the default (medium)
   integrity label, which everything below inherits. The first command leaves the low label.

These two commands were measured on a Windows Server 2025 VM, 2026-11-07 (S15 PR4j,
`235c39149`).

Measured on Windows Server 2025 (build 26100), 2026-11-03, with the 0.0.0 dry-run installer, from an
elevated PowerShell: install exit 0 in about 2 s, with no window and no UAC prompt; uninstall exit
0; both detection forms find the install. Not yet run through Intune itself. If you try the install
line by hand on a copy you downloaded with a browser, run `Unblock-File` on it first. Otherwise
Windows shows "Open File - Security Warning" (Unknown Publisher) and waits, even from an elevated
prompt, until someone presses Run.

Never run Clauseo elevated: WebView2 can fail to start when its data folder belongs to another
user ([tauri#13926](https://github.com/tauri-apps/tauri/issues/13926)).

### Clearing a leftover Windows sandbox by hand

The normal way to remove Clauseo's Windows protection is
`clauseo-sandbox-setup.exe --remove --per-machine` from the install folder, at an elevated prompt
(above). Use the commands below only when that exe is gone: quarantined by an antivirus (next
subsection), or deleted with the install folder. Close Clauseo for every user first, and don't run
Clauseo's set-up while you work.

They remove what the set-up makes:

- each local user named `ClauseoSb01` to `ClauseoSb32`, exactly. The commands match the name
  exactly, digits and case included, rather than `ClauseoSb*`, which would also catch other
  accounts that start the same way;
- each of those users' processes, then their profiles, deleted through Windows'
  `Win32_UserProfile` (which removes the folder and its `ProfileList` entry). Don't delete the
  `C:\Users\ClauseoSbNN` folders yourself;
- each user's value under
  `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList`, which
  hides it from the sign-in screen;
- the group's deny entry on `C:\ProgramData\Microsoft\DeviceSync`;
- the group `ClauseoSandboxUsers`, once it has no members. Set-up also gives the group the user
  right "Deny log on through Remote Desktop Services", so no sandbox user can sign in over Remote
  Desktop; `--remove` takes it off before deleting the group. These commands leave it: once the
  group is gone, the entry names a SID no account has, which does nothing. Local Security Policy
  (`secpol.msc`, User Rights Assignment) shows it as a bare SID, which you may delete there;
- the key `HKLM\SOFTWARE\Clauseo\Sandbox`. After a per-machine install, the installer's key
  `HKLM\SOFTWARE\Clauseo\Clauseo` stays beside it. After a per-user install, `HKLM\SOFTWARE\Clauseo`
  is left empty, and you can delete it once you have checked that it is.

The set-up also adds network filters (WFP) and four Windows Firewall rules for the sandbox users
(see [section 11](#11-windows-protection-the-admin-copy-its-tasks-and-its-network-rules)), and
`--remove` deletes them. The block below removes the firewall rules, matched by their exact names
and description. Stock Windows has no command that deletes WFP filters, so the block only lists
Clauseo's (after it).

Collect the SIDs before deleting anything: once a user is deleted, its name no longer leads to its
SID, and its profile can only be found by SID. The key's `PoolSids` lists the users' SIDs too, so
an earlier removal that stopped partway is finished as well. The block then follows `--remove`'s
order. Paste it into an elevated Windows PowerShell (64-bit, the default; in Intune, "Run script in 64 bit PowerShell Host"):

```powershell
$pool = '^ClauseoSb(0[1-9]|[12][0-9]|3[0-2])$'
$key = 'HKLM:\SOFTWARE\Clauseo\Sandbox'
$hide = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
$record = Get-ItemProperty -Path $key -ErrorAction SilentlyContinue
# 1. The SIDs, first.
$users = @(Get-LocalUser | Where-Object { $_.Name -cmatch $pool })
$sids = @(@($users | ForEach-Object { $_.SID.Value }) + @($record.PoolSids) |
    Where-Object { $_ } | Sort-Object -Unique)
$group = Get-LocalGroup -Name 'ClauseoSandboxUsers' -ErrorAction SilentlyContinue
$groupSids = @(@($group.SID.Value, $record.GroupSid) | Where-Object { $_ } | Sort-Object -Unique)
"Sandbox account SIDs:"; $sids  # Keep this list: the matter folders' entries name these SIDs.
# 2. Their processes, so their profiles can be deleted.
Get-CimInstance Win32_Process | ForEach-Object {
    $owner = Invoke-CimMethod -InputObject $_ -MethodName GetOwnerSid -ErrorAction SilentlyContinue
    if ($owner.Sid -and $sids -contains $owner.Sid) {
        Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }
}
# 3. Their profiles, then their accounts, then their sign-in screen values.
Get-CimInstance Win32_UserProfile | Where-Object { $sids -contains $_.SID -and $_.LocalPath -match '\\ClauseoSb(0[1-9]|[12][0-9]|3[0-2])(\.[^\\]+)?$' } | Remove-CimInstance
$users | Remove-LocalUser
if (Test-Path $hide) {
    (Get-Item $hide).Property | Where-Object { $_ -cmatch $pool } |
        ForEach-Object { Remove-ItemProperty -Path $hide -Name $_ }
}
# 4. The group's deny on DeviceSync.
$deviceSync = Join-Path $env:ProgramData 'Microsoft\DeviceSync'
if (Test-Path $deviceSync) {
    foreach ($sid in $groupSids) { icacls $deviceSync /remove:d "*$sid" }
}
# 5. The group, only if it is the one the key names and is empty.
if ($group -and (-not $record.GroupSid -or $group.SID.Value -eq $record.GroupSid) -and
    @(Get-LocalGroupMember -Group $group).Count -eq 0) {
    Remove-LocalGroup -SID $group.SID
}
# 5a. Clauseo's four firewall rules, by exact name and description: listed, then removed.
$fw = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -cmatch '^clauseo_sandbox_block_(outbound|inbound|loopback_udp|loopback_tcp)$' -and
        $_.Description -clike 'Clauseo sandbox - *' })
$fw | Format-Table DisplayName, Direction, Description -AutoSize
$fw | Remove-NetFirewallRule
# 5b. Clauseo's two SYSTEM tasks, by exact path and name: listed, then removed; then their folder, if empty.
$tasks = @(Get-ScheduledTask -TaskPath '\Clauseo\' -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -cin 'SandboxStop', 'SandboxRefresh' })
$tasks | Format-Table TaskPath, TaskName -AutoSize
$tasks | Unregister-ScheduledTask -Confirm:$false
try {
    $ts = New-Object -ComObject Schedule.Service; $ts.Connect(); $folder = $ts.GetFolder('\Clauseo')
    if ($folder.GetTasks(1).Count -eq 0 -and $folder.GetFolders(0).Count -eq 0) { $ts.GetFolder('\').DeleteFolder('Clauseo', 0) }
} catch { }
# 6. The key, last.
Remove-Item -Path $key -ErrorAction SilentlyContinue
```

Then list any WFP filters of Clauseo's left behind (read-only: it writes an XML file and searches
it for the provider's key, `{a5170842-1321-4b98-aa1e-e79c132f1c10}`; every Clauseo filter is
named `Clauseo sandbox ...` and sits in the sublayer
`{91037008-b7c9-4530-b42e-287700239f22}`):

```powershell
$wfp = Join-Path $env:TEMP 'clauseo-wfp-filters.xml'
netsh wfp show filters file=$wfp | Out-Null
@(Select-String -Path $wfp -SimpleMatch -Pattern 'a5170842-1321-4b98-aa1e-e79c132f1c10').Count  # 0: none left
Remove-Item $wfp
```

A number above 0 means Clauseo's filters are still there: they block only the sandbox users, so
they do no harm once those accounts are gone. To remove them, reinstall Clauseo, set up Safe
workspace again (which rewrites them), then uninstall Clauseo. `--remove` alone does nothing
once the key is gone.

Then check that each item is gone. The check below changes nothing and defines everything it
uses, so it works in any elevated PowerShell, before or after the clean-up. Every number is 0 and
both other values are `False`. `Profiles` finds profiles by folder name, and by the SIDs in the
key's `PoolSids` while the key is there. `DeviceSync` counts deny entries naming the group (by
name, or by the SID the key records while it is there), or a deleted local account or group, shown
as a bare `S-1-5-21-…` SID whose last number is 1000 or more: one of those is most likely the
deleted group's, but check it before you remove it. Windows keeps a deny of its own there for the
built-in Guest account, which on a computer set up from an image can show as a bare SID ending in
`-501`. The check skips SIDs ending below 1000; leave those entries alone. `Folders` counts the
folders left in
`C:\Users` under a pool user's exact name, with or without the `.<COMPUTER>` or `.000` Windows may
add, since a profile whose folder was held open can lose its entry and keep its folder:

```powershell
$pool = '^ClauseoSb(0[1-9]|[12][0-9]|3[0-2])$'
$folderName = '^ClauseoSb(0[1-9]|[12][0-9]|3[0-2])(\.[^\\]+)?$'
$key = 'HKLM:\SOFTWARE\Clauseo\Sandbox'
$hide = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
$deviceSync = Join-Path $env:ProgramData 'Microsoft\DeviceSync'
$poolSids = @((Get-ItemProperty -Path $key -ErrorAction SilentlyContinue).PoolSids | Where-Object { $_ })
$groupSid = (Get-ItemProperty -Path $key -ErrorAction SilentlyContinue).GroupSid
$denies = @((Get-Acl -Path $deviceSync -ErrorAction SilentlyContinue).Access | Where-Object {
    $id = $_.IdentityReference.Value
    $_.AccessControlType -eq 'Deny' -and ($id -match '(^|\\)ClauseoSandboxUsers$' -or ($groupSid -and $id -eq $groupSid) -or
        ($id -match '^S-1-5-21-\d+-\d+-\d+-(\d+)$' -and [int64]$Matches[1] -ge 1000)) })
[pscustomobject]@{
    Users      = @(Get-LocalUser | Where-Object { $_.Name -cmatch $pool }).Count
    Profiles   = @(Get-CimInstance Win32_UserProfile | Where-Object { $_.LocalPath -match '\\ClauseoSb(0[1-9]|[12][0-9]|3[0-2])(\.[^\\]+)?$' -or $poolSids -contains $_.SID }).Count
    Hidden     = @((Get-Item $hide -ErrorAction SilentlyContinue).Property | Where-Object { $_ -cmatch $pool }).Count
    DeviceSync = $denies.Count
    Group      = [bool](Get-LocalGroup -Name 'ClauseoSandboxUsers' -ErrorAction SilentlyContinue)
    Folders    = @(Get-ChildItem -Path 'C:\Users' -Directory -Force | Where-Object { $_.Name -cmatch $folderName }).Count
    Tasks      = @(Get-ScheduledTask -TaskPath '\Clauseo\' -ErrorAction SilentlyContinue).Count
    Key        = Test-Path $key
}
```

A `ClauseoSbNN` entry that remains is a link: check where it points, then remove just the link with `rmdir C:\Users\ClauseoSbNN` (cmd), which doesn't touch its target.

If a profile is still there, a process still holds it: restart the computer and paste the block
again. A second run skips what is gone. If the group is still there, it has members or isn't the
one the key named, and `--remove` would leave it too: look at it before you delete it.

If `Profiles` is 0 but `Folders` isn't, a profile's folder may have outlived its profile; it stays
even after a restart. Windows may have named it `ClauseoSb01`, `ClauseoSb01.<COMPUTER>` or
`ClauseoSb01.000`. First check that each counted folder is a leftover Clauseo profile: for
example, it holds a profile's files (`NTUSER.DAT`, `AppData`) and Clauseo was set up on this
computer. The block deletes every folder with such a name, whoever made it. After a restart,
paste the block below into an elevated PowerShell. It is for a leftover sandbox
profile folder only: it finds each folder in `C:\Users` named exactly as a pool user's profile
(never another account that starts with `ClauseoSb`), and deletes what is inside it one name at a
time, then the folder. It uses the backup and restore rights an administrator already has, so it
changes no owner and no permission: a folder a sandboxed program wrote can hold links and hard links
to files elsewhere, and changing permissions through them would change those files too. A link is
deleted as a link, never followed, and a file with other hard links keeps its attributes. It names
anything it couldn't delete. Run without elevation, it stops with "Run this in an elevated
PowerShell."

```powershell
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Priv {
  [StructLayout(LayoutKind.Sequential, Pack = 4)] struct TP { public int Count; public long Luid; public int Attr; }
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr p, int access, out IntPtr t);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool LookupPrivilegeValue(string s, string n, out long l);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool AdjustTokenPrivileges(IntPtr t, bool d, ref TP n, int l, IntPtr p, IntPtr r);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  public static bool Enable(string name) {
    IntPtr t; if (!OpenProcessToken(GetCurrentProcess(), 0x28, out t)) return false;
    TP tp = new TP(); tp.Count = 1; tp.Attr = 2; if (!LookupPrivilegeValue(null, name, out tp.Luid)) return false;
    return AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero) && Marshal.GetLastWin32Error() == 0;
  }
}
'@
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Del {
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetFileInformationByHandle(IntPtr h, int cls, ref uint info, uint size);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  // Removes one name (a file, an empty directory or a link itself), never following a link and never changing attributes.
  public static void Remove(string path) {
    IntPtr h = CreateFileW(path, 0x00010000, 7, IntPtr.Zero, 3, 0x02000000 | 0x00200000, IntPtr.Zero);
    if (h == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    try { uint flags = 0x1 | 0x2 | 0x10; if (!SetFileInformationByHandle(h, 21, ref flags, 4)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
    finally { CloseHandle(h); }
  }
}
'@
if (-not ([Priv]::Enable('SeBackupPrivilege') -and [Priv]::Enable('SeRestorePrivilege'))) { throw 'Run this in an elevated PowerShell.' }
$skipped = New-Object System.Collections.Generic.List[string]
function Remove-Leftover([string]$Dir) {
    foreach ($e in @([IO.Directory]::EnumerateFileSystemEntries($Dir))) {
        $a = [IO.File]::GetAttributes($e)
        try {
            if ($a -band [IO.FileAttributes]::ReparsePoint) {
                [Del]::Remove($e)
            } elseif ($a -band [IO.FileAttributes]::Directory) {
                Remove-Leftover $e
            } else {
                [Del]::Remove($e)
            }
        } catch { $skipped.Add("$($e): $($_.Exception.Message)") }
    }
    [Del]::Remove($Dir)
}
foreach ($Root in @(Get-ChildItem -LiteralPath 'C:\Users' -Directory -Force |
        Where-Object { $_.Name -cmatch '^ClauseoSb(0[1-9]|[12][0-9]|3[0-2])(\.[^\\]+)?$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) } |
        ForEach-Object { $_.FullName })) {
    try { Remove-Leftover $Root } catch { $skipped.Add("$($Root): $($_.Exception.Message)") }
    if (Test-Path -LiteralPath $Root) { $skipped | ForEach-Object { "not deleted: $_" }; throw "$Root is still there" } else { "$Root is gone" }
}
```

Never use `takeown /R`, `icacls /T` or `robocopy /MIR` on these folders: they follow links, or
rewrite files shared through hard links.

Each pool user's `AppData\LocalLow` deny goes with its profile. The matter folders aren't covered
here: clear the entries and low integrity labels Clauseo left on each one with the two `icacls`
commands under "Intune (Win32 app)" above, and give back a folder a sandbox user owns as
[section 9](#9-windows-protection-a-matter-folder-clauseo-cant-take-back) says. Each lawyer's
`sandbox` folder in their data folder stays as it is; keep its `set-aside` folder, which holds
files the sandbox set aside from a matter.

### If Defender quarantines a Clauseo file

See [section 10](#10-if-windows-security-removed-clauseos-set-up-program): what the installers
do when `clauseo-sandbox-setup.exe` is gone, and how to get it back and check it.

<a id="sandbox-setup"></a>
### If the Safe workspace can't be set up

On Windows, when something on the computer stops the Safe workspace's set-up, the app names the
reason, and "Copy details for IT" links to the part of this page that fixes it:

- a Windows policy blocks the set-up program: [section 3](#sandbox-policy);
- Windows won't let the Safe workspace's hidden Windows accounts sign in:
  [below](#sandbox-logon-right);
- the Safe workspace is set up for another Windows account: [below](#sandbox-other-account);
- the set-up program's admin copy is missing, changed or unsafe: [section 10](#sandbox-admin-copy);
- Windows security blocked or removed the set-up program: [section 10](#sandbox-tool-missing);
- a leftover Windows task blocks it: [below](#sandbox-leftover-task).

For any other reason, the host log ([section 4](#4-webview2), "The host log") says what failed.

<a id="sandbox-leftover-task"></a>
If Clauseo reports that the Safe workspace is blocked by a leftover Windows task, open
`%ProgramData%\Clauseo\Sandbox\sweep-admin.log` as an administrator (rotated once to
`sweep-admin.log.1` at 256 KiB). It names up to five offending task paths per run. A listed task
that isn't Clauseo's and hides itself from SYSTEM blocks the check: review it, and fix its
permissions or delete it with `schtasks /delete /tn "<path>" /f`. The next launch checks again
automatically. The Application event log carries counts only, never paths.

<a id="sandbox-logon-right"></a>
### If Windows won't let the Safe workspace's accounts sign in

Set-up signs each of the Safe workspace's hidden Windows accounts on locally. Windows refuses with
`ERROR_LOGON_TYPE_NOT_GRANTED` (1385) when "Allow log on locally" (`SeInteractiveLogonRight`)
doesn't include them, or when "Deny log on locally" names them. Before it makes any account,
set-up checks "Deny log on locally" for Everyone, LOCAL, INTERACTIVE, Authenticated Users, Local
account, Users and the accounts' group, `ClauseoSandboxUsers`; any of these refuses them all.

In Group Policy, under *Computer Configuration → Windows Settings → Security Settings → Local
Policies → User Rights Assignment*, add `ClauseoSandboxUsers` (or Users, which holds them too) to
"Allow log on locally", and take those groups out of "Deny log on locally".

The app says that setting up the Safe workspace again usually fixes this, and offers the set-up
again. If it fails again, the policy above is what to change.

<a id="sandbox-other-account"></a>
### If the Safe workspace belongs to another Windows account

On Windows, the Safe workspace is set up for one Windows account per computer: the one whose SID
is `OwnerSid` under `HKLM\SOFTWARE\Clauseo\Sandbox`. Clauseo for any other account on that
computer gets this reason, and the installer says so too. To move it to another account, remove
it first: `clauseo-sandbox-setup.exe --remove --per-machine` at an elevated prompt (see "Where
Clauseo's Windows protection was set up" in [section 1](#1-which-installer)), or by hand as
[Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand) says.
Then set it up again from Clauseo, signed in as the other account.

## 2. Signing

- **Windows: unsigned for the beta.** There is no code-signing certificate yet; one comes once
  there are users and funding (Rohan, 2026-11-05, in decision 0035, "Signing for the beta"). Windows
  shows "Unknown Publisher" in the download warning and the UAC prompt (seen on the test machine
  above), SmartScreen warns, and AppLocker or WDAC publisher rules cannot name Clauseo. Check the
  hash instead (below). The clicks past each warning are in [the Windows install
  guide](install/windows.md).
- **macOS: signed ad hoc for now**, with no Developer ID and not notarised (decision 0035,
  "Context"). Release builds will be notarised under an individual Developer ID, "Developer ID
  Application: Rohan Shiralkar", once it is set up (decision 0035, "Signing for the beta"). Until
  then Gatekeeper blocks the first open: [the macOS install guide](install/macos.md) has the
  clicks. There is no Team ID yet for a privacy (PPPC) profile; when there is one, it changes if
  the Apple account later moves to an organisation.
- **Check every download** against `SHA256SUMS.txt` from the same release:
  `sha256sum -c SHA256SUMS.txt --ignore-missing` on Linux or macOS (`shasum -a 256 -c` on older
  macOS), or `Get-FileHash .\Clauseo_<v>_x64-machine-setup.exe` in PowerShell and compare by eye.
  The sums come from the same release page as the file, so they show the file is the one listed
  there. The stronger check is the GitHub build provenance attestation every asset has, made by
  `.github/workflows/release.yml` (`actions/attest-build-provenance`): it shows the file was built
  by that workflow from this repository. `gh attestation verify <file> -R clauseo-ai/clauseo-acp`
  (decision 0035, "Consequences").

<a id="sandbox-policy"></a>
## 3. AppLocker and WDAC

- Allow `%PROGRAMFILES%\Clauseo\*` by **path**. Only administrators can write there, which is why
  firms with these rules need the per-machine installer: they usually block programs in
  user-writable folders such as `%LOCALAPPDATA%`.
- That rule covers Clauseo's set-up program, the admin copy Clauseo starts elevated:
  `%PROGRAMFILES%\Clauseo\Sandbox\clauseo-sandbox-setup.exe`. If AppLocker or Software
  Restriction Policies block it (`ERROR_ACCESS_DISABLED_BY_POLICY`, 1260), the Safe workspace
  can't be set up: allow that file by path.
- For Clauseo's own programs, do not use **hash** rules: every release changes every hash.
  **Publisher** rules cannot name Clauseo while it is unsigned (section 2).
- Coming (stage D): Clauseo will download Node and the agents into each user's data folder,
  `%LOCALAPPDATA%\ai.clauseo.desktop\`, which default AppLocker rules block. Don't allow that
  folder by path: the user can write there. Instead, in order of preference:
  - **publisher** rules for the signed runtime and tools: Node's `node.exe` is signed by the
    OpenJS Foundation, Codex's `codex.exe` by OpenAI (Check on the laptop pass: both
    publishers). Use **hash** rules for anything unsigned, such as `rg.exe`; those hashes
    change each time Clauseo downloads a new version;
  - or the IT-installed runtime under Program Files once its policy key lands (`install.bundle`,
    section 6, "Coming");
  - only as a last resort, a path rule for the user's `agents` folder. Anything the user can
    write there could then run. Scope it to `agents\runtime\` and `agents\<agent>\install\`
    in the data folder, never the whole data folder.
- Coming (stage C): on Windows, Clauseo's sandbox runs each agent as a separate local user
  (task S15). A policy that forbids local accounts leaves agents running without it, shown in the
  app as "Not protected".

### Windows in S mode

- **A device in S mode can't run Clauseo.** S mode runs only Microsoft Store apps; anything else
  gets "this mode of Windows only runs Microsoft-verified apps". That blocks Clauseo's installer,
  and the Node runtime and agent tools Clauseo downloads and runs, so a Store build would not help
  either. Intune's S mode supplemental policy for Win32 apps needs a signed app catalog, which
  unsigned Clauseo and its downloaded tools can't have
  ([Enable Win32 apps on S mode devices](https://learn.microsoft.com/en-us/intune/app-management/deployment/enable-win32-s-mode)).
- **The user can switch out alone, unless your policy blocks it.** Settings > System > Activation
  (Windows 11), then the Store's "Switch out of S mode" page; [the lawyer's
  guide](install/windows-s-mode.md) has the clicks. Microsoft: "Your organization can choose to keep
  all devices in S mode"
  ([S mode FAQ](https://support.microsoft.com/en-us/windows/experience/platform-variants/windows-10-and-windows-11-in-s-mode-faq)).
  That is the WindowsLicensing CSP's `SMode/SwitchingPolicy` set to `1`, "User Blocked", or Intune's
  **Switch out of S mode** set to **Keep in S mode**. The switch also goes through the Microsoft
  Store, so a policy that turns the Store off can stop it too.
- **To switch managed devices with Intune:** Devices > Manage devices > Configuration > Create >
  New policy, platform **Windows 10 and later**, profile type **Templates > Edition upgrade and mode
  switch**; under Mode switch, set **Switch out of S mode** to **Switch**
  ([Upgrade Windows editions or switch out of S mode](https://learn.microsoft.com/en-us/intune/device-configuration/templates/configure-edition-upgrade-windows),
  [the settings](https://learn.microsoft.com/en-us/intune/device-configuration/templates/ref-edition-upgrade-settings-windows)).
  It applies at the next check-in. Other MDMs can run the CSP node
  `./Vendor/MSFT/WindowsLicensing/SMode/SwitchFromSMode` (Exec; it does not reboot), which works
  even when `SwitchingPolicy` blocks the user
  ([WindowsLicensing CSP](https://learn.microsoft.com/en-us/windows/client-management/mdm/windowslicensing-csp)).
- **Switching is one-way.** The device can't go back to S mode, and removing the Intune assignment
  later doesn't revert it (both pages above).

## 4. WebView2

Clauseo draws its window with Microsoft Edge WebView2, Evergreen.

- **Install the Evergreen runtime per machine in your image.** If it is missing, the per-user
  installer fetches it silently, per user (seen on the test machine: about 50 s, no window). It
  installs a per-user Edge Update too (`%LOCALAPPDATA%\Microsoft\EdgeUpdate`, with two scheduled
  tasks `MicrosoftEdgeUpdateTaskUser<SID>Core` and `…UA`), which keeps that runtime updated. On
  non-persistent VDI a per-user runtime is gone at sign-out. Microsoft's guide:
  [WebView2 for enterprises](https://learn.microsoft.com/microsoft-edge/webview2/concepts/enterprise).
- **Minimum version:** 111.0.1661.41, the oldest runtime Clauseo works with (`minimumWebview2Version` in
  `crates/desktop/tauri.conf.json`). Below it, or with no runtime, Clauseo shows a dialog
  ("Clauseo needs Microsoft Edge WebView2 to open", "Download WebView2") instead of a window, and
  exits (decision 0035, "PR 2"). An installed runtime below it is not updated by the per-user
  installer, since Edge Update's update would ask for an administrator's password: the
  installer notes it in its details list and goes on. The per-machine installer updates it, and
  goes on with a details line if the update fails.
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
- **Keep Clauseo's data folder out of sync roots and roaming profiles** (OneDrive, iCloud Drive,
  Dropbox, a roaming profile): it holds Codex's login, and on Windows Claude's, as plain files, e.g.
  Codex's `agents\codex-acp\home\auth.json` in it. By default the folder is in none of them; the risk is
  a folder redirection, or `CLAUSEO_DATA_DIR`, putting it there
  ([decision 0048](decisions/0048-signin-on-the-wire.md), "Consequences"). Section 5's persistent
  profile container is a different thing: keep the folder there.
- **The host log:** `logs\host.log` in the data folder (on a Mac,
  `~/Library/Application Support/ai.clauseo.desktop/logs/host.log`). At 5 MB it rolls over to
  `host.1.log`, then `host.2.log`: three files, 15 MB at most. It holds Clauseo's own warnings and
  errors, and its own lines when it starts and stops, when Claude or Codex starts and ends, and
  when a matter opens or closes, by id and code only: no
  matter content and no file paths or names. It stays on the machine and is never uploaded. On
  Windows the uninstaller removes it with the rest of the uninstalling user's data folder when
  the "Also delete conversations, saved versions and settings" box is ticked; on a Mac it stays
  with the data folder ([decision 0047](decisions/0047-host-log-file.md)).

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

**The policy folder on Windows.** `C:\ProgramData\Clauseo` must be owned by Administrators and
not writable by Users. Any user can create a folder under `ProgramData`, so check one that is
already there. The per-machine installer creates the folder that way (owner
Administrators, nothing inherited, SYSTEM and Administrators full control, Users read) and
leaves a folder that is already there as it is, saying so in its details list. The uninstaller
leaves the folder, with any policy in it. After a per-user install, or when the folder was
already there, create or fix it from an elevated Command Prompt:

```bat
mkdir "%ProgramData%\Clauseo"
icacls "%ProgramData%\Clauseo" /setowner "*S-1-5-32-544"
icacls "%ProgramData%\Clauseo" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX"
```

or from an elevated PowerShell, as an Intune script runs (each grant is quoted, since PowerShell
reads `(OI)` itself otherwise):

```powershell
New-Item -ItemType Directory -Force "$env:ProgramData\Clauseo" | Out-Null
icacls "$env:ProgramData\Clauseo" /setowner "*S-1-5-32-544"
icacls "$env:ProgramData\Clauseo" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX"
```

(`mkdir` says the folder exists when it does; carry on with the rest.) Delete anything in an
existing folder that your IT team didn't put there. The SIDs are SYSTEM,
Administrators and Users, so the commands work in any Windows language. Check it with
`icacls "%ProgramData%\Clauseo"` (PowerShell: `icacls "$env:ProgramData\Clauseo"`): it lists only `NT AUTHORITY\SYSTEM:(OI)(CI)(F)`,
`BUILTIN\Administrators:(OI)(CI)(F)` and `BUILTIN\Users:(OI)(CI)(RX)`, none marked `(I)`
(the names are in the system's language). Remove any other entry with
`icacls "%ProgramData%\Clauseo" /remove:g <name>`. `dir /q "%ProgramData%"` shows the owner
beside `Clauseo`: `BUILTIN\Administrators`. Then put `policy.json` in it from the same elevated
prompt.

**Who must own `policy.json`.** The file and its folder must be owned by Administrators/SYSTEM
(root on macOS and Linux) and not writable by users. A file that fails is ignored, as if it were
not there, and the host log says why ([decision 0009](decisions/0009-policy-layer.md)).

- **Windows:** `C:\ProgramData\Clauseo` must be owned by Administrators, with a protected ACL
  (SYSTEM and Administrators full control, Users read). A per-machine install creates it that
  way; the commands to create or repair it by hand are in the section "The policy folder on
  Windows". A folder a user created first (or an empty one they swapped in) is theirs, and the
  firm's file in it is ignored. The file must have one name (no hard link) and must stay readable
  by Users, which the folder's ACL gives it: a file Users cannot read is ignored, since a user
  could plant one. Its owner must be Administrators or SYSTEM; a file written by the built-in
  Administrator account, or with UAC off, can be owned by that account's own SID and is then
  ignored, so set its owner to Administrators.
- **macOS:** deploy policy through device management (managed preferences, the preferred route),
  or create the folder as root before any user does:
  `sudo mkdir -p "/Library/Application Support/Clauseo"`,
  `sudo chown root:wheel "/Library/Application Support/Clauseo"`,
  `sudo chmod 755 "/Library/Application Support/Clauseo"`. An admin user can create that folder
  without a password, and a folder they create is not root's, so the file in it would be
  ignored. ACLs added with `chmod +a` are not seen by the check.
- **Linux:** `/etc/clauseo` as root, mode 755, file 644, as usual. On NixOS, files under `/etc`
  are symlinks into the store, so a policy deployed that way is ignored.
  To enforce the policy, restrict unprivileged user namespaces
  (`kernel.unprivileged_userns_clone=0`, or AppArmor's userns restriction, on by default in
  Ubuntu 24.04): inside one (`unshare -r`) a user's Clauseo sees the policy as ignored or absent.
- **Every OS:** the policy binds the Clauseo builds you deploy. Clauseo is open source, so a user
  who can run their own build is not bound by it; enforce which builds run with application
  allowlisting (MDM, WDAC, Santa).

**What a policy value does.** It replaces the user's value and shows as locked in the app: Settings
shows the policy's value with its choices greyed out and the line "Managed by your organisation"
(measured on Windows Server 2025, 2026-11-03, with `ui.theme` = `dark`). A value
of the wrong type for a known key is a warning and **still locks the key, at its default**. A
source that cannot be read at all (a `policy.json` that does not parse, a registry read that
fails) is skipped and **locks each security key off**: `mcp.servers` empty, `tools.web_search`,
`tools.browser`, `tools.connectors`, `privacy.counts` and `privacy.crash_reports` false, `install.bundle` and
`install.registry` unset, `skills.extra` `off`, `skills.allow_scripts` false, `skills.curated`
empty, `update.mode` `none`, no `plans.<id>` at all, and the "Coming" keys below at their
safe values (`sandbox.required` and `sandbox.allow_unprotected_auto` false, which this release
ignores: the Safe workspace comes after launch; each `network` key unset, empty or false, but
`network.unsafe_site_list_updates` true). The two `privacy` keys can
only be turned off by policy: a policy `true` for one is ignored with a warning, and the lawyer
keeps their own switch. A key this version does not know is only a warning, so one profile can
serve mixed versions, **unless its name looks like a security section's** (a section holding a
key marked **Security key** below). A policy key that starts with or contains something like a
security section's name locks that section's security keys off, whatever its punctuation or
case: `tool.web_search`, `skill_extra`, `SkillExtra`, `skill:extra`, `ToolBrowser`,
`clauseo.skills.extra`, `{"clauseo": {"skills": …}}` (through lists too), even `foo.tools_note`,
`toolbar` and `skillful`. Any word of the name counts, not just its first. "Like" means one
letter off for `mcp` (`mpc`, not `app`) and two for the longer sections, a swap of two letters
counting as one. Invisible characters such as a zero-width space are ignored, full-width letters
read as plain ones, and a top-level key with any other non-ASCII letter (`ѕkіlls` with
Cyrillic letters, or a Hindi `टिप्पणी`) counts as like every security section, as does one
directly inside an unknown section or inside `ui`, `matters` or `agents`; deeper keys, such as
`{"_comment": {"note": {"café": 1}}}`, are only compared by their letters. A key inside a known section counts
too: `{"ui": {"tools": …}}` locks `tools`, and `privacy.mcp` locks `mcp.servers`. Clauseo cannot tell what such a key meant, so it fails closed. Keep policy key names in plain
English and away from the section names; a key with other letters at the top level locks every
security setting. Other names that lock, for example: `clauseo` locks `plans.<id>`, `cpu_limit`,
`map` and `acp` lock `mcp.servers`, and `instance_id` locks `install.*`. This holds for managed
preferences too: Clauseo reads every forced key in the `ai.clauseo.desktop` domain, not only the
ones in the table, so a misspelt key or a `skills` dictionary there locks the section; a
managed preferences file Clauseo cannot read or parse locks every security key off. A key
written twice anywhere in `policy.json` makes that file count as malformed, and every security
key locks off; so does a number too large to read (`1e400`) or nesting deeper than 128 levels,
which would hide a duplicate behind it. A registry JSON value (`mcp.servers`, `plans.<id>`)
holding a key twice locks that key's section off (`mcp.servers` empty, no plan), and the rest of
the registry still applies. Either way the host log names the cause, the source (`policy.json`,
the registry or managed preferences) and the section, never a value or a path. (`Leaf::parse`, `refuse` and `SECURITY_KEYS` in
[`crates/settings/src/leaf.rs`](../crates/settings/src/leaf.rs); `lock_security_keys` in
[`crates/settings/src/effective.rs`](../crates/settings/src/effective.rs).)

**Formats.**

- **Registry:** `REG_DWORD` for a number, or a boolean as 0 or 1; `REG_SZ` for a string, or JSON
  text for an object (`agents.<id>`, `plans.<id>`, `mcp.servers`); `REG_MULTI_SZ` for a list of strings
  (`skills.curated`). Value names ignore case; an agent id keeps
  its case ([`crates/settings/src/policy/registry.rs`](../crates/settings/src/policy/registry.rs)).
- **`policy.json`:** the shape of the user's `settings.json`: sections `matters`, `agents`, `mcp`,
  `ui`, `tools`, `privacy`, `plans`, `install`, `skills`, `update`, `sandbox` and `network`, each an object; comments and trailing commas are allowed. Sample:
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
| `mcp.servers` | object: server name (1 to 64 of `A-Z a-z 0-9 _ -`, never `__` and not ending in `_`) to `command`, `args`, `env`, `enabled` (default false) | empty | `REG_SZ`, JSON | `{"firm-docs": {"command": "C:\\Program Files\\Firm Tools\\firm-docs-mcp.exe", "enabled": true}}`. **Security key.** Replaces the user's whole list; the names `browser` and `case-law` are reserved for Clauseo's own servers, and an entry of either name, in any case, or of a name holding `__` or ending in `_` (Claude would read `case-law_` as `case-law`), is left out with a warning |
| `ui.theme` | `system`, `light`, `dark` | `system` | `REG_SZ` | `dark` |
| `ui.font_size` | whole number, 14 to 20 (points) | 16 | `REG_DWORD` | 18 |
| `ui.density` | `comfortable`, `compact` | `comfortable` | `REG_SZ` | `compact` |
| `ui.language` | `en`, `hi` | `en` | `REG_SZ` | `en` |
| `ui.lite` | `automatic`, `on`, `off` | `automatic` | `REG_SZ` | `on` (RDS hosts without a GPU) |
| `ui.motion` | `system`, `reduce`, `full` | `system` | `REG_SZ` | `reduce` |
| `tools.web_search` | boolean | true | `REG_DWORD` 0 or 1 | 0: agents may not search the web or read web pages. **Security key** |
| `tools.browser` | boolean | true on macOS and Windows; false on Linux | `REG_DWORD` 0 or 1 | 0: agents do not get Clauseo's browser. **Security key** |
| `tools.connectors` | boolean | true | `REG_DWORD` 0 or 1 | 0: no data connector runs (the paid Indian case-law connector): it is not offered, and agents do not get it. On, nothing runs until a lawyer adds a connector. Amounts the app shows for it include GST; the GST breakdown is on the connector's receipt. **Security key** |
| `privacy.counts` | boolean | true | `REG_DWORD` 0 or 1 | 0: Clauseo keeps and sends no anonymous usage counts, and deletes what it kept. Policy can only turn it off (1 is ignored with a warning). **Security key** |
| `privacy.crash_reports` | boolean | false | `REG_DWORD` 0 or 1 | 0: Clauseo sends no crash reports. Policy can only turn it off (1 is ignored with a warning). **Security key** |
| `plans.<id>` | object: `endpoint` (`https` URL with a host; no user name, password, query or fragment), `provider` (`anthropic`, `bedrock`, `vertex`), `env` (map) | none: the plan login | `REG_SZ`, JSON | value `plans.claude-acp` = `{"endpoint": "https://ai-gateway.firm.example/anthropic", "provider": "anthropic"}`. **Security key**: replaces the user's whole plan for that agent; an unreadable source leaves no plan at all. An `env` name holding `KEY`, `TOKEN`, `SECRET` or `PASSWORD` (any case) is dropped with a warning: keep secrets in your own tools (an AWS profile, Google's application default credentials) |
| `install.bundle` | absolute path | none: download | `REG_SZ` | `D:\Clauseo\bundle`: an offline bundle folder. **Security key**: unset if a source is unreadable |
| `install.registry` | `https` URL with a host; no user name, password, query or fragment | none: the public hosts | `REG_SZ` | `https://npm.firm.example/`: a firm npm mirror. **Security key**: unset if a source is unreadable |
| `skills.extra` | `off`, `curated`, `any` | `off` | `REG_SZ` | `curated`: each launch loads only the skill packs in `skills.curated`, and each still needs turning on in `skills.enabled` (decision record [0040](decisions/0040-skill-packs.md)); `off` loads none beyond Clauseo's own. **Security key**: `off` if a source is unreadable or the value does not type |
| `skills.curated` | list of pack ids (2 to 30 of `a-z 0-9`, single hyphens) | empty | `REG_MULTI_SZ` | `nda-kit` and `lease-kit`, one per line: the packs `curated` admits. Policy only: a `settings.json` value is ignored with a warning. **Security key**: empty if a source is unreadable or any entry is no pack id |
| `skills.allow_scripts` | boolean | false | `REG_DWORD` 0 or 1 | 0: a pack holding a script or program is not loaded. With 1 such files are copied, never run by Clauseo, though the agent can run them. **Security key**: `false` if a source is unreadable or the value does not type |
| `skills.enabled` | object: pack id to `sha256:<64 lowercase hex>`, the hash the pack had when turned on | empty | `REG_SZ`, JSON | `{"nda-kit": "sha256:a1b2…"}` (all 64 digits). A pack whose files no longer hash to its value is not loaded. Replaces the user's whole map and locks it. Until turning a pack on from the app ships (0040, PR-E), this is the map in the user's `settings.json` |
| `update.mode` | `auto`, `notify`, `none` | `auto` | `REG_SZ` | `none`: Clauseo never checks for an update or installs one, and shows that your organisation manages updates. `auto` installs a downloaded update when Clauseo next opens; `notify` waits for the user. **Security key**: locks at `none` if a source is unreadable or the value does not type. A policy name this version doesn't know turns updates off until Clauseo is updated by hand; add new update.* policies only after the matching version is rolled out. Per-machine installs, and Macs where the user cannot write the `.app` itself or the folder it sits in, never update themselves anyway. Updates run only once release builds carry the update endpoint (R23 PR 6); until then every build shows updates as managed |
| `browser.allow_when_remote_debugging_disabled` | boolean | false | `REG_DWORD` 0 or 1 | 1: Clauseo's browser may start on a computer where your firm turned off Chrome's, Edge's or Chromium's remote debugging, or where Clauseo can't read that policy ([below](#browser-automation-and-remotedebuggingallowed)). Policy only: a `settings.json` value is ignored with a warning. **Security key**: false if a source is unreadable |

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

Keys Clauseo already reads and locks, with the safe value an unreadable source locks each at,
but that nothing acts on yet; the samples leave them out. Each moves into the table above in the
pull request that makes Clauseo act on it. Same formats as above ([decision
0051](decisions/0051-sandbox-in-host.md)).

| Key | Task | Note |
|---|---|---|
| <a id="sandbox-required"></a>`sandbox.required` | R17 | **Ignored in this release** (the Safe workspace comes after launch); Clauseo logs once that it is set but ignored. Boolean, default false, `REG_DWORD` 0 or 1. 1: on a computer that can't give the sandbox, no agent starts (`sandbox.required`). **Security key**: false if a source is unreadable; `sandbox.allow_unprotected_auto` carries the fail-closed |
| `sandbox.allow_unprotected_auto` | R17 | **Ignored in this release** (the Safe workspace comes after launch); Clauseo logs once that it is set but ignored. Boolean, default true, `REG_DWORD` 0 or 1. 0: outside the Safe workspace Clauseo doesn't use an automatic mode: Claude asks before each change, and ChatGPT asks before changing files outside the matter's folder. **Security key**: false if a source is unreadable or the value does not type |
| `network.upstream_proxy` | R17 | `http` or `https` URL with a host; no user name, password, query or fragment (Clauseo stores no proxy password). Default none: the proxy the OS is set to. `REG_SZ`. **Security key**: unset if a source is unreadable |
| `network.ca_certs` | R17 | Object: agent id to the absolute path of a CA file, trusted besides the system's. Default empty. `REG_SZ`, JSON: `{"claude-acp": "C:\\ProgramData\\Firm\\ca.pem"}`. Replaces the user's whole map; a bad entry is left out with a warning. **Security key**: empty if a source is unreadable |
| `network.allowed_domains` | R17 | List of host names (`*.firm.example` for every name under it), added to every matter's allowlist. Default empty. `REG_MULTI_SZ`. Policy only: a `settings.json` value is ignored with a warning; a bad entry is left out with a warning. **Security key**: empty if a source is unreadable |
| <a id="unsafe-site-override"></a>`network.allow_unsafe_site_override` | R17 | Boolean, default true, `REG_DWORD` 0 or 1. 0: the lawyer can't allow a site the block list names, and sites already allowed stop being honoured (they are kept) (decision 0053); the lawyer may also switch it off for themselves. A change in the policy file takes effect when Clauseo next starts. **Security key**: false if a source is unreadable or a policy value does not type |
| `network.unsafe_site_list_updates` | R17 | Boolean, default true, `REG_DWORD` 0 or 1. 0: Clauseo never fetches block list updates and keeps the stored or bundled list (decision 0053). A change in the policy file takes effect when Clauseo next starts. Policy only: a `settings.json` value is ignored with a warning. **Security key**, locked at true (its default) if a source is unreadable or a policy value does not type: an unreadable policy must not stop protection updates |

### Browser automation and RemoteDebuggingAllowed

Clauseo's browser is agent-browser, a command the agents run, which drives the computer's installed
Google Chrome (on Windows, Microsoft Edge when no Chrome is installed) through Chrome's remote
debugging, in a fresh profile for each session. With neither installed, the agents get no browser
and the app tells the lawyer what to install; a browser installed later reaches the agents once
Clauseo is reopened. The browser doesn't go through Clauseo's egress proxy, so
`network.allowed_domains` and the unsafe-site list don't apply to it; Chrome or Edge uses its own
network settings.

Chrome's, Edge's and Chromium's `RemoteDebuggingAllowed` policy turns remote debugging off.
Clauseo checks your Chrome, Edge and Chromium policy itself, before it starts any browser,
whichever browser it would pick.

**What Clauseo reads.** The value `RemoteDebuggingAllowed`, for both the computer and the user,
in Chrome's, Edge's and Chromium's policy locations. It takes the same values Chrome does:

- **Windows:** `SOFTWARE\Policies\Google\Chrome`, `SOFTWARE\Policies\Microsoft\Edge` and
  `SOFTWARE\Policies\Chromium`, each under both `HKLM` and `HKCU`. A `REG_DWORD` of 0 is false
  and any other number true; a `REG_SZ` holding a whole number counts the same way, so `"0"`
  (spaces around it allowed) is false too.
- **macOS:** the managed preferences files `/Library/Managed Preferences/com.google.Chrome.plist`,
  `com.microsoft.Edge.plist` and `org.chromium.Chromium.plist`, plus the same files under
  `/Library/Managed Preferences/<user>/` for the signed-in user. The value is a boolean, or the
  integer 0 (false) or 1 (true).
- **Linux:** every `*.json` file in `/etc/opt/chrome/policies/managed/`,
  `/etc/opt/edge/policies/managed/` and `/etc/chromium/policies/managed/`; other files are
  skipped, as Chrome skips them. The value is a boolean, or the integer 0 (false) or 1 (true).
  Clauseo reads at most 256 `.json` files in each folder and 16 MiB in each file; a folder or
  file past either limit counts as one it can't read (below). Clauseo skips `recommended/`,
  because Chrome doesn't take this policy as a recommendation either.

**What Clauseo doesn't read.** Brave's policy, and Chrome or Edge policy pushed from the cloud
(Chrome Browser Cloud Management, Edge management service) that is not written to the registry,
a managed preferences file or a policy folder. To have Clauseo honour such a policy, deploy
`RemoteDebuggingAllowed` through one of the places above too.

**The rule.** Each place says one of four things:

- **False:** Clauseo's browser doesn't start, and the app tells the lawyer that your firm doesn't
  allow apps to control a browser on this computer.
- **True, or nothing set** (the key, value, file or folder doesn't exist): no block.
- **A value Chrome wouldn't accept** (a `REG_BINARY`, a string that isn't a whole number, a
  plist or JSON file that doesn't parse, any other number than 0 or 1 in a plist or JSON file):
  ignored, as Chrome ignores it. No block.
- **A place Clauseo can't read** (permission denied or any other read failure, or past a limit
  above): Clauseo doesn't know what it says, so its browser doesn't start, and the app tells the
  lawyer Clauseo couldn't check this computer's browser settings. A false elsewhere still wins:
  the lawyer then hears that your firm doesn't allow it. The lawyer can try again from that
  message: Clauseo reads the policy then, and again each time a session starts its browser. A
  session already running keeps going without a browser; the next one gets it once nothing blocks.

Clauseo writes one line to the host log for each place it couldn't read and each value it
ignored, naming the browser and scope (never a path), once each time Clauseo starts. It reads
this policy again each time a session starts its browser, so a change there, or a place that
reads again, applies to the next session; its own key below, like every Clauseo policy key,
applies on the next start.

**Allowing Clauseo specifically.** Set Clauseo's policy key
`browser.allow_when_remote_debugging_disabled` to true. Clauseo's browser then starts despite
`RemoteDebuggingAllowed` false, or a place Clauseo can't read, and Chrome, Edge and Chromium
themselves stay as your policy sets them. A
user's `settings.json` can't set this key; only policy can. Set it the same way as any Clauseo
policy key (above):

- **Windows:** `REG_DWORD` value `browser.allow_when_remote_debugging_disabled` = 1 under
  `HKLM\SOFTWARE\Policies\Clauseo\Clauseo`, or `{"browser": {"allow_when_remote_debugging_disabled":
  true}}` in `C:\ProgramData\Clauseo\policy.json`.
- **macOS:** the key `browser.allow_when_remote_debugging_disabled`, boolean true, in the
  `ai.clauseo.desktop` profile, or the same JSON in
  `/Library/Application Support/Clauseo/policy.json`.
- **Linux:** the same JSON in `/etc/clauseo/policy.json`.

## 7. Hosts Clauseo reaches

For firewalls and proxies. Clauseo's own window talks to no host: its content security policy
allows only the app's own content and its bridge (`app.security.csp` in
`crates/desktop/tauri.conf.json`).

| Host | Why | When |
|---|---|---|
| `github.com`, then `release-assets.githubusercontent.com` | Downloads of the installers (GitHub Releases, decision 0035) | When someone downloads an installer. A release asset URL on `github.com` answers 302 to `release-assets.githubusercontent.com` (`curl -sI`, 2026-11-03). The app itself does not reach them yet: see the updater under "Coming" |
| `go.microsoft.com` (`/fwlink/p/?LinkId=2124703`), then `msedge.sf.dl.delivery.mp.microsoft.com` | WebView2 bootstrapper | Only when the runtime is missing at install, or when a user presses "Download WebView2". The fwlink answers 301 to that host (`curl -sI`, 2026-11-03). The bootstrapper then fetches the runtime through Edge Update from `config.edge.skype.com`, `msedge.api.cdp.microsoft.com` and `msedge.f.tlu.dl.delivery.mp.microsoft.com` (DNS-Client log, measured on Windows Server 2025, 2026-11-03; the same three per user and per machine). A runtime in your image avoids them all |
| Edge Update's hosts (`config.edge.skype.com`, `msedge.api.cdp.microsoft.com`, `msedge.f.tlu.dl.delivery.mp.microsoft.com` seen for installs) | WebView2 runtime updates | Microsoft's, as for Edge |
| `api.anthropic.com`, `claude.ai`, `platform.claude.com` | Claude Code, signed in with a Claude plan | While the agent works. An API key needs only `api.anthropic.com` |
| `api.openai.com`, `chatgpt.com`, `auth.openai.com` | Codex, signed in with ChatGPT | While the agent works. An API key needs only `api.openai.com` |
| Sites the agent searches and opens | Web search and Clauseo's browser | Only while `tools.web_search` or `tools.browser` is on |
| `github.com` (`/clauseo-ai/blocklist/releases/download/current/blocklist-v1.txt.gz` and `blocklist-v1.txt.gz.minisig`), then `release-assets.githubusercontent.com` | Clauseo's signed unsafe-site list: phishing domains and known botnet C2 addresses, which the egress proxy refuses ([decision 0053](decisions/0053-blocklist.md)). The app checks its signature and never applies an older list | Once at start, just after the stored or bundled copy is read, then every 6 hours (±30 minutes; 10 minutes after a signature that does not match, once). Clauseo fetches it itself, outside the agent's sandbox and never through the egress proxy, through `network.upstream_proxy` when it is set, otherwise `HTTPS_PROXY` (and `NO_PROXY`), then the OS's proxy settings, PAC included, asked for each request; a proxy setting Clauseo refuses (a URL with a password, say) fails the fetch rather than going around the proxy. TLS trusts the OS's certificate store, so a firm CA installed there works; `network.ca_certs` is not used for this fetch. Redirects are followed only to `release-assets.githubusercontent.com` (where `github.com` answers 302, `curl -sI`, 2026-11-07) and `objects.githubusercontent.com`, https only. A 404 or a failed fetch keeps the list in use. Off when `network.unsafe_site_list_updates` is false (section 6; a change in the policy file takes effect when Clauseo next starts): no request at all, and the list bundled with the app or the last one fetched is still enforced |
| `data.clauseo.chat` | Clauseo's optional paid data connectors. The first, case-law, is Indian Kanoon through Clauseo's relay: for each call, the tool name and its arguments (search words, document ids) go to `data.clauseo.chat` with the lawyer's token, and that server sends searches on to Indian Kanoon. Also the sign-in, `pricing.json`, account reads and recharges | `pricing.json` when Settings opens (at most once a day, or 10 minutes after a fetch that failed); the rest only after a lawyer adds a connector. Only while `tools.connectors` is on (sign-out and remove excepted). Clauseo makes these calls itself, outside the agent's sandbox; the agent never reaches this host. The host side ships with task R24 |

Opening a downloaded installer also makes Windows check it with SmartScreen
(`nav.smartscreen.microsoft.com`, `checkappexec.microsoft.com`,
`data-edge.smartscreen.microsoft.com`). Those are Windows' hosts, not Clauseo's.

The agents' hosts are Clauseo's starting list for its egress proxy (`builtin_allowlist` in
`crates/sandbox/src/proxy/allowlist.rs`), inferred and not yet measured. The proxy chains to
your firm's proxy: it reads `HTTPS_PROXY` and `HTTP_PROXY`, then the OS's proxy settings, PAC
included, and stores no proxy password ([decision 0039](decisions/0039-egress-proxy.md),
"Upstream").

Coming:

- `github.com/clauseo-ai/clauseo-acp/releases/latest/download/latest.json`, then the release's
  installer on `github.com` and `release-assets.githubusercontent.com`: the updater, for per-user
  installs only ([decision 0037](decisions/0037-updater.md)), about once an hour. It starts once
  release builds carry the update endpoint (task R23, PR 6); until then
  `plugins.updater.endpoints` is empty in `crates/desktop/tauri.conf.json` and no build checks.
  Never for per-machine installs, nor while `update.mode` is `none` (section 6).
- `t.clauseo.chat`: usage counts and crash reports (tasks S21 to S23), only when they are on, and
  firms can force them off (`privacy.*`). HTTPS POSTs with `User-Agent: Clauseo`; they follow
  `HTTPS_PROXY`, then the OS's proxy settings, but not a PAC file yet, and never Clauseo's egress
  proxy ([decision 0036](decisions/0036-counts.md), "The desktop").
- `nodejs.org` and `registry.npmjs.org`: Node and the agents' own downloads (task S17), unless
  `install.bundle` or `install.registry` points at your copy. They, the agents' sign-ins and the
  agents themselves go through `network.upstream_proxy`, else `HTTPS_PROXY` (or `ALL_PROXY`),
  else the OS's proxy settings, PAC included, and trust the OS's certificate store and the
  agent's `network.ca_certs` file ([decision 0033](decisions/0033-agents-installs.md), item 4).
  Known limits until the sandbox's local forwarder (S16) carries the agents' traffic:
  - an agent's own traffic uses one fixed proxy, the OS's answer for its API host: a PAC
    script's fallbacks and other per-host answers, and the OS's bypass list, are not applied to
    it (installs and sign-ins are asked about per host);
  - the OS is asked at start, when the settings change and again once the answer is a minute
    old, each time within 5 s; at start or after a settings change, a lookup slower than that
    counts as direct until it answers (up to 15 s more). On the minute's refresh, a lookup that
    fails or is slow keeps the answer before it, for up to 10 minutes from the last real answer,
    while that proxy still takes a connection (Clauseo connects to it and closes, sending
    nothing, within the 5 s): off the firm's network (a VPN dropped, a PAC file on an internal
    server failing with it) it goes direct at once. One `DIRECT` while the proxy still answers
    keeps the proxy until the next refresh says `DIRECT` again;
  - after changing networks (office, home, VPN), new launches, installs and sign-ins use the new
    proxy within about 2 minutes (the sandbox keeps each OS answer for 60 s). A running agent
    keeps the proxy it started with: a tier-C agent for its whole run, while a tier-B agent goes
    through Clauseo's egress proxy, which looks the upstream up per connection. An agent that
    stops answering after a network change is restarted by ending its thread's turn or reopening
    the matter.

  Known issue: on Windows, a machine-wide `SSL_CERT_FILE` (set by Anaconda, for example) can
  still break Codex even with `CODEX_CA_CERTIFICATE` set
  ([openai/codex#55668](https://github.com/openai/codex/issues/55668)); remove it, or point it
  at a full root bundle.

## 8. Claude's personal skills

A lawyer's personal claude.ai skills don't appear in Clauseo sessions. Clauseo runs Claude Code
with its own Claude home and sets `syncClaudeAiSkills: false` there, so Claude Code neither
syncs nor loads the account's claude.ai skills. There is no switch for it until it has been
reviewed. Your firm's skills reach Clauseo as skill packs
([decision 0040](decisions/0040-skill-packs.md)).

### What the fence between matters is not

On a machine without Clauseo's sandbox (tier C), there is no isolation between matters. The
agent runs as the lawyer and can read any folder the lawyer can, including other matters'
folders on disk. What Clauseo fences is its own data: other conversations in its search index,
other sessions' working copies, other matters' notes and the agents' credentials. That is not
a wall between matters. The agent may read Clauseo's `profile` folder: it holds only the
built-in plugin, the same for every matter. Run folders and matters created after a session started are not in
that session's Claude deny rules; Clauseo's own permission check refuses them when Claude asks
([decision 0030](decisions/0030-launch-profile.md), "The data folder fence").

## 9. Windows protection: a matter folder Clauseo can't take back

With Clauseo's Windows protection (tier B), each matter's agent runs as its own local account
(`ClauseoSb01`, `ClauseoSb02`, and so on). If a matter's folder ends up owned by one of those
accounts, Clauseo refuses to open the matter and first tries to give the folder back to the
lawyer. "Set up protection again" (one Windows prompt) takes back items inside the matters
Clauseo has recorded. A folder outside all of them, such as one an agent made that was then added
as a new matter, can't be taken back that way. An administrator does it by hand:

1. Open PowerShell as administrator.
2. Give the folder, the folder only, to the lawyer:
   `icacls "D:\Matters\Example" /setowner "CONTOSO\jsmith"`
3. List its entries with `icacls "D:\Matters\Example"`. If an account whose name starts with
   `ClauseoSb` is listed, remove its entry: `icacls "D:\Matters\Example" /remove "ClauseoSb03"`
4. Open the matter in Clauseo again.

Don't add `/T` or use `takeown /r`: Clauseo hands back what is inside the folder at its next
launch, and a recursive change can alter permissions your firm set on the files.

To clear the sandbox users and the rest of the protection from a computer whose
`clauseo-sandbox-setup.exe` is gone, see
[Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand).

<a id="sandbox-tool-missing"></a>
## 10. If Windows security removed Clauseo's set-up program

Microsoft Defender can quarantine `clauseo-sandbox-setup.exe`, which sets up and removes Clauseo's
Windows protection (section 1); on our test machine, as `Trojan:Win32/Bearfoos.A!ml`, a false
positive. Without it the protection can't be set up, or removed by the uninstaller.

- **Installing:** Clauseo runs only the copy in `C:\Program Files\Clauseo\Sandbox`. If the install
  wrote that copy and it was gone when the install tried to start it, the install stays and says
  so, and exits 20 (a `/P` install writes a line in its details list and sets no code). An
  install that didn't write the copy (a declined prompt, Clauseo running, a per-user `/S`, `/P`
  or `/UPDATE` install) never says this, and a copy that couldn't be written is a failed set-up
  (23 under `/S`). A copy missing from the install folder only holds up Clauseo's next refresh of
  the admin copy, which Clauseo reports.
- **Uninstalling:** if the file is gone and someone's protection is still set up (the value
  `OwnerSid` under `HKLM\SOFTWARE\Clauseo\Sandbox`; the key alone isn't enough), the uninstaller
  says the sandbox's hidden accounts are still on the computer; with `/S` it exits 21 instead. A
  per-user uninstall says so only to the Windows user who set it up; a per-machine uninstall says
  so whoever that was. The rest of the uninstall carries on.
- **Uninstalling, the copy refused:** before it starts the copy in
  `C:\Program Files\Clauseo\Sandbox`, the uninstaller checks it as Clauseo does: the drive's root,
  `C:\Program Files`, `Clauseo`, `Sandbox` and the file owned by SYSTEM, Administrators or
  TrustedInstaller and changeable by no one else, and the file's SHA-256, which it computes
  itself (no `certutil`), the one recorded in `AdminCopySha256` under that key. If not, or if it
  can't compute the hash, it starts nothing; a per-machine `/S` uninstall exits 27. It says the
  hidden accounts are still on the computer only when someone's protection is set up, as above
  (a box unless `/S`); otherwise it only writes a line in its details list. Put the folder's
  permissions back, or run the Clauseo installer again, then uninstall again; or remove the
  protection by hand (below).

**Exit codes** of both installers and the uninstaller:

| Code | Meaning |
|---|---|
| 0 | Done |
| 1 | Cancelled by the person installing (NSIS) |
| 2 | Stopped by an error (NSIS) |
| 20 | Install: installed, but this run wrote `clauseo-sandbox-setup.exe` to `C:\Program Files\Clauseo\Sandbox` and Windows security removed it (`/S` and interactive; not `/P`) |
| 21 | `/S` uninstall: uninstalled, but the protection stays, since the set-up program was gone |
| 28 | Install: nothing usable installed, because Windows Security (or another antivirus) removed or blocked `Clauseo.exe` while it was being written; allow Clauseo in Windows Security's Protection history, then install again |

Intune reports 20 as a failed install unless you add it to the app's return codes. A per-machine
uninstall runs in place, so its `QuietUninstallString`,
`"C:\Program Files\Clauseo\uninstall.exe" /S _?=C:\Program Files\Clauseo` (`_?=` last, no quotes),
returns 21 or 27 itself; started without `_?=` it removes nothing and returns 26 (section 1). A
per-user `uninstall.exe` returns 0 at once, so it reports 21 only when run as
`"%LOCALAPPDATA%\Clauseo\uninstall.exe" /S _?=%LOCALAPPDATA%\Clauseo`, which leaves `uninstall.exe`
and the folder behind; delete them afterwards.

**Get the file back (preferred).** Never leave a Defender exclusion on a per-user install's
`%LOCALAPPDATA%\Clauseo`: it weakens protection, since anything running as the lawyer could swap
the excluded file, and Clauseo runs that file elevated. Either:

- **run the Clauseo installer again** for a fresh copy; or
- **restore the quarantined file** by its path, never by the threat name (a machine-learning name
  such as this one can match another file), and check its SHA-256 against the hash Clauseo
  publishes with each release before anything runs it elevated.

If Defender quarantines it again during the restore, exclude it only until you are done, from an
elevated PowerShell (`$f` is the file in the install folder). For a per-user install, set `$f` to
`C:\Users\<user>\AppData\Local\Clauseo\clauseo-sandbox-setup.exe` with the lawyer's user folder:
at an administrator's prompt, `%LOCALAPPDATA%` is the administrator's folder, not the lawyer's.

```powershell
$f = "C:\Program Files\Clauseo\clauseo-sandbox-setup.exe"
$mp = "$env:ProgramFiles\Windows Defender\MpCmdRun.exe"
Add-MpPreference -ExclusionPath $f        # only if Defender quarantines it again
& $mp -Restore -ListAll                   # find the item for $f
& $mp -Restore -FilePath $f
(Get-FileHash $f -Algorithm SHA256).Hash  # must equal the published hash; stop if it differs
& $f --remove --per-machine               # or run the installer again to set Clauseo up
Remove-MpPreference -ExclusionPath $f
```

`--remove --per-machine` removes the protection for every user (section 1); uninstall Clauseo
after it. Report the false positive at <https://www.microsoft.com/wdsi/filesubmission>.

**Remove the protection by hand**, when the file can't be restored: see
[Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand) in
section 1.

Then the lawyer's folders: on each matter folder, the two `icacls` commands under "Intune
(Win32 app)" in section 1. Anything else that still lists an entry for one of the SIDs the script
listed (shown as a bare `S-1-5-21-…` once the account is gone) grants nothing; remove it with
`icacls "<folder>" /remove "*<SID>"`. If Clauseo stays uninstalled, the lawyer can then delete the
`sandbox` folder in `%LOCALAPPDATA%\ai.clauseo.desktop`, after moving out anything in
`sandbox\set-aside`: those are files set aside from a matter.

Removing the protection also removes its network filters and firewall rules: see
[section 11](#11-windows-protection-the-admin-copy-its-tasks-and-its-network-rules).

## 11. Windows protection: the admin copy, its tasks and its network rules

What Clauseo's Windows protection (tier B) puts on a computer besides the sandbox users: the admin
copy of its set-up program, the registry values that describe it, two SYSTEM tasks and the network
rules for the sandbox users. To remove all of it, see section 1 (`--remove`) or, when the set-up
program is gone,
[Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand).

### The admin copy

`clauseo-sandbox-setup.exe` sets up and removes the protection, and needs administrator rights to
do it. Clauseo never runs the copy beside `Clauseo.exe`: in a per-user install, that folder is one
the lawyer's own programs can change. It runs only the admin copy:

`C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe`

- **Per-user install:** the installer writes it after the one UAC prompt at the end of an
  interactive install (section 1). A silent per-user install skips it and exits 22.
- **Per-machine install:** the installer writes it on every install and update.

The `Sandbox` folder and the file are owned by Administrators. SYSTEM and Administrators have full
control; Users can read and run the file. No one else can change either. The folder holds only the
copy, its `.sig` file and a `staging` folder that only SYSTEM can open. Don't put anything else
there.

Before each elevated start, and before each protected agent starts, Clauseo checks three things:

1. the copy is there, as a plain file, not a link;
2. the copy, `Sandbox`, `Clauseo`, `C:\Program Files` and every folder up to the drive's root are
   owned by SYSTEM, Administrators or TrustedInstaller, and no one else can change them (nothing
   else may sit in `Sandbox`);
3. the copy's SHA-256 is the one recorded in `AdminCopySha256` (below).

If a check fails, Clauseo doesn't run the copy, and Safe workspace says why. Put the folder's
permissions back, or run the Clauseo installer again. If Windows security removed the copy, see
[section 10](#10-if-windows-security-removed-clauseos-set-up-program).

### The values under `HKLM\SOFTWARE\Clauseo\Sandbox`

Clauseo's set-up program and its two tasks write these values. **Don't edit, add or delete any of
them.** Clauseo checks them before it runs anything elevated: a changed value turns the protection
off, or stops a removal from finishing. To remove them, remove the protection (section 1). The key
gives SYSTEM and Administrators full control and Users read access.

| Value | Type | What it is |
|---|---|---|
| `OwnerSid` | `REG_SZ` | The SID of the Windows user who set the protection up (the lawyer). One owner per computer |
| `GroupSid` | `REG_SZ` | The SID of the group `ClauseoSandboxUsers` |
| `PoolSids` | `REG_MULTI_SZ` | The SIDs of the sandbox users, `ClauseoSb01` to `ClauseoSb32` |
| `DataDir` | `REG_SZ` | The owner's Clauseo data folder |
| `Removing` | `REG_DWORD` | 1 once a removal has started. If it stays, a removal stopped partway; the next `--remove` finishes it |
| `ResetSids` | `REG_MULTI_SZ` | The sandbox users the stop task found running. The next set-up deletes and remakes each one's profile before it is used again |
| `StopNext` | `REG_QWORD` | The number the next run of the stop task takes (a counter) |
| `StopSequence` | `REG_QWORD` | The number of the last run of the stop task that ended 0 |
| `StopCompleted` | `REG_QWORD` | When that run's last clean check began, as a Windows `FILETIME` (UTC). For diagnosis only |
| `AdminCopySha256` | `REG_SZ` | The admin copy's SHA-256, 64 lowercase hex digits |
| `AdminCopyVersion` | `REG_SZ` | The admin copy's version, which is Clauseo's version |
| `AdminCopyOwner` | `REG_SZ` | Whose install placed the admin copy: `machine` for a per-machine install, otherwise the lawyer's SID |
| `UserInstallDir` | `REG_SZ` | The install folder the admin copy came from. The refresh task looks for updates there |

The first nine belong to the set-up. Removing Safe workspace from inside Clauseo deletes them, but
keeps the admin copy, the last four values and the tasks (on a per-machine install it deletes
`\Clauseo\SandboxStop` too), so the lawyer can set it up again without the installer. Uninstalling
deletes everything here, then the key once it is empty.

### The two SYSTEM tasks

Both tasks live in the Task Scheduler folder `\Clauseo`, run as SYSTEM, run the admin copy with
one fixed argument, and never run twice at once.

| Task | Starts | Does | Time limit |
|---|---|---|---|
| `\Clauseo\SandboxRefresh` (`--refresh`) | At the owner's sign-in, and when Clauseo starts and finds its installed set-up program differs from the admin copy (after an update) | Keeps the admin copy current after a per-user install updates itself | 5 minutes |
| `\Clauseo\SandboxStop` (`--stop`) | On demand only | Stops every process of the sandbox users | 2 minutes |

**Who can do what.** Administrators own the folder `\Clauseo` and both tasks. SYSTEM and
Administrators have full control. The owner (the lawyer) has read and execute (`FRFX`): they can
start a task and end a running one, but can't change or delete it. An ended refresh keeps the old
copy; an ended stop doesn't count as a finished one. If someone other than an administrator made a
`\Clauseo` folder first, the set-up deletes it, with everything in it, and makes it again.

**Which installs have them.** A per-user install registers both. A per-machine install never
updates itself, so it has no refresh task: you update it. Its stop task is registered when the
lawyer sets up Safe workspace, and again at each set-up after that.

**The refresh task** (`--refresh`) reads `clauseo-sandbox-setup.exe` and its `.sig` file from
`UserInstallDir`, copies the program into `staging`, checks the staged copy against its update
signature (the `.sig` file) and the key built into the admin copy, and checks that it is for the
same processor and not an older version.
Only then does it replace the admin copy, and write `AdminCopyVersion` and `AdminCopySha256`. It
never writes to the install folder. Its exit codes:

| Code | Meaning |
|---|---|
| 0 | The admin copy is current: replaced, already current, or its values written |
| 32 | Not run as SYSTEM, or not run from the admin copy's path |
| 33 | The key can't be read, or lacks a usable `AdminCopyOwner`, `UserInstallDir` or `AdminCopyVersion` |
| 34 | The new program or its `.sig` was refused: missing, behind a link, not a plain file, too large or unreadable |
| 35 | The new program's update signature was refused |
| 36 | This build has no update key; nothing was read |
| 37 | The `staging` folder or the copy in it couldn't be made, read back or secured |
| 38 | The admin copy couldn't be replaced; the old copy is still in place |
| 39 | The new copy is in place, but its version or hash couldn't be written. Clauseo refuses the copy until the next run writes them |
| 40 | The new program is older than `AdminCopyVersion`, or the same version with other contents, or its version can't be read |
| 41 | `UserInstallDir` isn't inside the owner's profile (for `machine`, isn't a folder only administrators can change) |
| 42 | The new program is for another processor type |
| 43 | Something held the `Sandbox` folder open, so nothing moved |
| 44 | A set-up, `--adopt` or removal was running; nothing was read or changed |

**The stop task** (`--stop`) reads the sandbox users from `PoolSids`, and checks each is a
`ClauseoSbNN` account on this computer. It stops every process of theirs, whatever the process's
own permissions say, and cancels every BITS job they own. It adds each user it found to
`ResetSids`, and on success writes `StopCompleted` and `StopSequence`. It never stops the owner's
processes. Its exit codes:

| Code | Meaning |
|---|---|
| 0 | Every sandbox user's process was stopped (or none ran), their BITS jobs cancelled, the users marked and the success record written |
| 45 | Not run as SYSTEM, or not run from the admin copy's path; nothing was stopped |
| 46 | The key can't be read, or its `PoolSids`, `OwnerSid` or `StopNext` is malformed; nothing was stopped |
| 47 | A SID in `PoolSids` isn't a sandbox user's; nothing was stopped |
| 48 | A process or BITS job was left, or couldn't be identified (see [below](#if-the-sandboxstop-task-ends-with-48)) |
| 49 | The processes were stopped, but the users couldn't be marked in `ResetSids`, or the success record couldn't be written |
| 50 | The run couldn't take its number from `StopNext`; nothing was stopped |

Task Scheduler shows the last result in hexadecimal: 0x20 is 32, 0x30 is 48 and 0x32 is 50.

**Logs, and what to send support.** Each run writes one line to the Windows Application log,
source `Clauseo Sandbox`, event ID 1: an error when it failed, information otherwise. The refresh
task's line names no user path beyond the install folder; the stop task's gives counts, plus image
file names after a 48. Send Clauseo support the task's name, its exit code and that line, with the
Clauseo version.

**Removal.** Uninstalling Clauseo runs `clauseo-sandbox-setup.exe --remove --uninstall` (per
machine, `--remove --uninstall --per-machine`), which deletes both tasks after the four admin-copy
values and before the admin folder. Don't delete or change the tasks yourself, except in the
by-hand clean-up ([Clearing a leftover Windows sandbox by hand](#clearing-a-leftover-windows-sandbox-by-hand)).
The running set-up program can't delete itself, so a file named
`clauseo-sandbox-setup.exe.<number>.delete` can stay in `C:\Program Files\Clauseo\Sandbox` until
the next restart, when Windows deletes it; if it is still there after a restart, delete it.

### If the SandboxStop task ends with 48

The SYSTEM task `\Clauseo\SandboxStop` stops the sandbox users' processes. Exit 48 means it found
a process it couldn't identify, or one it couldn't stop. Nothing is lost: every sandbox user is
reset before it is used again. Its Application Event Log line names each such process's image
file (`unknown images: ...`), by file name only. A security product that hides its processes'
tokens from SYSTEM can cause this on every run; tell Clauseo support the file names the line
gives.

### If the SandboxSweep task ends with 56 or 57

The SYSTEM task `\Clauseo\SandboxSweep` runs at the end of every sandboxed session. It deletes the
sandbox users' scheduled tasks and cancels their BITS jobs; it stops no process. Exit 56 means a
scheduled task on the machine couldn't be read or deleted, or a Task Scheduler folder couldn't be
listed. Exit 57 means a sandbox user's BITS job couldn't be cancelled. Either way the run isn't
recorded as complete, so the next sandboxed launch asks for another run and waits up to 15 seconds
for it; if that run fails too, the launch is refused with "the sandbox's clean-up after an earlier
session isn't finished". The task's Application Event Log line gives counts only, never task names.
A task whose "run as" account is a domain account that can't be found (no domain controller is
reachable) doesn't cause 56: it is skipped and counted as "skipped" in that line. A "run as"
account that can't be found and isn't another domain's (a deleted local account, say) does, and is
counted as "unresolved"; so does a task with no "run as" account ("with no user or group"). A task
whose definition the SYSTEM account can't read is read from its file under
`C:\Windows\System32\Tasks`; it is skipped when that file names someone other than a sandbox
user, and counted as "skipped, not the pool's by their task file". The usual cause of 56 on every
run is a task that can't be read either way, or one of those. Look in Task Scheduler for tasks that won't open or whose account is gone, and fix or
delete them; the next run then ends 0. Exit 58 or 59 means the run couldn't record its number, usually because another Clauseo
set-up held its lock; the next launch asks again.

### Network rules for the sandbox users

The set-up blocks the sandbox users' network traffic with Windows Filtering Platform (WFP) filters.
The one exception is a permit, per sandbox user, to `127.0.0.1` over TCP on ports 46100 to 46199,
where Clauseo's own proxy listens for that launch. Each listener there demands a secret given to
its own launch only. In practice:

- **Agents reach the internet only through Clauseo's proxy.** It runs as part of Clauseo, as the
  signed-in lawyer, not as a sandbox user, and lets through only the hosts Clauseo allows
  (section 7).
- **Your firm's outbound firewall and proxy rules still apply**, to Clauseo's proxy rather than to
  the sandbox users. It chains to your firm's proxy (section 7). Rules that name the sandbox users
  aren't needed.
- Direct DNS (port 53) and ICMP are blocked. Windows' own services still resolve names for them,
  as the DNS Client service does for every program.

The set-up also adds four Windows Firewall rules for the same users, as a second layer:
`clauseo_sandbox_block_outbound`, `clauseo_sandbox_block_inbound`,
`clauseo_sandbox_block_loopback_udp` and `clauseo_sandbox_block_loopback_tcp`. If Group Policy keeps
local firewall rules from applying, or Windows Firewall is off, the WFP filters still enforce, and
the set-up logs it; you can add the same rules by Group Policy, or allow local rules
(`AllowLocalPolicyMerge`). If the Base Filtering Engine service (BFE) isn't running, the set-up
fails: set it to start automatically.

**How to recognise them.** All of Clauseo's WFP objects are named `Clauseo sandbox`: the provider
`{a5170842-1321-4b98-aa1e-e79c132f1c10}`, the sublayer `{91037008-b7c9-4530-b42e-287700239f22}`
and the filters in it. `netsh wfp show filters` lists them.

**Don't delete or change them.** Nothing else may sit in Clauseo's sublayer: the set-up deletes
any filter it didn't write. Before each protected agent starts, Clauseo checks that the filters
are exactly the set-up's and cover that agent's sandbox user. If they don't, the agent doesn't
start, rather than run unfiltered. That refusal is `NetworkUnfiltered` in Clauseo's logs: the
filters are missing, changed or unreadable, or the ports 46100 to 46199 are taken (by Windows'
dynamic port range, a reserved range such as Hyper-V's, or another program's listener). "Set up
protection again" in Clauseo (one Windows prompt) rewrites the filters. Removing the protection (section 1)
deletes the filters, the sublayer, the provider and the firewall rules.

<a id="sandbox-admin-copy"></a>
### If the set-up program's admin copy is missing, changed or unsafe

Before it starts `C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe`, Clauseo checks it
as the uninstaller does (above). It refuses a copy that is missing or has no recorded hash, one
whose SHA-256 isn't `AdminCopySha256`, or one in a folder that an account other than SYSTEM,
Administrators or TrustedInstaller owns or can change. Run the Clauseo installer again as an
administrator to put the copy back; for unsafe owners, put the folders' permissions back first.

## Later

Codex on Windows has no data-folder deny. Clauseo runs Codex there in Codex's unelevated
sandbox, which can't enforce a read deny, so a Codex shell on Windows can read Clauseo's data
folder, other matters' notes and the search index included. This lasts until Codex runs in the
elevated sandbox (S15) ([decision 0030](decisions/0030-launch-profile.md), "Codex's data-folder
deny").


ADMX and ADML templates, a firm bundle zip, an MSI and a `.pkg` for Jamf come when a firm asks. Claude
Code's and Codex's own managed settings are the vendors' (their business plans), not Clauseo's.
