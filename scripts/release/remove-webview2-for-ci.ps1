$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:CI -ne 'true') {
    throw 'WebView2 removal is restricted to a disposable GitHub Actions runner.'
}

$programFilesX86 = ${env:ProgramFiles(x86)}
if (-not $programFilesX86) {
    throw 'Program Files (x86) is unavailable.'
}
$runtimeRoot = [IO.Path]::GetFullPath(
    (Join-Path $programFilesX86 'Microsoft\EdgeWebView\Application')
)
$expectedParent = [IO.Path]::GetFullPath(
    (Join-Path $programFilesX86 'Microsoft\EdgeWebView')
)
if (-not [string]::Equals((Split-Path -Parent $runtimeRoot), $expectedParent, [StringComparison]::OrdinalIgnoreCase) -or
    -not [string]::Equals((Split-Path -Leaf $runtimeRoot), 'Application', [StringComparison]::Ordinal) -or
    -not (Test-Path -LiteralPath $runtimeRoot)) {
    throw "The expected WebView2 runtime root was not found: $runtimeRoot"
}

$versionDirectory = Get-ChildItem -LiteralPath $runtimeRoot -Directory |
    Where-Object { $_.Name -match '^\d+(\.\d+){1,3}$' } |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1
if ($null -eq $versionDirectory) {
    throw 'No installed Evergreen WebView2 runtime version was found.'
}
$setup = [IO.Path]::GetFullPath(
    (Join-Path $versionDirectory.FullName 'Installer\setup.exe')
)
if (-not $setup.StartsWith($runtimeRoot, [StringComparison]::OrdinalIgnoreCase) -or
    -not (Test-Path -LiteralPath $setup)) {
    throw "The WebView2 setup executable was not found beneath the verified runtime root: $setup"
}

Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
$uninstall = Start-Process -FilePath $setup `
    -ArgumentList '--uninstall', '--msedgewebview', '--system-level', '--force-uninstall' `
    -Wait -PassThru -WindowStyle Hidden
if ($uninstall.ExitCode -ne 0) {
    throw "WebView2 runtime removal exited with code $($uninstall.ExitCode)."
}

$deadline = [DateTime]::UtcNow.AddSeconds(30)
do {
    $runtimeExecutables = @(Get-ChildItem -LiteralPath $runtimeRoot -Filter 'msedgewebview2.exe' -Recurse -ErrorAction SilentlyContinue)
    if ($runtimeExecutables.Count -eq 0) { break }
    Start-Sleep -Milliseconds 250
} while ([DateTime]::UtcNow -lt $deadline)

if ($runtimeExecutables.Count -ne 0) {
    throw 'WebView2 runtime executables remain after the uninstall command.'
}
Write-Output 'WebView2 Runtime was removed from the disposable GitHub Actions runner.'
