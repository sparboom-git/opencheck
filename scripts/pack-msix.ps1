<#
.SYNOPSIS
  Packs OpenCheck as an MSIX for the Microsoft Store.

.DESCRIPTION
  Fills in packaging/msix/AppxManifest.xml with the identity from
  packaging/msix/identity.json and the version from Cargo.toml (0.3.0 becomes
  0.3.0.0: the Store keeps the fourth number for itself, so it is always 0),
  lays out the app with its logos and packs it with makeappx from the Windows
  SDK. The package is not signed: the Store signs it.

  Run from the repository root after `cargo build --release -p opencheck-app`.

.PARAMETER Out
  The .msix file to write; by default OpenCheck-v<version>-x64.msix.
#>
param(
    [string]$Out = "",
    [string]$Exe = "target/release/opencheck-app.exe",
    [string]$IdentityFile = "packaging/msix/identity.json"
)
$ErrorActionPreference = "Stop"

# PowerShell names ignore case: the file and the identity read from it need
# different names.
$identity = Get-Content $IdentityFile -Raw | ConvertFrom-Json
if (-not $identity.name -or -not $identity.publisher -or -not $identity.publisher_display_name) {
    Write-Warning "$IdentityFile is not filled in: no MSIX made."
    exit 0
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(\d+)\.(\d+)\.(\d+)"' | Select-Object -First 1).Matches.Groups
if (-not $version) { throw "No version of the form x.y.z in Cargo.toml" }
$msixVersion = "$($version[1].Value).$($version[2].Value).$($version[3].Value).0"
if (-not $Out) { $Out = "OpenCheck-v$($version[1].Value).$($version[2].Value).$($version[3].Value)-x64.msix" }

# makeappx.exe from the newest Windows SDK.
$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if ($env:MAKEAPPX) { $makeappx = Get-Item $env:MAKEAPPX }
if (-not $makeappx) { throw "makeappx.exe not found: install the Windows SDK, or set MAKEAPPX to its path" }

$layout = Join-Path ([System.IO.Path]::GetTempPath()) "opencheck-msix-layout"
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Path $layout | Out-Null
Copy-Item $Exe -Destination (Join-Path $layout "opencheck-app.exe")
Copy-Item packaging/msix/Assets -Destination (Join-Path $layout "Assets") -Recurse
Copy-Item LICENSE-MIT, LICENSE-APACHE -Destination $layout
Copy-Item crates/opencheck-app/assets/fonts/OFL.txt -Destination (Join-Path $layout "OFL-fonts.txt")

$escape = { param($s) [System.Security.SecurityElement]::Escape($s) }
$manifest = (Get-Content packaging/msix/AppxManifest.xml -Raw).
    Replace("{{NAME}}", (& $escape $identity.name)).
    Replace("{{PUBLISHER}}", (& $escape $identity.publisher)).
    Replace("{{PUBLISHER_DISPLAY_NAME}}", (& $escape $identity.publisher_display_name)).
    Replace("{{VERSION}}", $msixVersion)
Set-Content -Path (Join-Path $layout "AppxManifest.xml") -Value $manifest -Encoding utf8

& $makeappx.FullName pack /d $layout /p $Out /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed" }
Write-Host "Wrote $Out (package version $msixVersion)"
