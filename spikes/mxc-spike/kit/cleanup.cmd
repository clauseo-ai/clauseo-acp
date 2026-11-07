@echo off
rem MXC probe kit: undoes what run-probe.cmd left behind (see README.txt). No admin needed.
setlocal
set "HERE=%~dp0"
where pwsh >nul 2>nul
if %ERRORLEVEL%==0 (set "PSEXE=pwsh") else (set "PSEXE=powershell")
"%PSEXE%" -NoProfile -ExecutionPolicy Bypass -File "%HERE%cleanup.ps1" -Results "%HERE%results"
pause
