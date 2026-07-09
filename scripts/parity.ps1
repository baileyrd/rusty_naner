<#
.SYNOPSIS
Golden-output parity harness: runs the C# naner.exe and the Rust naner
side-by-side and diffs stdout, stderr, and exit codes
(MIGRATION_ANALYSIS §5.2).

.DESCRIPTION
Runs the fixed command matrix against both executables inside the same
working directory (point -WorkingDirectory at a fixture tree for the
initialized-root cases, or at an empty temp dir for the missing-root case),
masks timestamps, writes per-case captures to -OutDir, and prints a summary
table. Exits non-zero when any case diverges unless -AllowFailures is set —
Phase 0/1 runs use -AllowFailures since the Rust side is a stub by design.

.EXAMPLE
./scripts/parity.ps1 -CSharpExe C:\naner\vendor\bin\naner.exe `
    -RustExe target\release\naner.exe -WorkingDirectory C:\naner -AllowFailures
#>
param(
    [Parameter(Mandatory)] [string]$CSharpExe,
    [Parameter(Mandatory)] [string]$RustExe,
    [string]$WorkingDirectory = (Get-Location).Path,
    [string]$OutDir = 'parity-out',
    [switch]$AllowFailures
)

$ErrorActionPreference = 'Stop'

$cases = @(
    @{ Name = 'version';         Cmd = @('--version') },
    @{ Name = 'help';            Cmd = @('--help') },
    @{ Name = 'diagnose';        Cmd = @('--diagnose') },
    @{ Name = 'export-env-ps';   Cmd = @('--export-env', '-f', 'powershell') },
    @{ Name = 'export-env-bash'; Cmd = @('--export-env', '-f', 'bash') },
    @{ Name = 'export-env-cmd';  Cmd = @('--export-env', '-f', 'cmd') },
    @{ Name = 'export-env-nc';   Cmd = @('--export-env', '--no-comments') },
    @{ Name = 'install-list';    Cmd = @('install', '--list') },
    @{ Name = 'bad-args';        Cmd = @('--definitely-not-a-real-flag') }
)

function Mask([string]$text) {
    if ($null -eq $text) { return '' }
    # Timestamps in export headers and logs are the only expected
    # nondeterminism (MIGRATION_ANALYSIS §5.2).
    $text -replace '\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}(:\d{2})?', '<TIMESTAMP>'
}

function Capture([string]$exe, [string[]]$cmdArgs, [string]$prefix) {
    $stdout = "$prefix.stdout"
    $stderr = "$prefix.stderr"
    $proc = Start-Process -FilePath $exe -ArgumentList $cmdArgs `
        -WorkingDirectory $WorkingDirectory -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Set-Content -Path $stdout -Value (Mask (Get-Content -Raw $stdout -ErrorAction SilentlyContinue))
    Set-Content -Path $stderr -Value (Mask (Get-Content -Raw $stderr -ErrorAction SilentlyContinue))
    return $proc.ExitCode
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$failures = 0
$rows = foreach ($case in $cases) {
    $base = Join-Path $OutDir $case.Name
    $csExit = Capture $CSharpExe $case.Cmd "$base.csharp"
    $rsExit = Capture $RustExe   $case.Cmd "$base.rust"

    $outSame = -not (Compare-Object (Get-Content "$base.csharp.stdout") (Get-Content "$base.rust.stdout"))
    $errSame = -not (Compare-Object (Get-Content "$base.csharp.stderr") (Get-Content "$base.rust.stderr"))
    $exitSame = $csExit -eq $rsExit
    $pass = $outSame -and $errSame -and $exitSame
    if (-not $pass) { $failures++ }

    [pscustomobject]@{
        Case    = $case.Name
        Stdout  = if ($outSame) { 'ok' } else { 'DIFF' }
        Stderr  = if ($errSame) { 'ok' } else { 'DIFF' }
        Exit    = if ($exitSame) { "ok ($csExit)" } else { "DIFF ($csExit vs $rsExit)" }
        Result  = if ($pass) { 'PASS' } else { 'FAIL' }
    }
}

$rows | Format-Table -AutoSize
Write-Host "Captures in $OutDir (diff the .csharp.* / .rust.* pairs for details)."

if ($failures -gt 0) {
    Write-Host "$failures/$($cases.Count) cases diverge."
    if (-not $AllowFailures) { exit 1 }
    Write-Host '-AllowFailures set: divergence tolerated (pre-parity phase).'
}
exit 0
