# Extrator de Texto

Ferramenta única, local e autocontida, com dois motores na mesma janela:

- **OCR e conversão** de documentos para Markdown, TXT ou PDF pesquisável.
- **Transcrição** de áudio e vídeo para texto (PT-BR), sem enviar nada para serviços externos.

A interface é nativa (Rust, egui) e roda em uma janela única. O processamento é feito por um motor em Python, sem janela, que a interface inicia e controla em segundo plano.

## Instalação

1. Baixe o `Extrator-de-Texto-Setup-X.Y.Z.exe` da última [Release](../../releases).
2. Execute o instalador e aguarde. Ele não pede administrador. Se faltar, instala o Python e o Tesseract pelo `winget` (precisa de internet).
3. Abra pelo atalho **Extrator de Texto** no Menu Iniciar.

O programa fica em `%LOCALAPPDATA%\Programs\ExtratorDeTexto`, com o executável `Extrator de Texto.exe`, o desinstalador (também no Menu Iniciar) e as pastas internas ocultas. Para atualizar, execute o instalador da versão nova: o ambiente (`venv\`) e os dados (`dados\`) são mantidos.

Se algo falhar, use o atalho **Extrator de Texto - Reparar instalacao** ou, no aplicativo, a tela *Instalação / Ajuda* → *Verificar instalação*, que oferece corrigir o que faltar.

### O que é instalado

| Componente | Como |
|---|---|
| Python 3.11+ e bibliotecas (incluindo ffmpeg) | `winget` e `pip`, dentro de `venv\` (feito por `scripts\install.ps1`) |
| Tesseract OCR (motor nativo) | `winget` |
| Ghostscript (opcional, só para PDF/A) | manual: https://ghostscript.com/releases/gsdnld.html |
| Idiomas do OCR (`por`, `eng`, `osd`) | download com verificação de integridade, em `dados\tessdata` |

O programa não depende mais do WebView2.

## Uso

A janela tem um menu lateral com estas telas:

- **Converter**: conversão direta, OCR automático ou forçado, PDF pesquisável.
- **Lote**: processa uma pasta inteira.
- **Transcrição**: fila de áudios/vídeos para transcrever.
- **Diagnóstico**: analisa um arquivo antes de decidir o que fazer.
- **Execuções**: Central de execuções, com o andamento de cada tarefa (ETA e barra quando o total é conhecido).
- **Log**: registro do programa.
- **Configurações**: idioma do OCR, DPI, PSM etc.
- **Ajuda e instalação**: verifica a instalação, instala o que faltar e limpa o cache.

Há tema claro e escuro, e é possível arrastar arquivos para a janela. Só uma instância do programa roda por vez.

Formatos aceitos na conversão direta: `.pdf .docx .pptx .xlsx .html .htm .txt .csv .json .xml`. Imagens com OCR: `.png .jpg .jpeg .tif .tiff .bmp .webp`.

### Transcrição de áudio e vídeo

Usa o `faster-whisper` (Whisper local, em CPU). Suporta mp3, wav, m4a, ogg, flac, aac, wma e vídeos (mp4, mkv, mov, avi, webm, flv, wmv, m4v), cujo áudio é extraído automaticamente.

- Fila de arquivos, processados um de cada vez, com reordenação antes de iniciar.
- Pausar e continuar a qualquer momento (a pausa ocorre entre segmentos).
- Cancelar interrompe o item atual; os demais ficam pendentes.
- Progresso com percentual, tempo processado e restante, tempo decorrido e estimativa (HH:MM:SS).
- Saídas: `txt`, `md` (com marcação de tempo), `srt` (legendas) e `json` (com tempo por palavra, se marcado).
- Confirmação ao fechar o programa com uma transcrição em andamento.

Só os modelos já baixados aparecem na lista. Use **Baixar outro modelo…** para um maior (exige internet). Na primeira verificação, o modelo que ainda não foi baixado aparece como AVISO, e não como erro. O modelo `small` é o padrão recomendado para PT-BR.

### Linha de comando (motor de OCR)

Pelo atalho `scripts\Abrir_Extrator_Terminal.bat` ou diretamente:

```bat
venv\Scripts\python.exe src\extrator_core.py direct "arquivo.pdf"
venv\Scripts\python.exe src\extrator_core.py auto-ocr "arquivo.pdf" --pages "1-100" --format md
venv\Scripts\python.exe src\extrator_core.py force-ocr "arquivo.pdf" --format both
venv\Scripts\python.exe src\extrator_core.py searchable "arquivo.pdf"
venv\Scripts\python.exe src\extrator_core.py redo-pdf "arquivo.pdf"
venv\Scripts\python.exe src\extrator_core.py diagnose "arquivo.pdf"
```

## Segurança e privacidade

- O arquivo original nunca é sobrescrito.
- Tudo roda localmente. Documentos e áudios não são enviados a serviços externos nem de inteligência artificial.
- PDFs com assinatura digital geram aviso antes do OCR pesquisável, pois a cópia com OCR invalida a assinatura. PDFs protegidos por senha precisam ser desbloqueados antes.
- Os idiomas do OCR são conferidos contra o repositório oficial `tesseract-ocr/tessdata_fast` antes de serem aceitos.
- Avisos e confirmações aparecem dentro da própria janela; o programa não abre pop-ups.

No modo automático, uma página só é tratada como texto nativo (sem OCR) quando tem texto suficiente **e** não é dominada por uma imagem de página inteira. Assim, uma página digitalizada com só um carimbo de texto (por exemplo, "Documento assinado digitalmente") não deixa o conteúdo escaneado de fora.

### Cancelamento e retomada

Na interface, use o botão **Cancelar** da tela correspondente; no terminal, `Ctrl+C`. No OCR para Markdown/TXT, cada página concluída fica salva temporariamente: rodar de novo com as mesmas configurações continua de onde parou.

### Formato PDF/A (opcional)

O tipo de saída `pdfa` exige o Ghostscript instalado no Windows. Sem ele, use `pdf` (padrão).

## Onde ficam os dados

Tudo dentro da pasta de instalação:

| O quê | Onde |
|---|---|
| Ambiente Python | `venv\` |
| Configuração do OCR | `dados\config.json` |
| Logs | `dados\logs` |
| Log da instalação | `dados\logs\instalacao.log` |
| Retomada do OCR | `dados\work` |
| Idiomas do OCR | `dados\tessdata` |
| Modelos de transcrição | `dados\transcricao\modelos` |

Logs e cache de retomada com mais de 7 dias são apagados ao abrir o programa. O botão **Limpar cache e logs**, na tela *Ajuda e instalação*, apaga tudo na hora.

## Desenvolvimento

### Estrutura do repositório

```text
ui/           interface nativa em Rust (egui/eframe), crate extrator-ui; gera "Extrator de Texto.exe"
ui/src/       código da interface (telas, ponte com o motor, log, tema, instalação)
ui/tools/     motor_falso.py: motor falso, para desenvolver a interface sem OCR real
src/          motor Python sem janela:
              motor.py (protocolo JSON por linha), extrator_api.py (ponte),
              extrator_core.py (conversão e OCR), transcricao_core.py (transcrição),
              execucoes.py (Central de execuções), leitorlog.py (leitura de logs),
              semjanela.py (oculta consoles de subprocessos)
src/assets/   ícone do aplicativo
docs/         PROTOCOLO-MOTOR.md: protocolo entre a interface e o motor
tests/        testes automatizados do motor (pytest)
scripts/      install.ps1 (ambiente Python) e lançadores .bat; scripts/dev: ferramentas de desenvolvimento
installer/    build.ps1 (gera o instalador), Extrator.iss (Inno Setup) e publish.ps1
.gitlab-ci.yml, requirements.txt, requirements-dev.txt, VERSION.txt, CHANGELOG.md
```

`venv\`, `dados\`, `build\` e `dist\` são gerados e não entram no Git.

### Pré-requisitos para desenvolvimento

- Python 3.11 ou mais novo, com `requirements.txt` e `requirements-dev.txt` instalados em `venv\`.
- Rust (cargo e toolchain MSVC), para compilar a interface em `ui\`.
- Inno Setup 6, para gerar o instalador.

### Rodar a partir do código

Crie o ambiente Python com `scripts\Instalar.bat` e, em seguida, rode a interface com `cargo run` dentro de `ui\`.

Variáveis de ambiente usadas no desenvolvimento: `EXTRATOR_RAIZ`, `EXTRATOR_PYTHON`, `EXTRATOR_MOTOR`, `EXTRATOR_PAGINA` e `EXTRATOR_PRELOAD`. Para testar a interface sem executar OCR ou transcrição reais, use o motor falso em `ui\tools\motor_falso.py`.

### Testes

```bat
scripts\dev\Executar_Testes.bat
```

Executa os testes Python do motor (em `tests\`), sem executar OCR ou transcrição reais. Na interface, use `cargo test` e `cargo clippy` dentro de `ui\`.

### Build e instalador

`installer\build.ps1` compila a interface (`cargo build --release` em `ui\`) e gera o instalador com o Inno Setup 6. Requer Rust e Inno Setup 6 instalados.

```powershell
powershell -File installer\build.ps1
```

A saída vai para `dist\`.

### Lançar uma versão

1. Atualize `VERSION.txt` e o `CHANGELOG.md` (seção `## [X.Y.Z]`) e faça o commit.
2. Crie e envie a tag:

   ```bash
   git tag vX.Y.Z
   git push origin master vX.Y.Z
   ```

3. O pipeline do GitLab (`.gitlab-ci.yml`) roda na tag, gera `Extrator-de-Texto-Setup-X.Y.Z.exe` e cria a Release com ele anexado. Isso exige um runner Windows com Rust, Inno Setup 6 e a tag `windows`.

## Solução de problemas

**O Python não é reconhecido.** Recrie o ambiente a partir da pasta de instalação, no PowerShell:

```powershell
Remove-Item -Recurse -Force .\venv
scripts\Instalar.bat
```

**A instalação está incompleta ou com erro.** Use o atalho **Extrator de Texto - Reparar instalacao**. Ele abre uma janela própria com o log do reparo.
