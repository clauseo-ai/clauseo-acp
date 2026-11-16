; The installer hooks with the Safe workspace on: hooks.nsh, compiled with CLAUSEO_SAFE_WORKSPACE
; defined, so the installers run the elevated set-up (admin_copy.nsh) and the uninstaller its
; elevated removal. The Safe workspace is off for launch (decision 0035, "Safe workspace off for
; launch"): release.yml names this file instead of hooks.nsh (bundle.windows.nsis.installerHooks)
; only when its SAFE_WORKSPACE switch is "true", which also has sidecars.mjs build the set-up
; program these hooks write to Program Files. The tauri bundler has no way to pass makensis a
; -D define, so this file is the define.
!define CLAUSEO_SAFE_WORKSPACE
!include "${__FILEDIR__}\hooks.nsh"
