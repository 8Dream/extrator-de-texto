$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

# Versao vem do VERSION.txt ("Versao 1.3.0"), a unica fonte da verdade.
$Line = Get-Content -LiteralPath (Join-Path $Root "VERSION.txt") | Where-Object { $_ -match "^Versao\s+(\S+)" } | Select-Object -First 1
if (-not $Line) { throw "VERSION.txt sem linha 'Versao X.Y.Z'." }
$Version = ($Line -split "\s+")[1]

# Extrator de Texto.exe: interface nativa em Rust (ui\), compilada com o cargo.
# Versao, icone, manifesto e informacoes do arquivo sao embutidos pelo ui\build.rs
# (a versao vem do VERSION.txt). Nao depende de WebView2 nem de Python para abrir.
$Cargo = @((Get-Command cargo.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
           (Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe")) |
    Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $Cargo) { throw "cargo nao encontrado (instale o Rust: https://rustup.rs)." }
$env:PATH = (Split-Path -Parent $Cargo) + ";" + $env:PATH
# Toolchain GNU (ui\rust-toolchain.toml): precisa do gcc e do windres do MinGW-w64 (WinLibs, open source).
if (-not (Get-Command windres.exe -ErrorAction SilentlyContinue)) {
    $Mingw = Get-ChildItem (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages") -Filter "BrechtSanders.WinLibs*" -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "mingw64\bin" } | Where-Object { Test-Path (Join-Path $_ "windres.exe") } | Select-Object -First 1
    if (-not $Mingw) { throw "MinGW-w64 nao encontrado (winget install BrechtSanders.WinLibs.POSIX.UCRT)." }
    $env:PATH = "$Mingw;$env:PATH"
}
$BuildDir = Join-Path $Root "build"
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
$UiDir = Join-Path $Root "ui"
# O cargo escreve o progresso no stderr: sem "Stop" so durante a compilacao.
$Antes = $ErrorActionPreference; $ErrorActionPreference = "Continue"
Push-Location $UiDir   # o rust-toolchain.toml de ui\ escolhe o toolchain GNU
& $Cargo build --release --locked 2>&1 | ForEach-Object { "$_" } | Write-Host
$CargoCode = $LASTEXITCODE
Pop-Location
$ErrorActionPreference = $Antes
if ($CargoCode -ne 0) { throw "Falha ao compilar a interface (cargo build --release)." }
$LauncherExe = Join-Path $BuildDir "Extrator de Texto.exe"
Copy-Item -LiteralPath (Join-Path $UiDir "target\release\extrator.exe") -Destination $LauncherExe -Force
$Candidates = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
if (-not $Candidates) { throw "Inno Setup 6 nao encontrado (winget install JRSoftware.InnoSetup)." }

& ($Candidates | Select-Object -First 1) "/DAppVersion=$Version" (Join-Path $PSScriptRoot "Extrator.iss")
if ($LASTEXITCODE -ne 0) { throw "Falha ao compilar o instalador." }
Write-Host "Gerado: dist\Extrator-de-Texto-Setup-$Version.exe" -ForegroundColor Green
