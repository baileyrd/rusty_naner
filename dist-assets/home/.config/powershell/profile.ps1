# ============================================================================
# Naner PowerShell Profile
# Default profile for Naner terminal environment
# ============================================================================

# Override $PROFILE to point to this file instead of the default location
$global:PROFILE = "$env:NANER_ROOT\home\.config\powershell\profile.ps1"

# Suppress startup banner (optional - comment out to show vendor status)
# Set to $true to show detailed startup info, $false for minimal startup
$ShowNanerBanner = $false

if ($ShowNanerBanner) {
    Write-Host ""
    Write-Host "Naner Environment" -ForegroundColor Cyan
    Write-Host "================" -ForegroundColor Cyan

    if ($env:NANER_ROOT) {
        Write-Host "Root: $env:NANER_ROOT" -ForegroundColor DarkGray
    }

    # Display vendor tools status
    $vendorTools = @{
        "PowerShell" = "$env:NANER_ROOT\vendor\powershell\pwsh.exe"
        "Git Bash"   = "$env:NANER_ROOT\vendor\git\bin\bash.exe"
        "7-Zip"      = "$env:NANER_ROOT\vendor\7zip\7z.exe"
        "Terminal"   = "$env:NANER_ROOT\vendor\terminal\wt.exe"
    }

    $toolStatus = @()
    foreach ($tool in $vendorTools.GetEnumerator()) {
        if (Test-Path $tool.Value) {
            $toolStatus += $tool.Key
        }
    }

    if ($toolStatus.Count -gt 0) {
        Write-Host "Tools: " -NoNewline -ForegroundColor DarkGray
        Write-Host ($toolStatus -join ", ") -ForegroundColor Green
    }
    Write-Host ""
}

# ============================================================================
# Module Imports
# ============================================================================

# Optional shell-experience modules -- not vendored by naner, so only loaded
# when the user has installed them separately (e.g. via `Install-Module`).
foreach ($module in 'posh-git', 'Terminal-Icons', 'z') {
    if (Get-Module -ListAvailable -Name $module) {
        Import-Module $module -ErrorAction SilentlyContinue
    }
}

# ============================================================================
# Environment Setup
# ============================================================================

# Ensure PSReadLine is loaded for better command line editing
if (Get-Module -ListAvailable -Name PSReadLine) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue

    # Set PSReadLine options for better experience
    Set-PSReadLineOption -PredictionSource History -ErrorAction SilentlyContinue
    Set-PSReadLineOption -PredictionViewStyle ListView -ErrorAction SilentlyContinue
    Set-PSReadLineOption -EditMode Windows -ErrorAction SilentlyContinue
    Set-PSReadLineOption -HistoryNoDuplicates:$true -ErrorAction SilentlyContinue

    # Key bindings
    Set-PSReadLineKeyHandler -Key Tab -Function MenuComplete -ErrorAction SilentlyContinue
    Set-PSReadLineKeyHandler -Key UpArrow -Function HistorySearchBackward -ErrorAction SilentlyContinue
    Set-PSReadLineKeyHandler -Key DownArrow -Function HistorySearchForward -ErrorAction SilentlyContinue
}

# Persistent UTF-8 -- needed for Nerd Font glyphs (prompt + Terminal-Icons).
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

# ============================================================================
# Aliases and Functions
# ============================================================================

# Common Unix-like aliases
Set-Alias -Name l -Value Get-ChildItem -ErrorAction SilentlyContinue
Set-Alias -Name ls -Value Get-ChildItem -ErrorAction SilentlyContinue
# `grep` deliberately left unaliased: naner vendors a real grep.exe (Git for
# Windows/MSYS2) on PATH, and Select-String's flags don't match its syntax.
Set-Alias -Name vim -Value nvim -ErrorAction SilentlyContinue
Set-Alias -Name which -Value Get-Command -ErrorAction SilentlyContinue
Set-Alias -Name env -Value Get-EnvVars -ErrorAction SilentlyContinue

# Git shortcuts (if git is available)
function gs { git status }
function ga { git add $args }
function gc { git commit -m $args }
function gp { git push }
function gl { git log --oneline --graph --decorate }
function gd { git diff $args }

# Navigation shortcuts
function .. { Set-Location .. }
function ... { Set-Location ..\.. }
function .... { Set-Location ..\..\.. }
function ~ { Set-Location $env:USERPROFILE }

# Create and enter directory
function mkcd {
    param([string]$Path)
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
    Set-Location $Path
}

# Quick edit of PowerShell profile
function Edit-Profile {
    $profilePath = "$env:NANER_ROOT\home\.config\powershell\profile.ps1"
    if (Test-Path $profilePath) {
        if (Get-Command code -ErrorAction SilentlyContinue) {
            code $profilePath
        } elseif (Get-Command notepad -ErrorAction SilentlyContinue) {
            notepad $profilePath
        } else {
            Write-Host "No editor found. Profile location: $profilePath"
        }
    }
}
Set-Alias -Name eprofile -Value Edit-Profile -ErrorAction SilentlyContinue

# Reload profile
function Reload-Profile {
    . "$env:NANER_ROOT\home\.config\powershell\profile.ps1"
    Write-Host "Profile reloaded" -ForegroundColor Green
}
Set-Alias -Name reload -Value Reload-Profile -ErrorAction SilentlyContinue

# Show PATH entries
function Show-Path {
    $env:PATH -split ';' | ForEach-Object { Write-Host $_ }
}

# Show all environment variables
function Get-EnvVars {
    Get-ChildItem Env:
}

# List directory with colors and details
function ll {
    param([string]$Path = ".")
    Get-ChildItem -Path $Path | Format-Table -AutoSize Mode, LastWriteTime, Length, Name
}

# Find files by name
function Find-File {
    param([string]$Pattern)
    Get-ChildItem -Recurse -Filter $Pattern -ErrorAction SilentlyContinue | Select-Object FullName
}
Set-Alias -Name ff -Value Find-File -ErrorAction SilentlyContinue

# ============================================================================
# Prompt (oh-my-posh, if installed)
# ============================================================================

# OhMyPosh is an optional vendor (`naner install ohmyposh`), not part of the
# essential bootstrap -- guarded so a shell launched before it's installed
# still gets a working (plain) prompt instead of an import error on every
# startup.
if (Get-Command oh-my-posh -ErrorAction SilentlyContinue) {
    oh-my-posh init pwsh --config "jandedobbeleer" | Invoke-Expression
}

# ============================================================================
# Command-not-found suggestions
# ============================================================================

# When a command isn't found, ask naner whether a vendor provides it and print
# the hint above the shell's own error. Guarded on naner.exe existing, errors
# swallowed, and `naner suggest` is silent on no match — a mistyped command
# costs one fast offline lookup, nothing more.
if (Test-Path "$env:NANER_ROOT\vendor\bin\naner.exe") {
    $ExecutionContext.InvokeCommand.CommandNotFoundAction = {
        param($CommandName, $CommandLookupEventArgs)
        if ($CommandLookupEventArgs.CommandOrigin -eq 'Runspace' -and $CommandName -notlike 'get-*') {
            try {
                & "$env:NANER_ROOT\vendor\bin\naner.exe" suggest $CommandName 2>$null | Write-Host
            } catch { }
        }
    }
}

# ============================================================================
# User Customizations
# ============================================================================

# Add your custom aliases, functions, and settings below this line
# This section is preserved when updating the default profile

# Example: Set default editor
# $env:EDITOR = "code"

# Example: Custom function
# function MyFunction {
#     Write-Host "Hello from custom function"
# }

# ============================================================================
# End of Profile
# ============================================================================
