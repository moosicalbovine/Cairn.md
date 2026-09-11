param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$ExpectedVersion = '0.1.0'
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$testRoot = Join-Path $env:TEMP ("cairn-installer-validation-" + [guid]::NewGuid())
$library = Join-Path $testRoot 'user-library'
New-Item -ItemType Directory -Force -Path $library | Out-Null
$sentinel = Join-Path $library 'keep-me.md'
Set-Content -LiteralPath $sentinel -Value '# User-owned library content' -NoNewline
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
    $desktopFlow = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\test\desktop-flow.ps1')).Path
    & $desktopFlow -BinaryPath $application.FullName -Scenario workspace -InstalledRelease

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
    if (-not $uninstallAttempted -and $null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller)) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -WindowStyle Hidden -ErrorAction SilentlyContinue
    }
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    $resolvedTempRoot = [IO.Path]::GetFullPath($env:TEMP)
    if ($resolvedTestRoot.StartsWith($resolvedTempRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolvedTestRoot).StartsWith('cairn-installer-validation-', [StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
