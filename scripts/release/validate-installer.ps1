param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$ExpectedVersion = '0.1.0'
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$testRoot = Join-Path $env:TEMP ("cairn-installer-validation-" + [guid]::NewGuid())
$appData = Join-Path $testRoot 'app-data'
$report = Join-Path $testRoot 'browser-report.json'
$ready = "$report.ready"
$library = Join-Path $testRoot 'user-library'
New-Item -ItemType Directory -Force -Path $appData, $library | Out-Null
$sentinel = Join-Path $library 'keep-me.md'
Set-Content -LiteralPath $sentinel -Value '# User-owned library content' -NoNewline
$applicationProcess = $null
$uninstaller = $null
$uninstallAttempted = $false

function Find-CairnUninstallEntry {
    Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' |
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
    $env:CAIRN_PERF_SCENARIO = 'idle'
    $env:CAIRN_PERF_OUTPUT = $report
    $env:CAIRN_APP_DATA_DIR = $appData
    $applicationProcess = Start-Process -FilePath $application.FullName -PassThru -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $ready) -and [DateTime]::UtcNow -lt $deadline) {
        if ($applicationProcess.HasExited) {
            throw "Installed Cairn.md exited with code $($applicationProcess.ExitCode) before becoming ready"
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath $ready)) {
        throw 'Installed Cairn.md did not become editor-ready within 30 seconds'
    }
    Stop-Process -Id $applicationProcess.Id -Force -ErrorAction SilentlyContinue
    $null = $applicationProcess.WaitForExit(10000)

    $uninstallAttempted = $true
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    if ($uninstall.ExitCode -ne 0) {
        throw "Uninstaller exited with code $($uninstall.ExitCode)"
    }
    if (-not (Test-Path -LiteralPath $sentinel)) {
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
        Stop-Process -Id $applicationProcess.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not $uninstallAttempted -and $null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller)) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -WindowStyle Hidden -ErrorAction SilentlyContinue
    }
    Remove-Item Env:CAIRN_PERF_MODE -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_SCENARIO -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_PERF_OUTPUT -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_APP_DATA_DIR -ErrorAction SilentlyContinue
}
