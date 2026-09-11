param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$TauriConfigPath = 'src-tauri\tauri.conf.json',
    [string]$GeneratedNsisRoot = 'src-tauri\target\release\nsis'
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$resolvedConfig = (Resolve-Path -LiteralPath $TauriConfigPath).Path
$resolvedNsisRoot = (Resolve-Path -LiteralPath $GeneratedNsisRoot).Path

$config = Get-Content -LiteralPath $resolvedConfig -Raw | ConvertFrom-Json
$installMode = $config.bundle.windows.webviewInstallMode
if ($installMode.type -ne 'embedBootstrapper' -or $installMode.silent -ne $true) {
    throw 'Tauri must package the silent embedded WebView2 bootstrapper.'
}

$generatedScript = Get-ChildItem -LiteralPath $resolvedNsisRoot -Filter 'installer.nsi' -Recurse |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if ($null -eq $generatedScript) {
    throw "The generated Tauri NSIS script was not found below $resolvedNsisRoot"
}
$scriptSource = Get-Content -LiteralPath $generatedScript.FullName -Raw
$requiredStatements = @(
    '!define INSTALLWEBVIEW2MODE "embedBootstrapper"',
    'File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"',
    'ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1'
)
foreach ($statement in $requiredStatements) {
    if (-not $scriptSource.Contains($statement, [StringComparison]::Ordinal)) {
        throw "The generated NSIS script is missing its embedded WebView2 install path: $statement"
    }
}

$bootstrapperDefinition = [regex]::Match(
    $scriptSource,
    '(?m)^!define WEBVIEW2BOOTSTRAPPERPATH "(?<path>.+)"\s*$'
)
if (-not $bootstrapperDefinition.Success) {
    throw 'The generated NSIS script does not identify its WebView2 bootstrapper source.'
}
$bootstrapperText = $bootstrapperDefinition.Groups['path'].Value
if ($bootstrapperText.StartsWith('${__FILEDIR__}', [StringComparison]::Ordinal)) {
    $bootstrapperText = Join-Path `
        (Split-Path -Parent $generatedScript.FullName) `
        $bootstrapperText.Substring('${__FILEDIR__}'.Length).TrimStart('\')
}
$bootstrapperSource = (Resolve-Path -LiteralPath $bootstrapperText).Path
$sourceSignature = Get-AuthenticodeSignature -LiteralPath $bootstrapperSource
if ($sourceSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
    $sourceSignature.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
    throw 'The WebView2 bootstrapper source is not validly signed by Microsoft.'
}

$sevenZip = Get-Command '7z.exe' -ErrorAction Stop
$extractRoot = Join-Path $env:TEMP ("cairn-webview2-proof-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $extractRoot | Out-Null
try {
    & $sevenZip.Source x -y "-o$extractRoot" $resolvedInstaller | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip could not inspect the NSIS installer (exit code $LASTEXITCODE)."
    }
    $embeddedBootstrapper = Get-ChildItem -LiteralPath $extractRoot `
        -Filter 'MicrosoftEdgeWebview2Setup.exe' -File -Recurse |
        Select-Object -First 1
    if ($null -eq $embeddedBootstrapper) {
        throw 'The final NSIS installer does not contain the WebView2 bootstrapper.'
    }
    $sourceHash = (Get-FileHash -LiteralPath $bootstrapperSource -Algorithm SHA256).Hash
    $embeddedHash = (Get-FileHash -LiteralPath $embeddedBootstrapper.FullName -Algorithm SHA256).Hash
    if ($embeddedHash -ne $sourceHash) {
        throw 'The WebView2 payload in the final installer does not match the signed build input.'
    }
    $embeddedSignature = Get-AuthenticodeSignature -LiteralPath $embeddedBootstrapper.FullName
    if ($embeddedSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
        $embeddedSignature.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
        throw 'The embedded WebView2 bootstrapper is not validly signed by Microsoft.'
    }
}
finally {
    $resolvedExtractRoot = [IO.Path]::GetFullPath($extractRoot)
    $resolvedTempRoot = [IO.Path]::GetFullPath($env:TEMP)
    if ($resolvedExtractRoot.StartsWith($resolvedTempRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolvedExtractRoot).StartsWith('cairn-webview2-proof-', [StringComparison]::Ordinal)) {
        Remove-Item -LiteralPath $resolvedExtractRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Output 'The final NSIS installer contains the expected Microsoft-signed WebView2 bootstrapper.'
