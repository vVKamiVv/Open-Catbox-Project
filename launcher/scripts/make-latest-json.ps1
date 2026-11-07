<#
.SYNOPSIS
  Writes the updater's latest.json for a launcher release.

.DESCRIPTION
  Run by release.ps1, after OpenCatbox-Setup.exe and its .sig are in -OutDir. Writes latest.json in the Tauri v2
  updater format, pointing at OpenCatbox-Setup.exe of the GitHub release tagged v<version>. Upload the Setup, its .sig
  and latest.json to that release; launchers read releases/latest/download/latest.json.

.EXAMPLE
  .\launcher\scripts\make-latest-json.ps1 -Version 0.2.0 -NotesFile CHANGELOG.md -OutDir release
  With CHANGELOG.md (or any file with a "## v<version>" heading) only that version's section becomes the notes.
#>
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $NotesFile,
  [Parameter(Mandatory)] [string] $OutDir,
  [string] $Repo = "vVKamiVv/Open-Catbox-Project"
)
$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') { throw "Version must look like 0.1.0, not '$Version'." }

$asset = "OpenCatbox-Setup.exe"
$sig = Join-Path $OutDir "$asset.sig"
if (-not (Test-Path -LiteralPath $sig)) { throw "No $sig. Build the release with release.ps1." }

# a "## v<version>" section of a changelog, else the whole file
$text = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $NotesFile))
$section = [regex]::Match($text, "(?ms)^## v$([regex]::Escape($Version))\b[^\n]*\n(.*?)(?=^## |\z)")
$notes = $(if ($section.Success) { $section.Groups[1].Value } else { $text }).Trim()

$latest = [ordered]@{
  version   = $Version
  notes     = $notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = ([IO.File]::ReadAllText((Resolve-Path -LiteralPath $sig))).Trim()
      url       = "https://github.com/$Repo/releases/download/v$Version/$asset"
    }
  }
}
$json = Join-Path (Resolve-Path -LiteralPath $OutDir) "latest.json"
# UTF-8 without BOM: the updater's JSON parser rejects a BOM
[IO.File]::WriteAllText($json, ($latest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
Write-Output "Wrote $json"
