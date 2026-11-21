#Requires -Version 5.1
<#
  lycodl one-line installer for Windows.
  Usage (PowerShell):
    irm https://raw.githubusercontent.com/lilyco-42/lycodl/master/install/install.ps1 | iex
    .\install.ps1 -Version v0.1.1
#>
param(
  [string]$Version = "latest",
  [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"
$repo = "lilyco-42/lycodl"
$asset = "lycodl-windows-x64.zip"

if ($Version -eq "latest") {
  $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
  $url = "https://github.com/$repo/releases/download/$Version/$asset"
}

$zip = Join-Path $env:TEMP $asset
$dest = Join-Path $env:LOCALAPPDATA "Programs\lycodl"

Write-Host "downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

Write-Host "installing into $dest"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
Expand-Archive -Path $zip -DestinationPath $dest -Force
Remove-Item -Force $zip

$exe = Get-ChildItem -Path $dest -Filter "lycodl.exe" -Recurse | Select-Object -First 1
if (-not $exe) { throw "lycodl.exe not found inside the package" }

if (-not $NoShortcut) {
  $shell = New-Object -ComObject WScript.Shell
  $dirs = @(
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
    [Environment]::GetFolderPath("Desktop")
  )
  foreach ($dir in $dirs) {
    if (-not (Test-Path $dir)) { continue }
    $saved = $false
    foreach ($name in @("lycodl 视频下载器", "lycodl")) {
      try {
        $lnk = $shell.CreateShortcut((Join-Path $dir ($name + ".lnk")))
        $lnk.TargetPath = $exe.FullName
        $lnk.Arguments = "--web"
        $lnk.WorkingDirectory = $exe.DirectoryName
        $lnk.Description = "lycodl - video downloader and publisher"
        $lnk.Save()
        $saved = $true
        break
      } catch { }
    }
    if (-not $saved) { Write-Warning "could not create a shortcut in $dir" }
  }
}

$driver = Join-Path $dest "publish-driver\platforms.mjs"
if (-not (Test-Path $driver)) {
  Write-Warning "publish-driver is missing from the package; multi-platform publishing will not start."
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  Write-Warning "Node.js not found: downloads work, but multi-platform publishing needs Node 18+ (publish-driver is Node)."
}

Write-Host ""
Write-Host "installed: $($exe.FullName)"
Write-Host "run:       `"$($exe.FullName)`" --web"
Write-Host "publish:   set the browser CDP URL in Settings (Chrome must run with --remote-debugging-port=9333)"