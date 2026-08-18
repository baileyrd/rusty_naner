@echo off
REM Naner Terminal Launcher - Batch Wrapper
REM Runs vendor\bin\naner.exe -- the binary the release workflow stages and
REM that naner-init installs and updates. bin\ is the user's own directory
REM and ships empty. If naner.exe is not there yet, hands over to
REM naner-init.exe, which owns the bootstrap.

setlocal

REM This file sits at the root of the installation. %~dp0 always ends with a
REM backslash, and a NANER_ROOT that ends in one escapes the closing quote of
REM any "%NANER_ROOT%" a child process builds into a command line. Round-tripping
REM through a trailing dot drops the separator while leaving a drive root
REM C:\ intact.
for %%I in ("%~dp0.") do set "NANER_ROOT=%%~fI"
set "NANER_EXE=%NANER_ROOT%\vendor\bin\naner.exe"

if exist "%NANER_EXE%" (
    "%NANER_EXE%" %*
    goto :eof
)

REM No naner.exe yet. naner-init.exe owns the bootstrap: it prompts before
REM downloading anything, installs naner.exe, then launches it with these same
REM arguments. It sits at the root, where a first-time user drops it, or in
REM vendor\bin once an install has updated itself.
set "NANER_INIT=%NANER_ROOT%\naner-init.exe"
if not exist "%NANER_INIT%" set "NANER_INIT=%NANER_ROOT%\vendor\bin\naner-init.exe"

REM start /wait, not a bare call: naner-init is a GUI-subsystem binary, so
REM cmd.exe does not wait for it and its own next prompt would race
REM naner-init's Y/n prompt for the user's keystrokes -- rusty_naner#81.
if exist "%NANER_INIT%" (
    echo naner.exe not found - handing over to naner-init.exe to install it.
    start /wait "" "%NANER_INIT%" %*
    goto :eof
)

echo ERROR: neither naner.exe nor naner-init.exe was found.
echo.
echo Looked for:
echo   %NANER_EXE%
echo   %NANER_ROOT%\naner-init.exe
echo   %NANER_ROOT%\vendor\bin\naner-init.exe
echo.
echo Download naner-init.exe into this folder from
echo https://github.com/baileyrd/rusty_naner/releases/latest
exit /b 1
