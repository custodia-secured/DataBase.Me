@echo off
rem Double-click: builds DataBase.Me from this checkout and opens it in its own window.
rem This is the desktop app, not a web page. There is no browser version yet: server mode is v2.
rem Keep this window open while you use it. Closing the app, or Ctrl+C here, stops it.
rem It opens the same vault and settings as the installed app. See the warning printed below.
rem Inherited CLAUDE variables are cleared first.
setlocal
title DataBase.Me
for /f "delims==" %%v in ('set CLAUDE 2^>nul') do set "%%v="
cd /d "%~dp0"

where pnpm >nul 2>nul
if errorlevel 1 set "MISSING=pnpm"
where cargo >nul 2>nul
if errorlevel 1 set "MISSING=%MISSING% cargo"
if defined MISSING (
  echo Nothing was started, because this needs what is not on PATH: %MISSING%
  echo CONTRIBUTING.md says what to install.
  pause
  exit /b 1
)

if not exist "node_modules\" (
  echo The packages are not installed, so nothing was started. Run this once, then try again:
  echo.
  echo   pnpm install --dir "%~dp0."
  echo.
  pause
  exit /b 1
)

netstat -ano | findstr LISTENING | findstr /r ":1420\>"
if not errorlevel 1 (
  echo.
  echo Port 1420 is already in use, so nothing was started.
  echo If that is DataBase.Me, its window is already open. Otherwise, stop what holds the port.
  pause
  exit /b 1
)

echo This opens your real vault, the same one the installed app uses.
echo A branch that changes the vault format upgrades that vault as it opens, and the
echo installed app then refuses it, so back up the vault before running such a branch.
echo The first build takes a few minutes. Later ones take seconds.
echo.
call pnpm dev
if errorlevel 1 (
  echo.
  echo DataBase.Me stopped. If you did not stop it, the reason is above.
  pause
)
endlocal
