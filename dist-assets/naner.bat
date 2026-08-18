@echo off
REM Naner Terminal Launcher - Batch Wrapper
REM Resolves NANER_ROOT from this file's own location and runs
REM vendor\bin\naner.exe -- the binary the release workflow stages and that
REM naner-init installs and updates. (bin\ is the user's own directory and
REM ships empty.)

setlocal

REM This file sits at the root of the installation. %~dp0 always ends with a
REM backslash, and a NANER_ROOT that ends in one escapes the closing quote of
REM any "%NANER_ROOT%" a child process builds into a command line. Round-tripping
REM through a trailing dot drops the separator while leaving a drive root
REM (C:\) intact.
for %%I in ("%~dp0.") do set "NANER_ROOT=%%~fI"
set "NANER_EXE=%NANER_ROOT%\vendor\bin\naner.exe"

if not exist "%NANER_EXE%" (
    echo ERROR: naner.exe not found at %NANER_EXE%
    echo.
    echo Run naner-init.exe in this folder to install or repair the
    echo installation. If you do not have it, download it from
    echo https://github.com/baileyrd/rusty_naner/releases/latest
    exit /b 1
)

"%NANER_EXE%" %*

endlocal
