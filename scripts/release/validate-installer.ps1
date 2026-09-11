param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$ExpectedVersion = '0.1.0'
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$testRoot = Join-Path $env:TEMP ("cairn-installer-validation-" + [guid]::NewGuid())
$appData = Join-Path $testRoot 'app-data'
$library = Join-Path $testRoot 'user-library'
$tracked = Join-Path $testRoot 'tracked-source'
$report = Join-Path $testRoot 'installed-workflow.json'
$ready = "$report.ready"
New-Item -ItemType Directory -Force -Path $appData, $library, $tracked | Out-Null
$trackedSource = Join-Path $tracked 'tracked-proof.md'
Set-Content -LiteralPath $trackedSource -Value '# Original tracked file remains unchanged.' -NoNewline
$applicationProcess = $null
$uninstaller = $null
$uninstallAttempted = $false

function Find-CairnUninstallEntry {
    $uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    if (-not (Test-Path -LiteralPath $uninstallRoot)) { return $null }
    Get-ChildItem -LiteralPath $uninstallRoot |
        Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Cairn.md' } |
        Select-Object -First 1
}

function Resolve-UninstallerPath($entry) {
    $command = [Environment]::ExpandEnvironmentVariables([string]$entry.UninstallString).Trim()
    if (-not $command) {
        throw 'The Cairn.md uninstall command is missing'
    }
    if ($command.StartsWith('"')) {
        $closingQuote = $command.IndexOf('"', 1)
        if ($closingQuote -lt 2) {
            throw 'The Cairn.md uninstall command is invalid'
        }
        return $command.Substring(1, $closingQuote - 1)
    }
    if (Test-Path -LiteralPath $command) {
        return $command
    }
    $executable = [regex]::Match(
        $command,
        '^(?<path>.+?\.exe)(?:\s|$)',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if (-not $executable.Success) {
        throw 'The Cairn.md uninstall command has no executable path'
    }
    return $executable.Groups['path'].Value
}

if ($null -ne (Find-CairnUninstallEntry)) {
    throw 'Cairn.md is already installed for this user; use a disposable account or runner'
}

try {
    $install = Start-Process -FilePath $resolvedInstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    if ($install.ExitCode -ne 0) {
        throw "Installer exited with code $($install.ExitCode)"
    }

    $entry = Find-CairnUninstallEntry
    if ($null -eq $entry) {
        throw 'Cairn.md did not register a current-user uninstall entry'
    }
    if ($entry.DisplayVersion -ne $ExpectedVersion) {
        throw "Installed version $($entry.DisplayVersion) does not match $ExpectedVersion"
    }
    $uninstaller = Resolve-UninstallerPath $entry
    if (-not (Test-Path -LiteralPath $uninstaller)) {
        throw 'The Cairn.md uninstaller is missing'
    }
    $registeredLocation = ([string]$entry.InstallLocation).Trim().Trim('"')
    $installLocation = if ($registeredLocation -and (Test-Path -LiteralPath $registeredLocation)) {
        $registeredLocation
    } else {
        Split-Path -Parent $uninstaller
    }
    if (-not $installLocation -or -not (Test-Path -LiteralPath $installLocation)) {
        throw 'The registered install location is missing'
    }
    $application = Get-ChildItem -LiteralPath $installLocation -Filter '*.exe' |
        Where-Object { $_.Name -notmatch 'uninstall' } |
        Select-Object -First 1
    if ($null -eq $application) {
        throw 'The installed Cairn.md executable is missing'
    }
    $env:CAIRN_PERF_MODE = '1'
    $env:CAIRN_PERF_SCENARIO = 'installed'
    $env:CAIRN_PERF_OUTPUT = $report
    $env:CAIRN_PERF_LIBRARY_ROOT = $library
    $env:CAIRN_PERF_TRACKED_ROOT = $tracked
    $env:CAIRN_APP_DATA_DIR = $appData
    $applicationProcess = Start-Process -FilePath $application.FullName -PassThru -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddSeconds(45)
    while (-not (Test-Path -LiteralPath $ready) -and [DateTime]::UtcNow -lt $deadline) {
        if ($applicationProcess.HasExited) {
            throw "Installed Cairn.md exited with code $($applicationProcess.ExitCode) before completing its workflow"
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath $ready) -or -not (Test-Path -LiteralPath $report)) {
        throw 'Installed Cairn.md did not complete its workflow within 45 seconds'
    }
    $workflow = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    if ($workflow.benchmark -ne 'cairn-installed-workflow' -or $workflow.passed -ne $true) {
        $failedChecks = @($workflow.checks.psobject.Properties | Where-Object { $_.Value -ne $true } | ForEach-Object { $_.Name })
        throw "Installed Cairn.md workflow failed: $($failedChecks -join ', ')"
    }
    $termination = Start-Process -FilePath 'taskkill.exe' `
        -ArgumentList '/PID', $applicationProcess.Id, '/T', '/F' `
        -Wait -PassThru -WindowStyle Hidden
    if ($termination.ExitCode -ne 0) {
        throw "Installed Cairn.md could not be stopped after validation (code $($termination.ExitCode))"
    }
    $applicationProcess.WaitForExit(10000) | Out-Null

    $savedDocument = Join-Path (Join-Path $library 'Installed Project') 'installed-proof.md'
    if (-not (Test-Path -LiteralPath $savedDocument) -or
        (Get-Content -LiteralPath $savedDocument -Raw) -notlike '*Edited through the installed Cairn.md visual editor.*') {
        throw 'The installed workflow did not leave the expected autosaved Markdown file'
    }
    if ((Get-Content -LiteralPath $trackedSource -Raw) -ne '# Original tracked file remains unchanged.') {
        throw 'The installed workflow changed the original tracked Markdown file'
    }

    $reinstall = Start-Process -FilePath $resolvedInstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    if ($reinstall.ExitCode -ne 0) {
        throw "Same-version reinstall exited with code $($reinstall.ExitCode)"
    }
    $reinstalledEntry = Find-CairnUninstallEntry
    if ($null -eq $reinstalledEntry -or $reinstalledEntry.DisplayVersion -ne $ExpectedVersion) {
        throw 'Cairn.md reinstall did not preserve its current-user registration'
    }
    $uninstaller = Resolve-UninstallerPath $reinstalledEntry
    if (-not (Test-Path -LiteralPath $savedDocument)) {
        throw 'Cairn.md reinstall removed the user-selected library'
    }

    $uninstallAttempted = $true
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    if ($uninstall.ExitCode -ne 0) {
        throw "Uninstaller exited with code $($uninstall.ExitCode)"
    }
    if (-not (Test-Path -LiteralPath $savedDocument)) {
        throw 'Uninstall removed user-owned library content'
    }
    if (Test-Path -LiteralPath $application.FullName) {
        throw 'The installed Cairn.md executable remains after uninstall'
    }
    if ($null -ne (Find-CairnUninstallEntry)) {
        throw 'The Cairn.md uninstall registration remains after uninstall'
    }
    Write-Output "Cairn.md $ExpectedVersion installer validation passed"
}
finally {
    if ($null -ne $applicationProcess -and -not $applicationProcess.HasExited) {
        Start-Process -FilePath 'taskkill.exe' `
            -ArgumentList '/PID', $applicationProcess.Id, '/T', '/F' `
            -Wait -WindowStyle Hidden -ErrorAction SilentlyContinue
    }
    if (-not $uninstallAttempted -and $null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller)) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -WindowStyle Hidden -ErrorAction SilentlyContinue
    }
    Remove-Item Env:CAIRN_PERF_MODE -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_SCENARIO -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_OUTPUT -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_LIBRARY_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_TRACKED_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_APP_DATA_DIR -ErrorAction SilentlyContinue
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    $resolvedTempRoot = [IO.Path]::GetFullPath($env:TEMP)
    if ($resolvedTestRoot.StartsWith($resolvedTempRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolvedTestRoot).StartsWith('cairn-installer-validation-', [StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
