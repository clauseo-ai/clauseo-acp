; Tauri's installer messages in English (bundle.windows.nsis.customLanguageFiles in
; crates/desktop/tauri.conf.json). Naming a file there replaces the bundler's own English.nsh, so
; this file holds all of Tauri's messages: Clauseo's wording first, then Tauri's stock English.
; No byte order mark: the bundler adds one when it copies the file.

; Clauseo's wording: the one line whose words differ from Tauri's (approved by Rohan, 2026-11-03),
; then the four lines windows/hooks.nsh shows after the app deletes its data (S20, 2026-11-05;
; deleteAppDataFailedUnnamed 2026-11-06).
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
; by its name instead. Provisional (S20 follow-up, 2026-11-06; for the app thread's copy page).
LangString deleteAppDataFailedUnnamed ${LANG_ENGLISH} "Clauseo couldn't delete all of its data. Its data is in the ai.clauseo.desktop folder in your local AppData folder."

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
