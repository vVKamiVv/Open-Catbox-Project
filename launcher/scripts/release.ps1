<#
.SYNOPSIS
  Builds a launcher release: OpenCatbox-Setup.exe, its updater signature, latest.json and SHA-256 sums.

.DESCRIPTION
  OpenCatbox-Setup.exe is the launcher with shin.exe inside: run, it installs itself (see launcher/src/install.rs).

  1. shin.exe, then the launcher with the "installer" feature (which embeds that shin.exe), are built with the dist
     profile into target\installer, so normal builds are untouched. They carry their own C runtime (+crt-static), so a
     PC needs nothing installed first, and build paths are rewritten (--remap-path-prefix), so no folder or user name
     of this PC goes out with them: both binaries are searched for those and any hit stops the release.
  2. The Setup is signed for the updater with the key in -KeyDir (updater.key and updater.key.password). They are read
     into environment variables for the signing command only and never printed.
  3. make-latest-json.ps1 writes latest.json for the GitHub release v<version>, then SHA256SUMS.txt lists the files.

  The version is the one in launcher/Cargo.toml; the notes are its "## v<version>" section of CHANGELOG.md.
  Upload OpenCatbox-Setup.exe, OpenCatbox-Setup.exe.sig, latest.json and SHA256SUMS.txt to the release v<version>.

.EXAMPLE
  .\launcher\scripts\release.ps1 -OutDir release
#>
param(
  [Parameter(Mandatory)] [string] $OutDir,
  [string] $NotesFile,
  [string] $KeyDir = (Join-Path $env:USERPROFILE ".open-catbox")
)
$ErrorActionPreference = "Stop"
# Windows PowerShell has no $PSScriptRoot yet in parameter defaults
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $NotesFile) { $NotesFile = Join-Path $root "CHANGELOG.md" }

# cargo reports progress on stderr, which Windows PowerShell turns into errors when output is redirected
function Invoke-Native([string] $What, [scriptblock] $Block) {
  $eap = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try { & $Block } finally { $ErrorActionPreference = $eap }
  if ($LASTEXITCODE) { throw "$What failed (exit code $LASTEXITCODE)." }
}

$manifest = [IO.File]::ReadAllText((Join-Path $root "launcher\Cargo.toml"))
$version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$key = Join-Path $KeyDir "updater.key"
$password = Join-Path $KeyDir "updater.key.password"
foreach ($f in $key, $password) { if (-not (Test-Path -LiteralPath $f)) { throw "The updater key is missing: $f" } }

$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE ".rustup" }
$targetDir = Join-Path $root "target\installer"
$built = Join-Path $targetDir "dist"

# CARGO_ENCODED_RUSTFLAGS (separated by 0x1F) keeps the spaces in this checkout's path
$flags = @(
  "-Ctarget-feature=+crt-static",
  "--remap-path-prefix=$root=open-catbox",
  "--remap-path-prefix=$cargoHome=cargo",
  "--remap-path-prefix=$rustupHome=rustup"
)
$saved = $env:CARGO_ENCODED_RUSTFLAGS
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1F
try {
  Push-Location $root
  Invoke-Native "Building shin" { cargo build --profile dist --target-dir $targetDir -p shin }
  $env:OPEN_CATBOX_SHIN = Join-Path $built "shin.exe"
  Invoke-Native "Building the launcher" { cargo build --profile dist --target-dir $targetDir -p open-catbox-launcher --features installer }
} finally {
  Pop-Location
  $env:CARGO_ENCODED_RUSTFLAGS = $saved
  Remove-Item Env:OPEN_CATBOX_SHIN -ErrorAction SilentlyContinue
}

# nothing of this PC in what goes out: its user name or the checkout's path, as ASCII or UTF-16
$latin1 = [Text.Encoding]::GetEncoding(28591)
# the user name as written (ignoring case, short names hide in longer words), paths in any case
$needles = @(
  @($env:USERNAME, [StringComparison]::Ordinal),
  @($root, [StringComparison]::OrdinalIgnoreCase),
  @($root.Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase)
)
foreach ($exe in (Join-Path $built "shin.exe"), (Join-Path $built "open-catbox-launcher.exe")) {
  $text = $latin1.GetString([IO.File]::ReadAllBytes($exe))
  foreach ($needle in $needles) {
    $n = $needle[0]
    foreach ($form in $n, (($n.ToCharArray() | ForEach-Object { "$_`0" }) -join '')) {
      if ($text.IndexOf($form, $needle[1]) -ge 0) { throw "$exe holds '$n' (this PC's user name or path)." }
    }
  }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$setup = Join-Path (Resolve-Path -LiteralPath $OutDir) "OpenCatbox-Setup.exe"
Copy-Item -LiteralPath (Join-Path $built "open-catbox-launcher.exe") -Destination $setup -Force

try {
  $env:TAURI_SIGNING_PRIVATE_KEY_PATH = $key
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [IO.File]::ReadAllText($password)
  Invoke-Native "Signing" { cargo tauri signer sign --app-version $version $setup | Out-Null }
} finally {
  Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PATH, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}

& (Join-Path $PSScriptRoot "make-latest-json.ps1") -Version $version -NotesFile $NotesFile -OutDir $OutDir

$files = "OpenCatbox-Setup.exe", "OpenCatbox-Setup.exe.sig", "latest.json"
$sums = $files | ForEach-Object { "$((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $OutDir $_)).Hash.ToLower())  $_" }
[IO.File]::WriteAllText((Join-Path (Resolve-Path -LiteralPath $OutDir) "SHA256SUMS.txt"), ($sums -join "`n") + "`n", (New-Object Text.UTF8Encoding $false))
Write-Output "Release v$version in $OutDir`: $($files -join ', '), SHA256SUMS.txt. Upload them to the GitHub release v$version."
