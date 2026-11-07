MXC probe kit
=============

What it does: checks whether this Windows PC can run Microsoft Execution Containers' process
container (MXC, Clauseo's planned "tier A" sandbox on Windows) and saves the answer.

How to run
----------
1. Before unzipping: right-click mxc-probe-kit.zip > Properties > tick "Unblock" > OK.
   That stops Windows asking about each file.
2. Unzip anywhere, for example Downloads\mxc-probe-kit.
3. Double-click run-probe.cmd. Use a normal window: do NOT "Run as administrator".
4. When it ends, send back the files in the results folder.
5. Optional: double-click cleanup.cmd to undo everything (see below).

Admin rights: not needed. Run it unelevated. If the window is elevated anyway, the probe
says so at the top ("NOTE: this window is elevated"), because Clauseo runs agents unelevated
and results can differ. It also warns if started from session 0 (SSH or a service).

Time: mostly the download. It fetches MXC v1.0.1 from Microsoft's GitHub release
(github.com/microsoft/mxc/releases/download/v1.0.1/mxc-release-binaries.zip, 542 MB), which takes
1 to 5 minutes on normal Wi-Fi. If the file's SHA-256 doesn't match the pinned value
0E832239454BA7FCD338FF56218A287EEF4C0A01C1484084B869F55FEFF8ABE9, it stops and runs nothing.
The checks after that take under a minute.

What it checks:
- the Windows edition, build and update level, against MXC's minimum (Windows 11 24H2 or 25H2
  at build .9278 or later);
- whether processmodel.dll and the process-security APIs exist;
- what MXC's own probe (wxc-exec --probe) reports;
- if MXC's process container is available, it also runs cmd in it with the UI on and off. If
  node.exe is on PATH, it runs node the same way. Each run lets the program write only to a
  scratch folder, with all network access blocked.

Nothing listens on the network, nothing is installed, no agents run, and no account or
password is needed.

Where the output goes
---------------------
The results folder next to run-probe.cmd:
- probe-log-<time>.txt       everything shown on screen
- probe-summary-<time>.json  the answers, for Clauseo
- left-behind.json           what the run created (cleanup.cmd reads it)

What it leaves behind
---------------------
- %TEMP%\mxc-probe-kit: the 542 MB download, the extracted MXC programs (x64 or arm64
  folder), a scratch "matter" folder and the smoke runs' policy files.
- The results folder in the kit folder.
- Nothing else, as far as MXC v1.0.1's source shows:
  - Its process-container path keeps no registry entries or files of its own. Its registry
    tracker (HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\
    ProcessSandboxes\Mappings) exists in the code, but nothing calls it.
  - It creates AppContainer profiles (HKCU\...\CurrentVersion\AppContainer\Mappings plus a
    %LOCALAPPDATA%\Packages folder) and host-folder permission records
    (%LOCALAPPDATA%\Microsoft\MXC\dacl-restore) only in its older AppContainer fallback. The
    kit never runs anything there: on a PC that fallback would serve, the smoke runs are
    skipped.
  - The firewall filters belong to Windows for the life of each run. The kit uses no proxy and
    no loopback exemption.
  - MXC's telemetry consent file (%LOCALAPPDATA%\mxc) is written only when someone answers its
    consent prompt, which the kit never shows.
- Windows itself may keep something when it creates a container. That isn't documented, so the
  probe checks: it snapshots these before and after and lists anything new under "Left behind"
  and in left-behind.json:
  - HKCU AppContainer\Mappings and ProcessSandboxes\Mappings;
  - %LOCALAPPDATA%\Packages and the MXC folders;
  - the firewall rules;
  - the loopback exemptions.

cleanup.cmd
-----------
Removes exactly what left-behind.json lists:
- the AppContainer profiles, through Windows' DeleteAppContainerProfile;
- the ProcessSandboxes keys;
- the Packages and MXC folders;
- then %TEMP%\mxc-probe-kit.

It leaves alone any entry named like an installed app's package (with a "_" in it), in case
something else installed an app during the run. New firewall rules or loopback exemptions would
need admin to remove; it prints the command instead. It keeps the results folder; delete the
kit folder to remove that too. No admin needed.
