# Publica o instalador gerado em dist\ como pacote e Release do GitLab.
# Roda no CI (usa CI_JOB_TOKEN) quando uma tag vX.Y.Z e enviada.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

$Tag = $env:CI_COMMIT_TAG
if (-not $Tag) { throw "CI_COMMIT_TAG ausente: este script so roda para tags." }
$Version = $Tag.TrimStart("v")
$Exe = Join-Path $Root "dist\Extrator-de-Texto-Setup-$Version.exe"
if (-not (Test-Path -LiteralPath $Exe)) { throw "Instalador nao encontrado: $Exe (a tag $Tag combina com VERSION.txt?)" }

$Api = $env:CI_API_V4_URL
$Project = $env:CI_PROJECT_ID
$Headers = @{ "JOB-TOKEN" = $env:CI_JOB_TOKEN }
$Name = Split-Path -Leaf $Exe
$PackageUrl = "$Api/projects/$Project/packages/generic/extrator-de-texto/$Version/$Name"

Invoke-RestMethod -Method Put -Uri $PackageUrl -Headers $Headers -InFile $Exe -ContentType "application/octet-stream" | Out-Null
Write-Host "Pacote enviado: $PackageUrl"

# Notas da Release: trecho do CHANGELOG.md para esta versao, se existir.
$Notes = "Instalador do Extrator de Texto $Version."
$Changelog = Join-Path $Root "CHANGELOG.md"
if (Test-Path -LiteralPath $Changelog) {
    $Text = Get-Content -LiteralPath $Changelog -Raw -Encoding UTF8
    if ($Text -match "(?ms)^## \[$([regex]::Escape($Version))\][^\r\n]*\r?\n(.*?)(?=^## \[|\z)") {
        $Notes = $Matches[1].Trim()
    }
}

$Body = @{
    name        = "Versao $Version"
    tag_name    = $Tag
    description = $Notes
    assets      = @{ links = @(@{ name = $Name; url = $PackageUrl; link_type = "package" }) }
} | ConvertTo-Json -Depth 5

try {
    Invoke-RestMethod -Method Post -Uri "$Api/projects/$Project/releases" -Headers $Headers -ContentType "application/json; charset=utf-8" -Body ([Text.Encoding]::UTF8.GetBytes($Body)) | Out-Null
    Write-Host "Release criada para $Tag."
} catch {
    throw "Falha ao criar a Release: $($_.Exception.Message)"
}
