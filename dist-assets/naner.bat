@echo off
REM Naner Terminal Launcher - Batch Wrapper
REM Calls naner.exe (C# version) if available, else falls back to PowerShell

setlocal

REM Get the directory where this batch file is located
set "NANER_ROOT=%~dp0"
set "NANER_EXE=%NANER_ROOT%vendor\bin\naner.exe"
set "PS_SCRIPT=%NANER_ROOT%src\powershell\Invoke-Naner.ps1"

REM Check if C# executable exists
if exist "%NANER_EXE%" (
    REM Use C# version (preferred)
    "%NANER_EXE%" %*
) else (
    REM Fall back to PowerShell version
    echo WARNING: naner.exe not found. Using PowerShell fallback...
    echo To build C# version: cd src\csharp ^&^& .\build.ps1
    echo.

    if not exist "%PS_SCRIPT%" (
        echo ERROR: PowerShell script not found at %PS_SCRIPT%
        exit /b 1
    )

    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %*
)

endlocal
