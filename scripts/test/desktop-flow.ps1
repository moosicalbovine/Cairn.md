param([string]$BinaryPath = "src-tauri\target\debug\cairn-md.exe")

$ErrorActionPreference = 'Stop'
$port = 4444
$elementKey = 'element-6066-11e4-a52e-4f735466cecf'
$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
$driverCommand = Get-Command 'tauri-driver' -ErrorAction Stop
$testBase = Join-Path ([IO.Path]::GetTempPath()) ("cairn-webdriver-" + [Guid]::NewGuid().ToString('N'))
$appData = Join-Path $testBase 'app-data'
$libraryRoot = Join-Path $testBase 'library'
$stdoutPath = Join-Path $testBase 'tauri-driver.stdout.log'
$stderrPath = Join-Path $testBase 'tauri-driver.stderr.log'
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
        TimeoutSec = 15
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
    throw 'tauri-driver did not become ready within 30 seconds.'
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
    $env:CAIRN_WEBDRIVER_MODE = '1'
    $env:CAIRN_APP_DATA_DIR = $appData
    $env:CAIRN_WEBDRIVER_LIBRARY_ROOT = $libraryRoot

    $driver = Start-Process -FilePath $driverCommand.Source `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath `
        -WindowStyle Hidden -PassThru
    Wait-Driver

    $session = Invoke-Driver -Method Post -Path '/session' -Body @{
        capabilities = @{
            alwaysMatch = @{
                'tauri:options' = @{ application = $resolvedBinary }
            }
        }
    }
    $sessionId = $session.value.sessionId
    if (-not $sessionId) { $sessionId = $session.sessionId }
    if (-not $sessionId) { throw 'WebDriver did not return a session id.' }

    Find-Element -Using 'css selector' -Value '.desktop-shell' | Out-Null
    $createProject = Find-Element -Using 'css selector' -Value 'button[aria-label="Create project"]'
    Click-Element -ElementId $createProject
    Set-PromptText -Text 'WebDriver Project'
    Find-Element -Using 'xpath' -Value "//nav[@aria-label='Projects']//button[.//span[normalize-space()='WebDriver Project']]" | Out-Null

    $newDocument = Find-Element -Using 'xpath' -Value "//button[normalize-space()='New document']"
    Click-Element -ElementId $newDocument
    Set-PromptText -Text 'desktop-proof.md'
    Wait-ElementText -Using 'css selector' -Value '.document-header h2' -Expected 'desktop-proof.md' | Out-Null

    $visualEditor = Find-Element -Using 'css selector' -Value '.visual-segment .ProseMirror[contenteditable="true"]' -TimeoutSeconds 30
    Send-Text -ElementId $visualEditor -Text 'Edited in the real Cairn.md desktop window.'
    Wait-ElementText -Using 'css selector' -Value '.persistence-state span' -Expected 'Saving…' | Out-Null
    Wait-ElementText -Using 'css selector' -Value '.persistence-state span' -Expected 'Saved' -TimeoutSeconds 30 | Out-Null

    $documentRow = Find-Element -Using 'xpath' -Value "//*[@role='option' and .//span[normalize-space()='desktop-proof.md']]"
    $editorHeading = Wait-ElementText -Using 'css selector' -Value '.document-header h2' -Expected 'desktop-proof.md'
    if (-not $documentRow -or -not $editorHeading) {
        throw 'The contents list and editor were not simultaneously available.'
    }

    $sourceButton = Find-Element -Using 'xpath' -Value "//div[contains(@class,'mode-switch')]//button[normalize-space()='Source']"
    Click-Element -ElementId $sourceButton
    $sourceEditor = Find-Element -Using 'css selector' -Value '.source-editor .cm-content[contenteditable="true"]' -TimeoutSeconds 30
    $sourceText = Get-ElementText -ElementId $sourceEditor
    if ($sourceText -notlike '*Edited in the real Cairn.md desktop window.*') {
        throw "Source mode did not contain the visual edit. Actual text: $sourceText"
    }

    $savedPath = Join-Path (Join-Path $libraryRoot 'WebDriver Project') 'desktop-proof.md'
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline -and -not (Test-Path -LiteralPath $savedPath)) {
        Start-Sleep -Milliseconds 150
    }
    if (-not (Test-Path -LiteralPath $savedPath)) { throw 'Autosave did not create the Markdown file.' }
    $savedText = Get-Content -LiteralPath $savedPath -Raw
    if ($savedText -notlike '*Edited in the real Cairn.md desktop window.*') {
        throw 'The saved Markdown file did not contain the visual edit.'
    }

    Write-Host 'Desktop flow passed: project, document, visual edit, autosave, source mode, and persistent contents navigation.'
} finally {
    if ($sessionId) {
        try { Invoke-Driver -Method Delete -Path "/session/$sessionId" | Out-Null } catch {}
    }
    if ($driver -and -not $driver.HasExited) {
        Stop-Process -Id $driver.Id -Force -ErrorAction SilentlyContinue
        $driver.WaitForExit(5000)
    }
    Remove-Item Env:CAIRN_WEBDRIVER_MODE -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_APP_DATA_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:CAIRN_WEBDRIVER_LIBRARY_ROOT -ErrorAction SilentlyContinue
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
