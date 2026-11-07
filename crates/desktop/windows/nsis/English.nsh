; Tauri's installer messages in English (bundle.windows.nsis.customLanguageFiles in
; crates/desktop/tauri.conf.json). Naming a file there replaces the bundler's own English.nsh, so
; this file holds all of Tauri's messages: Clauseo's wording first, then Tauri's stock English.
; No byte order mark: the bundler adds one when it copies the file.

; Clauseo's wording: the one line whose words differ from Tauri's (approved by Rohan, 2026-11-03),
; then the four lines windows/hooks.nsh shows after the app deletes its data (S20, 2026-11-05;
; deleteAppDataFailedUnnamed 2026-11-06), then the two the per-machine installer shows about the
; firm-policy folder (2026-11-06), and the two the uninstaller shows about S15's removal: when it
; outlasts its wait (removeStillRunning, 2026-11-07) and when a silent per-user uninstall skips it
; (removeSkippedSilent, 2026-11-07); then the two drafts about a set-up program Windows security
; removed (removeExeMissing, setupExeRemoved, 2026-11-07), the draft for an admin copy the
; uninstaller won't start (removeAdminCopyRefused, 2026-11-07), then the eleven the installer shows
; about the admin copy of S15's set-up program (adminCopy*, 2026-11-07).
; deleteAppData labels the uninstaller's box (unticked) that removes the whole data folder, the
; store included; papers stay in their matter folders. The box is one line, 400 x 25 px: keep it
; short (a test holds it to 60 characters).
LangString deleteAppData ${LANG_ENGLISH} "Also delete conversations, saved versions and settings"
; deleteAppDataFailed: something in the data folder could not be deleted (or the app was missing)
; and no kept folder is left in it; $R9 is the local data folder. deleteAppDataKept: a folder that
; stays, the sandbox's (S15's records) or the files tier B set aside; $R8 is that folder. Each path
; stands alone after a blank line. Both in a message box, skipped by /S. Provisional (app thread,
; 2026-11-05).
LangString deleteAppDataFailed ${LANG_ENGLISH} "Clauseo couldn't delete all of its data. You can delete this folder yourself.$\n$\n$R9"
LangString deleteAppDataKept ${LANG_ENGLISH} "Clauseo kept this folder because it may hold files you still need. Look through it before you delete it.$\n$\n$R8"
; deleteAppDataKeptFailed: a folder stays ($R8, as deleteAppDataKept) and something else in the
; data folder ($R9) could not be deleted: one box for both. Provisional (app thread, 2026-11-05).
LangString deleteAppDataKeptFailed ${LANG_ENGLISH} "Clauseo couldn't delete all of its data. It kept one folder because it may hold files you still need. Look through that folder before you delete anything.$\n$\nThe folder it kept:$\n$R8$\n$\nThe rest of its data:$\n$R9"
; deleteAppDataFailedUnnamed: as deleteAppDataFailed, but the data folder is itself a link (or
; could not be read), so no path is shown: one would lead through the link. The folder is named
; by its name instead. Provisional (app thread, 2026-11-06).
LangString deleteAppDataFailedUnnamed ${LANG_ENGLISH} "Clauseo couldn't delete all of its data. It's in the ai.clauseo.desktop folder in your local AppData folder. Ask whoever looks after this computer to remove it."
; policyFolderExists and policyFolderFailed: the per-machine installer's lines in its details list
; (DetailPrint, windows/hooks.nsh NSIS_HOOK_POSTINSTALL) about the firm-policy folder;
; $APPDATA is C:\ProgramData there. Exists: something was already at that path, left as it
; is. Failed: the folder could not be created. Provisional (stage F, 2026-11-06).
LangString policyFolderExists ${LANG_ENGLISH} "Clauseo left the existing folder $APPDATA\Clauseo as it is. A firm policy there is read only if administrators own the folder."
LangString policyFolderFailed ${LANG_ENGLISH} "Clauseo couldn't create $APPDATA\Clauseo for a firm policy. Your IT team can create it."
; removeStillRunning: S15's removal of the Safe workspace (clauseo-sandbox-setup.exe --remove)
; was still running when the uninstaller stopped waiting for it, after 2 minutes; it carries on
; and finishes by itself. In the uninstaller's details list (DetailPrint), never a box.
; Provisional (stage F, 2026-11-07).
LangString removeStillRunning ${LANG_ENGLISH} "Clauseo is still removing the Safe workspace. It finishes on its own."
; removeSkippedSilent: a silent per-user uninstall (/S) can't ask for the UAC prompt that removing
; the Safe workspace needs, so it skips the removal and keeps the sandbox folder. In the
; uninstaller's details list (DetailPrint), never a box. Provisional (stage F, 2026-11-07).
LangString removeSkippedSilent ${LANG_ENGLISH} "Clauseo didn't remove the Safe workspace, because this uninstall is silent. To remove it, reinstall Clauseo and uninstall it without /S."
; removeExeMissing: the uninstaller found S15's set-up program (clauseo-sandbox-setup.exe) gone,
; most likely quarantined by Windows security, while the machine's record shows a set-up, so it
; couldn't run --remove. A box; under /S a details line and exit code 21. setupExeRemoved: an
; installer found that program gone right after writing it; the install stays. A box; under /S a
; details line and exit code 20. Provisional (app thread, stage C's fact check, 2026-11-07).
LangString removeExeMissing ${LANG_ENGLISH} "Clauseo couldn't remove the Safe workspace, because Windows security removed the program that sets it up. The Safe workspace's hidden Windows accounts are still on this computer. Whoever looks after this computer can remove them. Clauseo's notes for IT explain how."
; removeAdminCopyRefused: the uninstaller found S15's set-up program in Program Files, but a
; folder on its way or the file itself can be changed by more than administrators, or its SHA-256
; isn't the one the machine's record holds (or either couldn't be read), so it didn't start it.
; A box; under /S a details line and exit code 27. Draft, for the app thread (2026-11-07).
LangString removeAdminCopyRefused ${LANG_ENGLISH} "Clauseo couldn't remove the Safe workspace, because it couldn't confirm that the program that sets it up, in Program Files, is the one Clauseo installed and that only administrators can change it. The Safe workspace's hidden Windows accounts are still on this computer. Whoever looks after this computer can remove them. Clauseo's notes for IT explain how."
LangString setupExeRemoved ${LANG_ENGLISH} "Windows security removed part of Clauseo while it was installing: the program that sets up the Safe workspace. Until it's restored, Clauseo won't start Claude or ChatGPT in the Safe workspace. Whoever looks after this computer can restore it. Clauseo's notes for IT explain how."
; The admin copy (windows/admin_copy.nsh): tier B, which the app calls Safe workspace (and so do
; these lines), needs one UAC prompt at install. The installer is English only (0035), so these
; have no Hindi. Provisional (app thread and stage F, 2026-11-07).
; adminCopyAsk: a box just before the prompt, saying it is coming and why; the prompt may name the
; publisher "Unknown Publisher" while the installer is unsigned.
; adminCopyWorking and adminCopyDone: details-list lines while the set-up runs (it can take a few
; minutes) and once it is done.
; adminCopyDeclined: the prompt was declined; for a standard user, whoever looks after the
; computer types their password into it. adminCopyFailed: the set-up ran and failed (or couldn't
; start). adminCopyOtherAccount: another Windows account on this computer already has tier B.
; Each a box and a details line, per-user installs.
; adminCopyMachineFailed: the per-machine installer's set-up failed; a box interactively, a
; details line always. adminCopySkippedSilent: a silent per-user install (/S) can't ask for the
; prompt; details list only. adminCopyNewerKept: the copy already on the computer is from a newer
; Clauseo than this installer, so it stays; details list only. adminCopyAppRunning: Clauseo was
; still running, so Safe workspace wasn't set up (a box and a details line; exit code 24).
; adminCopyStillRunning: the set-up outlasted the installer's 10-minute wait and finishes by
; itself; details list only.
LangString adminCopyAsk ${LANG_ENGLISH} "Next, Windows asks for permission to set up Safe workspace. Windows may name the publisher as Unknown Publisher. That's expected: it's this installer asking."
LangString adminCopyWorking ${LANG_ENGLISH} "Setting up Safe workspace. This can take a few minutes."
LangString adminCopyDone ${LANG_ENGLISH} "Safe workspace is set up."
LangString adminCopyDeclined ${LANG_ENGLISH} "Clauseo is installed, but Safe workspace isn't set up, because Windows' permission prompt was declined. Clauseo works without it. To set it up, run this installer again and choose Yes. If Windows asks for an administrator's password, ask whoever looks after this computer to enter it."
LangString adminCopyFailed ${LANG_ENGLISH} "Clauseo is installed, but Safe workspace couldn't be set up. Clauseo works without it. Run this installer again to try once more. If it still isn't set up, ask whoever looks after this computer."
LangString adminCopyOtherAccount ${LANG_ENGLISH} "Clauseo is installed, but Safe workspace isn't set up, because another Windows account on this computer already has it. Clauseo works without it."
LangString adminCopyMachineFailed ${LANG_ENGLISH} "Clauseo is installed, but Safe workspace couldn't be set up on this computer. Clauseo works without it. Whoever looks after this computer can run this installer again to set it up."
LangString adminCopyNewerKept ${LANG_ENGLISH} "Clauseo kept the newer copy of Safe workspace's set-up program that's already on this computer."
LangString adminCopyAppRunning ${LANG_ENGLISH} "Clauseo is installed, but Safe workspace isn't set up, because Clauseo is running. Quit Clauseo, then run this installer again."
LangString adminCopyStillRunning ${LANG_ENGLISH} "Clauseo is still setting up Safe workspace. It finishes on its own."
LangString adminCopySkippedSilent ${LANG_ENGLISH} "Safe workspace isn't set up, because this install is silent. To set it up, run this installer again without /S."

; Tauri's stock English, as @tauri-apps/cli 2.12.2 ships it (crates/tauri-bundler/src/bundle/
; windows/nsis/languages/English.nsh at tauri-cli-v2.12.2), deleteAppData aside, with one fix:
; Tauri writes {{product_name}} in appRunning, appRunningOkKill and failedToKillApp, which NSIS
; shows as it is (language files are included, not rendered); here they use ${PRODUCTNAME}. On a
; Tauri upgrade, copy that file's lines here again and keep the fix; a key missing here has no
; English text.
LangString addOrReinstall ${LANG_ENGLISH} "Add/Reinstall components"
LangString alreadyInstalled ${LANG_ENGLISH} "Already Installed"
LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed. Select the operation you want to perform and click Next to continue."
LangString appRunning ${LANG_ENGLISH} "${PRODUCTNAME} is running! Please close it first then try again."
LangString appRunningOkKill ${LANG_ENGLISH} "${PRODUCTNAME} is running!$\nClick OK to kill it"
LangString chooseMaintenanceOption ${LANG_ENGLISH} "Choose the maintenance option to perform."
LangString choowHowToInstall ${LANG_ENGLISH} "Choose how you want to install ${PRODUCTNAME}."
LangString createDesktop ${LANG_ENGLISH} "Create desktop shortcut"
LangString dontUninstall ${LANG_ENGLISH} "Do not uninstall"
LangString dontUninstallDowngrade ${LANG_ENGLISH} "Do not uninstall (Downgrading without uninstall is disabled for this installer)"
LangString failedToKillApp ${LANG_ENGLISH} "Failed to kill ${PRODUCTNAME}. Please close it first then try again"
LangString installingWebview2 ${LANG_ENGLISH} "Installing WebView2..."
LangString newerVersionInstalled ${LANG_ENGLISH} "A newer version of ${PRODUCTNAME} is already installed! It is not recommended that you install an older version. If you really want to install this older version, it's better to uninstall the current version first. Select the operation you want to perform and click Next to continue."
LangString older ${LANG_ENGLISH} "older"
LangString olderOrUnknownVersionInstalled ${LANG_ENGLISH} "An $R4 version of ${PRODUCTNAME} is installed on your system. It's recommended that you uninstall the current version before installing. Select the operation you want to perform and click Next to continue."
LangString silentDowngrades ${LANG_ENGLISH} "Downgrades are disabled for this installer, can't proceed with the silent installer, please use the graphical interface installer instead.$\n"
LangString unableToUninstall ${LANG_ENGLISH} "Unable to uninstall!"
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall before installing"
LangString unknown ${LANG_ENGLISH} "unknown"
LangString webview2AbortError ${LANG_ENGLISH} "Failed to install WebView2! The app can't run without it. Try restarting the installer."
LangString webview2DownloadError ${LANG_ENGLISH} "Error: Downloading WebView2 Failed - $0"
LangString webview2DownloadSuccess ${LANG_ENGLISH} "WebView2 bootstrapper downloaded successfully"
LangString webview2Downloading ${LANG_ENGLISH} "Downloading WebView2 bootstrapper..."
LangString webview2InstallError ${LANG_ENGLISH} "Error: Installing WebView2 failed with exit code $1"
LangString webview2InstallSuccess ${LANG_ENGLISH} "WebView2 installed successfully"
