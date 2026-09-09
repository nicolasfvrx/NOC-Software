[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('noc-manager', 'noc-display')]
    [string]$App,
    [ValidateSet('2012', '2016')]
    [string]$Server = '2012'
)

$ErrorActionPreference = 'Stop'
$nocRoot = Split-Path -Parent $PSScriptRoot
$project = Join-Path $nocRoot $App
$target = 'x86_64-pc-windows-msvc'
$label = "$App-windows-server-$Server-x64"
$dist = Join-Path $nocRoot 'dist'
# A fresh staging directory avoids shipping files left over from an earlier build.
$stage = Join-Path ([IO.Path]::GetTempPath()) ("noc-package-" + [Guid]::NewGuid().ToString('N'))
$package = Join-Path $stage $label

Push-Location $project
try {
    # Running inside the project is required to load its .cargo/config.toml.
    if ($env:RUSTFLAGS -or $env:CARGO_ENCODED_RUSTFLAGS -or $env:CARGO_BUILD_RUSTFLAGS -or
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS) {
        throw 'Remove custom Rust flags before building the compatibility packages.'
    }
    & cargo +1.77.2 build --release --locked --target $target
    if ($LASTEXITCODE -ne 0) { throw "Compilation failed: $App" }
    $binary = Join-Path $project "target/$target/release/$App.exe"
    $expectedName = if ($App -eq 'noc-manager') { 'NOC Manager' } else { 'NOC Display' }
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($binary)
    if ($version.ProductName -ne $expectedName -or $version.FileDescription -ne $expectedName -or
        $version.CompanyName -ne 'Norfair Operation Center') {
        throw 'Missing or incorrect Windows application metadata.'
    }

    # Inspect actual imports as an additional regression check for Server 2012.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vs) { throw 'Visual Studio C++ Build Tools not found.' }
    $dumpbin = Get-ChildItem -Path (Join-Path $vs 'VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe') |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $dumpbin) { throw 'dumpbin.exe not found.' }
    $imports = & $dumpbin.FullName /imports $binary
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect executable imports.' }
    if (($imports -join "`n") -match '(?i)bcryptprimitives\.dll|\bProcessPrng\b|\bSetThreadDescription\b|VCRUNTIME\d+.*\.dll|MSVCP\d+.*\.dll') {
        throw 'Executable imports a runtime or API outside the common compatibility baseline.'
    }
    if ($App -eq 'noc-manager') {
        & $binary --version
        if ($LASTEXITCODE -ne 0) { throw 'Version smoke test failed.' }
    }

    New-Item -ItemType Directory -Path $package -Force | Out-Null
    Copy-Item -LiteralPath $binary -Destination $package
    Copy-Item -LiteralPath (Join-Path $project 'config.example.toml') -Destination $package
    Copy-Item -LiteralPath (Join-Path $project 'README.md') -Destination $package
    Copy-Item -LiteralPath (Join-Path $nocRoot 'doc/windows.md') -Destination $package
    if ($App -eq 'noc-display') {
        foreach ($asset in @('background.jpg', 'logo.png')) {
            Copy-Item -LiteralPath (Join-Path $project $asset) -Destination $package
        }
    } else {
        foreach ($example in @('kiosks.example.json', 'commands.example.json')) {
            Copy-Item -LiteralPath (Join-Path $project $example) -Destination $package
        }
    }
    $rustVersion = & rustc +1.77.2 --version
    @("Application: $expectedName", "Version: $($version.ProductVersion)",
      "Target: Windows Server $Server x64", "Compiler: $rustVersion",
      "Source: $env:GITHUB_SHA", 'Common baseline: Rust 1.77.2, static MSVC CRT',
      'Native validation on the target Windows Server is still required.') |
        Set-Content -LiteralPath (Join-Path $package 'BUILD-INFO.txt') -Encoding UTF8
    $hash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $App.exe" | Set-Content -LiteralPath (Join-Path $package 'SHA256SUMS.txt') -Encoding ASCII
    New-Item -ItemType Directory -Path $dist -Force | Out-Null
    Compress-Archive -LiteralPath $package -DestinationPath (Join-Path $dist "$label.zip") -Force
    Write-Host "Package: $dist/$label.zip"
} finally {
    Pop-Location
}
