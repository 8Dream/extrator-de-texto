param([string]$LogFile, [string]$DoneFile)
# Modo do instalador (Inno Setup): reexecuta este script como filho e grava a
# saida linha a linha em $LogFile, para o instalador ler ao vivo sem travar a
# janela. No fim grava o codigo de saida em $DoneFile. Sem parametros, o script
# se comporta normalmente (Instalar.bat).
if ($LogFile) {
    $Code = 1
    try {
        $LogDir = Split-Path -Parent $LogFile
        if ($LogDir -and -not (Test-Path -LiteralPath $LogDir)) { New-Item -ItemType Directory -Path $LogDir -Force | Out-Null }
        Set-Content -LiteralPath $LogFile -Value $null -Encoding Default -ErrorAction SilentlyContinue
        & "$PSHOME\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $PSCommandPath *>&1 | ForEach-Object {
            $Line = "$_"
            $Deadline = [Diagnostics.Stopwatch]::StartNew()
            while ($true) {
                try { Add-Content -LiteralPath $LogFile -Value $Line -Encoding Default -ErrorAction Stop; break }
                catch { if ($Deadline.ElapsedMilliseconds -ge 2000) { break }; Start-Sleep -Milliseconds 50 }
            }
        }
        $Code = $LASTEXITCODE
        if ($null -eq $Code) { $Code = 1 }
    } catch {
        $Code = 1
    }
    if ($DoneFile) { Set-Content -LiteralPath $DoneFile -Value $Code -Encoding Ascii }
    exit $Code
}
$ErrorActionPreference = "Stop"
# Pastas do PATH que o Windows nao consegue resolver (ex.: juncao quebrada de
# outro programa) derrubam o ensurepip/pip com WinError 448; sao ignoradas
# so nesta execucao, sem alterar o PATH do usuario.
$PathOk = foreach ($Entry in ($env:PATH -split ";")) {
    if (-not $Entry) { continue }
    try {
        if ((Get-Item -LiteralPath $Entry -Force -ErrorAction Stop).Attributes -band [IO.FileAttributes]::ReparsePoint) { [void][System.IO.Directory]::GetFileSystemEntries($Entry) }
        $Entry
    } catch { }
}
$env:PATH = $PathOk -join ";"
# Com a saida capturada pelo instalador (sem console visivel), mantem a
# codificacao padrao do Windows, que e a que o instalador le.
if ([Console]::IsOutputRedirected) {
    # Python/rich tambem na codificacao padrao (tabelas em ASCII, acentos ok).
    $env:PYTHONIOENCODING = [System.Text.Encoding]::Default.WebName
} else {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
}
# Este script fica em scripts\; a raiz do programa e a pasta acima.
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Requirements = Join-Path $Root "requirements.txt"

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Test-Command([string]$Name) {
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Get-CompatiblePython {
    foreach ($Version in @("3.13", "3.12", "3.11")) {
        try {
            $Candidate = (& py "-$Version" -c "import sys; print(sys.executable)" 2>$null | Select-Object -First 1)
            if ($Candidate) {
                $Candidate = $Candidate.Trim()
                if ($Candidate -and (Test-Path -LiteralPath $Candidate)) {
                    return $Candidate
                }
            }
        } catch {}
    }
    try {
        $Candidate = (& py -3 -c "import sys; print(sys.executable if sys.version_info[:2] >= (3, 11) else '')" 2>$null | Select-Object -First 1)
        if ($Candidate) {
            $Candidate = $Candidate.Trim()
            if ($Candidate -and (Test-Path -LiteralPath $Candidate)) {
                return $Candidate
            }
        }
    } catch {}
    return $null
}

Write-Step "Verificando Python (3.11 ou superior)"
$PythonExe = Get-CompatiblePython

if (-not $PythonExe) {
    if (-not (Test-Command "winget")) {
        throw "Python 3.11+ nao encontrado e o winget nao esta disponivel. Instale o Python e execute novamente."
    }
    Write-Step "Instalando Python 3.13"
    & winget install --id Python.Python.3.13 -e --scope user --silent --disable-interactivity --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        throw "Falha ao instalar Python 3.13 pelo winget."
    }
    $PythonExe = Get-CompatiblePython
    if (-not $PythonExe) {
        throw "Python foi instalado, mas nao foi localizado. Feche e abra o terminal e execute novamente."
    }
}
Write-Host "Python: $PythonExe" -ForegroundColor Green

# O ambiente virtual fica dentro da propria pasta do programa (venv\), nunca
# no perfil do usuario. Isso torna a pasta autocontida e portavel: mover ou
# copiar a pasta inteira para outra maquina/pasta continua funcionando.
$EnvRoot = Join-Path $Root "venv"
$EnvPython = Join-Path $EnvRoot "Scripts\python.exe"

if (-not (Test-Path -LiteralPath $EnvPython)) {
    Write-Step "Criando ambiente isolado em .\venv"
    & $PythonExe -m venv $EnvRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Falha ao criar o ambiente virtual."
    }
} else {
    Write-Host "Reutilizando ambiente existente: $EnvPython" -ForegroundColor Green
}

Write-Step "Instalando bibliotecas Python"
& $EnvPython -m pip install --upgrade --progress-bar off --no-color pip setuptools wheel
if ($LASTEXITCODE -ne 0) { throw "Falha ao atualizar o pip." }
& $EnvPython -m pip install --upgrade --progress-bar off --no-color -r $Requirements
if ($LASTEXITCODE -ne 0) { throw "Falha ao instalar as dependencias Python." }

function Test-WebView2 {
    $keys = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
    )
    foreach ($key in $keys) {
        $version = (Get-ItemProperty -Path $key -Name pv -ErrorAction SilentlyContinue).pv
        if ($version -and $version -ne "0.0.0.0") { return $true }
    }
    return $false
}

Write-Step "Verificando o Microsoft Edge WebView2 (necessario para a interface)"
if (Test-WebView2) {
    Write-Host "WebView2 encontrado." -ForegroundColor Green
} else {
    if (Test-Command "winget") {
        & winget install --id Microsoft.EdgeWebView2Runtime -e --silent --disable-interactivity --accept-source-agreements --accept-package-agreements
    }
    if (-not (Test-WebView2)) {
        Write-Warning "WebView2 nao encontrado. Instale em https://developer.microsoft.com/microsoft-edge/webview2/ (Evergreen Runtime) para abrir o programa."
    }
}

Write-Step "Verificando Tesseract OCR"
$TesseractCandidates = @(
    (Get-Command tesseract.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "$env:ProgramFiles\Tesseract-OCR\tesseract.exe",
    "$env:ProgramW6432\Tesseract-OCR\tesseract.exe",
    "${env:ProgramFiles(x86)}\Tesseract-OCR\tesseract.exe",
    "$env:LOCALAPPDATA\Programs\Tesseract-OCR\tesseract.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

if (-not $TesseractCandidates) {
    if (-not (Test-Command "winget")) {
        throw "Tesseract nao encontrado e winget indisponivel. Instale UB-Mannheim Tesseract OCR manualmente."
    }
    Write-Step "Instalando Tesseract OCR"
    & winget install --id UB-Mannheim.TesseractOCR -e --silent --disable-interactivity --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "A instalacao UB-Mannheim falhou. Tentando pacote alternativo."
        & winget install --id Tesseract-OCR.Tesseract -e --silent --disable-interactivity --accept-source-agreements --accept-package-agreements
        if ($LASTEXITCODE -ne 0) {
            throw "Nao foi possivel instalar o Tesseract."
        }
    }
}

Write-Step "Verificando Ghostscript (necessario apenas para o formato PDF/A opcional)"
$GhostscriptCandidates = @(
    (Get-Command gswin64c.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    (Get-Command gswin32c.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue)
) | Where-Object { $_ }

if (-not $GhostscriptCandidates) {
    # O pacote do Ghostscript nao esta disponivel de forma confiavel nas
    # fontes padrao do winget nesta configuracao; nao ha um --id garantido
    # para tentar automaticamente. O programa funciona normalmente sem ele
    # (o tipo de PDF de saida padrao e "pdf", nao "pdfa") e detecta a
    # ausencia do Ghostscript antes de tentar gerar um PDF/A.
    Write-Warning "Ghostscript nao encontrado. O modo PDF/A (opcional) ficara indisponivel; os demais modos funcionam normalmente."
    Write-Host "Para habilitar PDF/A, baixe e instale manualmente em: https://ghostscript.com/releases/gsdnld.html"
}

Write-Step "Preparando idiomas OCR em pasta local do programa"
$DataRoot = Join-Path $Root "dados"
$Tessdata = Join-Path $DataRoot "tessdata"
New-Item -ItemType Directory -Path $Tessdata -Force | Out-Null

# Recalcula o caminho depois da eventual instalacao e copia tambem os arquivos
# de configuracao do Tesseract, necessarios ao OCRmyPDF (hOCR, TXT etc.).
$TesseractCandidates = @(
    (Get-Command tesseract.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "$env:ProgramFiles\Tesseract-OCR\tesseract.exe",
    "$env:ProgramW6432\Tesseract-OCR\tesseract.exe",
    "${env:ProgramFiles(x86)}\Tesseract-OCR\tesseract.exe",
    "$env:LOCALAPPDATA\Programs\Tesseract-OCR\tesseract.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

if ($TesseractCandidates) {
    $TesseractExe = $TesseractCandidates | Select-Object -First 1
    $SourceTessdata = Join-Path (Split-Path -Parent $TesseractExe) "tessdata"
    if (Test-Path -LiteralPath $SourceTessdata) {
        Copy-Item -Path (Join-Path $SourceTessdata "*") -Destination $Tessdata -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-GitBlobSha1([byte[]]$Bytes) {
    $header = [System.Text.Encoding]::ASCII.GetBytes("blob $($Bytes.Length)`0")
    $combined = New-Object byte[] ($header.Length + $Bytes.Length)
    [Array]::Copy($header, 0, $combined, 0, $header.Length)
    [Array]::Copy($Bytes, 0, $combined, $header.Length, $Bytes.Length)
    $sha1 = [System.Security.Cryptography.SHA1]::Create()
    try {
        return -join ($sha1.ComputeHash($combined) | ForEach-Object { $_.ToString("x2") })
    } finally {
        $sha1.Dispose()
    }
}

function Get-ExpectedTessdataSha([string]$Language) {
    # A GitHub API expoe o sha do blob git (nao um checksum publicado
    # separadamente), mas ainda serve para detectar download corrompido ou
    # incompleto: comparamos o conteudo baixado contra o que o GitHub diz
    # ser o conteudo atual do arquivo no repositorio oficial.
    try {
        $uri = "https://api.github.com/repos/tesseract-ocr/tessdata_fast/contents/$Language.traineddata"
        $response = Invoke-RestMethod -Uri $uri -Headers @{ "User-Agent" = "ExtratorDeTexto-Installer" } -UseBasicParsing -TimeoutSec 15
        return $response.sha
    } catch {
        return $null
    }
}

$LanguageBase = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/main"
foreach ($Language in @("por", "eng", "osd")) {
    $Destination = Join-Path $Tessdata "$Language.traineddata"
    if (-not (Test-Path -LiteralPath $Destination)) {
        Write-Host "Baixando idioma: $Language"
        Invoke-WebRequest -Uri "$LanguageBase/$Language.traineddata" -OutFile $Destination -UseBasicParsing
        $ExpectedSha = Get-ExpectedTessdataSha $Language
        $ActualSha = Get-GitBlobSha1([System.IO.File]::ReadAllBytes($Destination))
        if ($ExpectedSha) {
            if ($ActualSha -ne $ExpectedSha) {
                Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
                throw "Falha na verificacao de integridade do idioma '$Language': o arquivo baixado nao corresponde ao esperado. Execute novamente."
            }
            Write-Host "  Integridade verificada: $Language (sha $ActualSha)"
        } else {
            Write-Warning "Nao foi possivel verificar a integridade de '$Language' (API do GitHub indisponivel). Arquivo mantido sem verificacao."
        }
    }
}

Write-Step "Verificando a instalacao"
$env:TESSDATA_PREFIX = $Tessdata
& $EnvPython (Join-Path $Root "src\extrator_core.py") --check
if ($LASTEXITCODE -ne 0) {
    throw "A verificacao final encontrou componentes ausentes."
}

Write-Host "`nInstalacao concluida com sucesso." -ForegroundColor Green
Write-Host "Todo o ambiente (venv\) e os dados (dados\) ficam dentro desta propria pasta."
Write-Host "Abra o programa por: scripts\Abrir_Extrator.bat (ou pelo atalho do Menu Iniciar)"
