<#
.SYNOPSIS
    Configures Windows Terminal to launch BBS Launcher as the default profile.
#>

param(
    [string]$BbsPath = "$(Get-Location)\target\release\bbs-launcher.exe"
)

$settingsPath = "$env:LOCALAPPDATA\Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"

if (-not (Test-Path $settingsPath)) {
    Write-Host "Windows Terminal settings not found at: $settingsPath" -ForegroundColor Yellow
    $settingsPath = Read-Host "Enter path to your Windows Terminal settings.json"
    if (-not (Test-Path $settingsPath)) {
        Write-Host "Settings file not found. Exiting." -ForegroundColor Red
        exit 1
    }
}

$json = Get-Content $settingsPath -Raw | ConvertFrom-Json

$profileName = "BBS Launcher"
$guid = [guid]::NewGuid().ToString().ToUpper()

$newProfile = [ordered]@{
    name = $profileName
    commandline = $BbsPath
    startingDirectory = $env:USERPROFILE
    hidden = $false
    useAcrylic = $true
    acrylicOpacity = 0.85
    colorScheme = "BBS Dark"
}

if (-not $json.profiles.list) {
    $json.profiles | Add-Member -NotePropertyName "list" -NotePropertyValue @()
}

$json.profiles.list += $newProfile | ConvertTo-Json -Compress | ConvertFrom-Json

if (-not $json.schemes) {
    $json | Add-Member -NotePropertyName "schemes" -NotePropertyValue @()
}

$darkScheme = [ordered]@{
    name = "BBS Dark"
    background = "#0c0c0c"
    foreground = "#00ff41"
    black = "#0c0c0c"
    red = "#ff3333"
    green = "#00ff41"
    yellow = "#ffff00"
    blue = "#0066ff"
    magenta = "#ff00ff"
    cyan = "#00ffff"
    white = "#cccccc"
    brightBlack = "#666666"
    brightRed = "#ff6666"
    brightGreen = "#66ff66"
    brightYellow = "#ffff66"
    brightBlue = "#6699ff"
    brightMagenta = "#ff66ff"
    brightCyan = "#66ffff"
    brightWhite = "#ffffff"
}

$json.schemes += $darkScheme | ConvertTo-Json -Compress | ConvertFrom-Json

$json | ConvertTo-Json -Depth 10 | Set-Content $settingsPath -Encoding UTF8

Write-Host ""
Write-Host "BBS Launcher profile added to Windows Terminal!" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Open Windows Terminal settings (Ctrl+,)"
Write-Host "  2. Set 'Default profile' to '$profileName'"
Write-Host "  3. Restart Windows Terminal"
Write-Host ""
Write-Host "Profile GUID: $guid" -ForegroundColor Yellow
Write-Host "To set via JSON, add to your settings:" -ForegroundColor Yellow
Write-Host "  `"defaultProfile`": `"$guid`""
Write-Host ""
