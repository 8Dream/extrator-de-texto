$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

# Versao vem do VERSION.txt ("Versao 1.3.0"), a unica fonte da verdade.
$Line = Get-Content -LiteralPath (Join-Path $Root "VERSION.txt") | Where-Object { $_ -match "^Versao\s+(\S+)" } | Select-Object -First 1
if (-not $Line) { throw "VERSION.txt sem linha 'Versao X.Y.Z'." }
$Version = ($Line -split "\s+")[1]

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
