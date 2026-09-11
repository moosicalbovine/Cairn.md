param(
    [string]$BinaryPath = "src-tauri\target\debug\cairn-md.exe",
    [ValidateSet('workspace', 'recovery')]
    [string]$Scenario = 'workspace'
)

$ErrorActionPreference = 'Stop'
$port = 4444
$elementKey = 'element-6066-11e4-a52e-4f735466cecf'
$webviewIdentifier = 'io.github.moosicalbovine.cairn-md-webdriver'
$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
$driverCommand = Get-Command 'msedgedriver' -ErrorAction Stop
$testBase = Join-Path ([IO.Path]::GetTempPath()) ("cairn-webdriver-" + [Guid]::NewGuid().ToString('N'))
$appData = Join-Path $testBase 'app-data'
$libraryRoot = Join-Path $testBase 'library'
# Tauri forces an unconfigured WebView data directory to LOCALAPPDATA/<identifier>.
# Keep this identifier aligned with tauri.webdriver.conf.json so EdgeDriver can attach.
$webviewData = Join-Path $env:LOCALAPPDATA $webviewIdentifier
$stdoutPath = Join-Path $testBase 'edge-driver.stdout.log'
$stderrPath = Join-Path $testBase 'edge-driver.stderr.log'
$recoveryMarkerPath = Join-Path $testBase 'recovery-marker.txt'
$diskSaveBlockPath = Join-Path $testBase 'disk-save-blocked.ready'
$driver = $null
$sessionId = $null

function Invoke-Driver {
    param(
        [Parameter(Mandatory)][string]$Method,
        [Parameter(Mandatory)][string]$Path,
        [object]$Body
    )
    $parameters = @{
        Method = $Method
        Uri = "http://127.0.0.1:$Port$Path"
        ContentType = 'application/json'
        TimeoutSec = 60
    }
    if ($null -ne $Body) {
        $parameters.Body = $Body | ConvertTo-Json -Depth 8 -Compress
    }
    Invoke-RestMethod @parameters
}

function Wait-Driver {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            Invoke-Driver -Method Get -Path '/status' | Out-Null
            return
        } catch {
            Start-Sleep -Milliseconds 250
        }
    }
    throw 'Microsoft Edge WebDriver did not become ready within 30 seconds.'
}

function Clear-WebViewData {
    param([switch]$BestEffort)

    $resolvedProfile = [IO.Path]::GetFullPath($webviewData)
    $resolvedLocalAppData = [IO.Path]::GetFullPath($env:LOCALAPPDATA)
    $expectedProfile = [IO.Path]::GetFullPath(
        [IO.Path]::Combine($resolvedLocalAppData, $webviewIdentifier)
    )
    if (-not [string]::Equals(
        $resolvedProfile,
        $expectedProfile,
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Refusing to clear an unexpected WebView2 profile path: $resolvedProfile"
    }
    if (-not (Test-Path -LiteralPath $resolvedProfile)) { return }

    for ($attempt = 0; $attempt -lt 10; $attempt += 1) {
        try {
            Remove-Item -LiteralPath $resolvedProfile -Recurse -Force -ErrorAction Stop
            return
        } catch {
            if ($attempt -eq 9) {
                if ($BestEffort) {
                    Write-Warning "Could not clear the dedicated WebView2 test profile: $resolvedProfile"
                    return
                }
                throw
            }
            Start-Sleep -Milliseconds 250
        }
    }
}

function Start-DriverSession {
    $script:driver = Start-Process -FilePath $driverCommand.Source `
        -ArgumentList "--port=$port", '--host=127.0.0.1', '--verbose' `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath `
        -WindowStyle Hidden -PassThru
    Wait-Driver

    $session = Invoke-Driver -Method Post -Path '/session' -Body @{
        capabilities = @{
            alwaysMatch = @{
                browserName = 'webview2'
                'ms:edgeChromium' = $true
                'ms:edgeOptions' = @{
                    binary = $resolvedBinary
                    args = @()
                    webviewOptions = @{ userDataFolder = $webviewData }
                }
            }
        }
    }
    $script:sessionId = $session.value.sessionId
    if (-not $script:sessionId) { $script:sessionId = $session.sessionId }
    if (-not $script:sessionId) { throw 'WebDriver did not return a session id.' }
}

function Stop-DriverSession {
    if ($script:sessionId) {
        try { Invoke-Driver -Method Delete -Path "/session/$script:sessionId" | Out-Null } catch {}
        $script:sessionId = $null
    }
    if ($script:driver -and -not $script:driver.HasExited) {
        Stop-Process -Id $script:driver.Id -Force -ErrorAction SilentlyContinue
        $script:driver.WaitForExit(5000) | Out-Null
    }
    $script:driver = $null
}

function Find-CairnProcess {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process = Get-Process -Name 'cairn-md' -ErrorAction SilentlyContinue |
            Where-Object {
                try {
                    [string]::Equals($_.Path, $resolvedBinary, [StringComparison]::OrdinalIgnoreCase)
                } catch {
                    $false
                }
            } |
            Select-Object -First 1
        if ($process) { return $process }
        Start-Sleep -Milliseconds 100
    }
    throw 'The Cairn.md desktop process was not found.'
}

function Wait-File {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Description,
        [int]$TimeoutSeconds = 15
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Path) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "$Description was not available within $TimeoutSeconds seconds."
}

function Wait-FileValue {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string[]]$Expected,
        [Parameter(Mandatory)][string]$Description,
        [int]$TimeoutSeconds = 15
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Path) {
            try {
                $value = (Get-Content -LiteralPath $Path -Raw).Trim()
                if ($Expected -contains $value) { return $value }
            } catch {}
        }
        Start-Sleep -Milliseconds 100
    }
    throw "$Description did not reach an expected value within $TimeoutSeconds seconds."
}

function Get-Sha256Fingerprint {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $hash = $algorithm.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text))
        return 'sha256:' + (($hash | ForEach-Object { $_.ToString('x2') }) -join '')
    } finally {
        $algorithm.Dispose()
    }
}

function Find-Element {
    param(
        [Parameter(Mandatory)][string]$Using,
        [Parameter(Mandatory)][string]$Value,
        [int]$TimeoutSeconds = 15
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $response = Invoke-Driver -Method Post -Path "/session/$sessionId/element" -Body @{
                using = $Using
                value = $Value
            }
            $elementId = $response.value.$elementKey
            if ($elementId) { return $elementId }
        } catch {
            Start-Sleep -Milliseconds 150
        }
    }
    throw "Element was not found: $Using $Value"
}

function Click-Element {
    param([Parameter(Mandatory)][string]$ElementId)
    Invoke-Driver -Method Post -Path "/session/$sessionId/element/$ElementId/click" -Body @{} | Out-Null
}

function Send-Text {
    param(
        [Parameter(Mandatory)][string]$ElementId,
        [Parameter(Mandatory)][string]$Text
    )
    Invoke-Driver -Method Post -Path "/session/$sessionId/element/$ElementId/value" -Body @{
        text = $Text
        value = @($Text.ToCharArray() | ForEach-Object { [string]$_ })
    } | Out-Null
}

function Set-PromptText {
    param([Parameter(Mandatory)][string]$Text)
    Invoke-Driver -Method Post -Path "/session/$sessionId/alert/text" -Body @{ text = $Text } | Out-Null
    Invoke-Driver -Method Post -Path "/session/$sessionId/alert/accept" -Body @{} | Out-Null
}

function Get-ElementText {
    param([Parameter(Mandatory)][string]$ElementId)
    (Invoke-Driver -Method Get -Path "/session/$sessionId/element/$ElementId/text").value
}

function Wait-ElementText {
    param(
        [Parameter(Mandatory)][string]$Using,
        [Parameter(Mandatory)][string]$Value,
        [Parameter(Mandatory)][string]$Expected,
        [int]$TimeoutSeconds = 15
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $elementId = Find-Element -Using $Using -Value $Value -TimeoutSeconds 1
            if ((Get-ElementText -ElementId $elementId) -eq $Expected) { return $elementId }
        } catch {
            Start-Sleep -Milliseconds 150
        }
    }
    throw "Element did not reach expected text '$Expected': $Using $Value"
}

try {
    New-Item -ItemType Directory -Path $appData, $libraryRoot -Force | Out-Null
    Clear-WebViewData
    $env:CAIRN_WEBDRIVER_MODE = '1'
    $env:CAIRN_APP_DATA_DIR = $appData
    $env:CAIRN_WEBDRIVER_LIBRARY_ROOT = $libraryRoot
    $env:TAURI_WEBVIEW_AUTOMATION = 'true'
    if ($Scenario -eq 'recovery') {
        $env:CAIRN_WEBDRIVER_RECOVERY_MARKER_PATH = $recoveryMarkerPath
        $env:CAIRN_WEBDRIVER_DISK_SAVE_BLOCK_PATH = $diskSaveBlockPath
    }

    Write-Host "Starting Cairn.md desktop $Scenario flow."
    Start-DriverSession

    Find-Element -Using 'css selector' -Value '.desktop-shell' | Out-Null
    $createProject = Find-Element -Using 'css selector' -Value 'button[aria-label="Create project"]'
    Click-Element -ElementId $createProject
    $projectName = if ($Scenario -eq 'recovery') { 'Recovery Project' } else { 'WebDriver Project' }
    $documentName = if ($Scenario -eq 'recovery') { 'recovery-proof.md' } else { 'desktop-proof.md' }
    Set-PromptText -Text $projectName
    Find-Element -Using 'xpath' -Value "//nav[@aria-label='Projects']//button[.//span[normalize-space()='$projectName']]" | Out-Null

    $newDocument = Find-Element -Using 'xpath' -Value "//button[normalize-space()='New document']"
    Click-Element -ElementId $newDocument
    Set-PromptText -Text $documentName
    Wait-ElementText -Using 'css selector' -Value '.document-header h2' -Expected $documentName | Out-Null

    $visualEditor = Find-Element -Using 'css selector' -Value '.visual-segment .ProseMirror[contenteditable="true"]' -TimeoutSeconds 30
    $expectedText = if ($Scenario -eq 'recovery') {
        'Recovered after forced termination in the real Cairn.md desktop window.'
    } else {
        'Edited in the real Cairn.md desktop window.'
    }
    Send-Text -ElementId $visualEditor -Text $expectedText
    Write-Host 'The editor acknowledged the complete test input.'

    if ($Scenario -eq 'recovery') {
        $acknowledgedAt = [DateTime]::UtcNow
        $expectedFingerprints = @(
            Get-Sha256Fingerprint -Text $expectedText
            Get-Sha256Fingerprint -Text "$expectedText`n"
            Get-Sha256Fingerprint -Text "$expectedText`r`n"
        )
        Wait-File -Path $diskSaveBlockPath -Description 'The deterministic disk-save block' -TimeoutSeconds 30
        Wait-FileValue -Path $recoveryMarkerPath -Expected $expectedFingerprints `
            -Description 'The complete durable recovery snapshot' -TimeoutSeconds 30 | Out-Null
        $recoveryLag = [DateTime]::UtcNow - $acknowledgedAt
        if ($recoveryLag.TotalSeconds -gt 2) {
            throw "Durable recovery exceeded two seconds after acknowledged input: $([math]::Round($recoveryLag.TotalMilliseconds)) ms"
        }
        Write-Host 'The complete recovery snapshot is durable and the disk save is blocked; forcing process termination.'
        $appProcess = Find-CairnProcess
        $termination = Start-Process -FilePath 'taskkill.exe' `
            -ArgumentList '/PID', $appProcess.Id, '/T', '/F' `
            -Wait -PassThru -WindowStyle Hidden
        if ($termination.ExitCode -ne 0) {
            throw "Forced termination exited with code $($termination.ExitCode)."
        }
        $appProcess.WaitForExit(10000) | Out-Null
        $terminationLag = [DateTime]::UtcNow - $acknowledgedAt

        Stop-DriverSession
        Remove-Item Env:CAIRN_WEBDRIVER_RECOVERY_MARKER_PATH -ErrorAction SilentlyContinue
        Remove-Item Env:CAIRN_WEBDRIVER_DISK_SAVE_BLOCK_PATH -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $recoveryMarkerPath, $diskSaveBlockPath -Force -ErrorAction SilentlyContinue
        Write-Host 'Restarting the same Cairn.md workspace to resolve recovery.'
        Start-DriverSession
        Find-Element -Using 'css selector' -Value '.desktop-shell' | Out-Null
        $documentRow = Find-Element -Using 'xpath' -Value "//*[@role='option' and .//span[normalize-space()='$documentName']]"
        Click-Element -ElementId $documentRow
        Wait-ElementText -Using 'css selector' -Value '.persistence-state span' -Expected 'Recovered' -TimeoutSeconds 30 | Out-Null
        $keepRecovery = Find-Element -Using 'xpath' -Value "//button[normalize-space()='Keep recovered changes']"
        Click-Element -ElementId $keepRecovery
        Wait-ElementText -Using 'css selector' -Value '.persistence-state span' -Expected 'Saved' -TimeoutSeconds 30 | Out-Null
    } else {
        Wait-ElementText -Using 'css selector' -Value '.persistence-state span' -Expected 'Saved' -TimeoutSeconds 30 | Out-Null
    }

    $documentRow = Find-Element -Using 'xpath' -Value "//*[@role='option' and .//span[normalize-space()='$documentName']]"
    $editorHeading = Wait-ElementText -Using 'css selector' -Value '.document-header h2' -Expected $documentName
    if (-not $documentRow -or -not $editorHeading) {
        throw 'The contents list and editor were not simultaneously available.'
    }

    $sourceButton = Find-Element -Using 'xpath' -Value "//div[contains(@class,'mode-switch')]//button[normalize-space()='Source']"
    Click-Element -ElementId $sourceButton
    $sourceEditor = Find-Element -Using 'css selector' -Value '.source-editor .cm-content[contenteditable="true"]' -TimeoutSeconds 30
    $sourceText = Get-ElementText -ElementId $sourceEditor
    if ($sourceText -notlike "*$expectedText*") {
        throw "Source mode did not contain the visual edit. Actual text: $sourceText"
    }

    $savedPath = Join-Path (Join-Path $libraryRoot $projectName) $documentName
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline -and -not (Test-Path -LiteralPath $savedPath)) {
        Start-Sleep -Milliseconds 150
    }
    if (-not (Test-Path -LiteralPath $savedPath)) { throw 'Autosave did not create the Markdown file.' }
    $savedText = Get-Content -LiteralPath $savedPath -Raw
    if ($savedText -notlike "*$expectedText*") {
        throw 'The saved Markdown file did not contain the visual edit.'
    }

    if ($Scenario -eq 'recovery') {
        Write-Host "Recovery flow passed: the complete edit reached durable recovery in $([math]::Round($recoveryLag.TotalMilliseconds)) ms; forced termination completed after $([math]::Round($terminationLag.TotalMilliseconds)) ms and the document returned to Saved."
    } else {
        Write-Host 'Desktop flow passed: project, document, visual edit, autosave, source mode, and persistent contents navigation.'
    }
} catch {
    $failureMessage = $_.Exception.Message
    Write-Warning "Desktop flow failed: $failureMessage"
    $annotationMessage = $failureMessage.Replace('%', '%25').Replace("`r", '%0D').Replace("`n", '%0A')
    Write-Host "::error title=Cairn.md desktop $Scenario flow failed::$annotationMessage"
    if (Test-Path -LiteralPath $stdoutPath) {
        Write-Host '--- Microsoft Edge WebDriver stdout ---'
        Get-Content -LiteralPath $stdoutPath
    }
    if (Test-Path -LiteralPath $stderrPath) {
        Write-Host '--- Microsoft Edge WebDriver stderr ---'
        Get-Content -LiteralPath $stderrPath
    }
    throw
} finally {
    Stop-DriverSession
    Remove-Item Env:CAIRN_WEBDRIVER_MODE -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_APP_DATA_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_WEBDRIVER_LIBRARY_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_WEBDRIVER_RECOVERY_MARKER_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_WEBDRIVER_DISK_SAVE_BLOCK_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_WEBVIEW_AUTOMATION -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $recoveryMarkerPath, $diskSaveBlockPath -Force -ErrorAction SilentlyContinue
    Clear-WebViewData -BestEffort
    $resolvedBase = [IO.Path]::GetFullPath($testBase)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolvedBase.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolvedBase).StartsWith('cairn-webdriver-', [StringComparison]::Ordinal)) {
        for ($attempt = 0; $attempt -lt 5; $attempt += 1) {
            try {
                Remove-Item -LiteralPath $resolvedBase -Recurse -Force -ErrorAction Stop
                break
            } catch {
                if ($attempt -eq 4) { Write-Warning "Could not remove WebDriver temp folder: $resolvedBase" }
                Start-Sleep -Milliseconds 250
            }
        }
    }
}
