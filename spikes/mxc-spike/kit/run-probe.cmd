@echo off
rem MXC probe kit: runs probe.ps1 in this folder. The execution-policy bypass applies to this
rem one PowerShell process only; nothing on the machine changes. No admin needed.
setlocal
set "HERE=%~dp0"
where pwsh >nul 2>nul
if %ERRORLEVEL%==0 (set "PSEXE=pwsh") else (set "PSEXE=powershell")
"%PSEXE%" -NoProfile -ExecutionPolicy Bypass -File "%HERE%probe.ps1" -Results "%HERE%results"
echo.
echo Results are in %HERE%results. Run cleanup.cmd to undo everything the probe left behind.
pause
