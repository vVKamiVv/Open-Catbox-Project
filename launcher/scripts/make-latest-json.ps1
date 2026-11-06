<#
.SYNOPSIS
  Prepares the release assets of the launcher for GitHub and writes the updater's latest.json.

.DESCRIPTION
  Run after a signed `cargo tauri build` (TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD set).
  Copies the NSIS installer and its .sig into -OutDir under names without spaces
  (OpenCatbox_<version>_x64-setup.exe and .sig) and writes latest.json in the Tauri v2 updater format, pointing at
  the asset of the GitHub release tagged v<version>. Upload all three files to that release; the launcher reads
  releases/latest/download/latest.json.

.EXAMPLE
  .\launcher\scripts\make-latest-json.ps1 -Version 0.1.0 -NotesFile CHANGELOG.md -OutDir release
  With CHANGELOG.md (or any file with a "## v<version>" heading) only that version's section becomes the notes.
#>
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $NotesFile,
  [string] $BundleDir = (Join-Path $PSScriptRoot "..\..\target\release\bundle\nsis"),
  [string] $OutDir = ".",
  [string] $Repo = "vVKamiVv/Open-Catbox-Project"
)
$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') { throw "Version must look like 0.1.0, not '$Version'." }

$installer = Get-ChildItem -LiteralPath $BundleDir -Filter "*_${Version}_x64-setup.exe" | Select-Object -First 1
if (-not $installer) { throw "No *_${Version}_x64-setup.exe in $BundleDir. Build with: cargo tauri build" }
$sig = "$($installer.FullName).sig"
if (-not (Test-Path -LiteralPath $sig)) { throw "No $sig. Set TAURI_SIGNING_PRIVATE_KEY(_PASSWORD) and build again." }

# a "## v<version>" section of a changelog, else the whole file
$text = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $NotesFile))
$section = [regex]::Match($text, "(?ms)^## v$([regex]::Escape($Version))\b[^\n]*\n(.*?)(?=^## |\z)")
$notes = $(if ($section.Success) { $section.Groups[1].Value } else { $text }).Trim()

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$asset = "OpenCatbox_${Version}_x64-setup.exe"
Copy-Item -LiteralPath $installer.FullName -Destination (Join-Path $OutDir $asset) -Force
Copy-Item -LiteralPath $sig -Destination (Join-Path $OutDir "$asset.sig") -Force

$latest = [ordered]@{
  version   = $Version
  notes     = $notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = ([IO.File]::ReadAllText($sig)).Trim()
      url       = "https://github.com/$Repo/releases/download/v$Version/$asset"
    }
  }
}
$json = Join-Path (Resolve-Path -LiteralPath $OutDir) "latest.json"
# UTF-8 without BOM: the updater's JSON parser rejects a BOM
[IO.File]::WriteAllText($json, ($latest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
Write-Output "Wrote $json"
Write-Output "Upload to release v${Version}: $asset, $asset.sig, latest.json"
