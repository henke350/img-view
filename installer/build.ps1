# Builds the release .exe and the Windows installer.
# Run from anywhere:  powershell -ExecutionPolicy Bypass -File installer\build.ps1
# Needs Rust (cargo) and Inno Setup 6 (winget install JRSoftware.InnoSetup).
# Output: target\installer\img-view-setup-<version>.exe

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# Release build of the app.
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' } else { $cargo = $cargo.Source }
& $cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# The installer version comes from Cargo.toml, so there is one place to bump it.
$match = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $match) { throw 'no version found in Cargo.toml' }
$version = $match.Matches[0].Groups[1].Value

# Inno Setup's compiler: on PATH, or in one of its usual install folders.
$iscc = Get-Command ISCC -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $iscc) {
    $iscc = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw 'Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup' }

& $iscc /Qp "/DAppVersion=$version" (Join-Path $PSScriptRoot 'img-view.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }

Get-Item (Join-Path $repo "target\installer\img-view-setup-$version.exe")
