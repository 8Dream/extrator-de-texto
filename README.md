# Extrator de Texto

Ferramenta única, local e autocontida, com dois motores na mesma janela:

- **OCR e conversão** de documentos para Markdown, TXT ou PDF pesquisável.
- **Transcrição** de áudio e vídeo para texto (PT-BR), sem enviar nada para serviços externos.

## Instalação

1. Baixe o `Extrator-de-Texto-Setup-X.Y.Z.exe` da última [Release](../../releases).
2. Execute o instalador e aguarde. Ele instala o Python, as bibliotecas e o Tesseract (precisa de internet).
3. Abra pelo atalho **Extrator de Texto** no Menu Iniciar.

O programa fica em `%LOCALAPPDATA%\Programs\ExtratorDeTexto`, sem exigir administrador. Para atualizar, execute o instalador da versão nova: o ambiente (`venv\`) e os dados (`dados\`) são mantidos.

Se algo falhar, use o atalho **Extrator de Texto - Reparar instalacao** ou, no aplicativo, a aba *Instalação / Ajuda* → *Verificar instalação*, que oferece corrigir o que faltar.

### O que é instalado

| Componente | Como |
|---|---|
| Python 3.11+ e bibliotecas (incluindo ffmpeg) | `winget` e `pip`, dentro de `venv\` |
| Tesseract OCR (motor nativo) | `winget` |
| Microsoft Edge WebView2 (mostra a interface) | `winget`, se ainda não houver (já vem no Windows 11) |
| Ghostscript (opcional, só para PDF/A) | manual: https://ghostscript.com/releases/gsdnld.html |
| Idiomas do OCR (`por`, `eng`, `osd`) | download com verificação de integridade, em `dados\tessdata` |

## Uso

A janela tem um menu lateral com estas telas:

- **Converter**: conversão direta, OCR automático ou forçado, PDF pesquisável.
- **Lote**: processa uma pasta inteira.
- **Transcrição**: fila de áudios/vídeos para transcrever.
- **Diagnóstico**: analisa um arquivo antes de decidir o que fazer.
- **Configurações**: idioma do OCR, DPI, PSM etc.
- **Instalação / Ajuda**: verifica e conserta a instalação, limpa cache e logs.

### Transcrição de áudio e vídeo

Usa o `faster-whisper` (Whisper local, em CPU). Suporta mp3, wav, m4a, ogg, flac, aac, wma e vídeos (mp4, mkv, mov, avi, webm, flv, wmv, m4v), cujo áudio é extraído automaticamente.

- Fila de arquivos, processados um de cada vez, com reordenação antes de iniciar.
- Pausar e continuar a qualquer momento (a pausa ocorre entre segmentos).
- Cancelar interrompe o item atual; os demais ficam pendentes.
- Progresso com percentual, tempo processado e restante, tempo decorrido e estimativa (HH:MM:SS).
- Saídas: `txt`, `md` (com marcação de tempo), `srt` (legendas) e `json` (com tempo por palavra, se marcado).
- Confirmação ao fechar o programa com uma transcrição em andamento.

Só os modelos já baixados aparecem na lista. Use **Baixar outro modelo…** para um maior (exige internet). O modelo `small` é o padrão recomendado para PT-BR.

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

No modo automático, uma página só é tratada como texto nativo (sem OCR) quando tem texto suficiente **e** não é dominada por uma imagem de página inteira. Assim, uma página digitalizada com só um carimbo de texto (por exemplo, "Documento assinado digitalmente") não deixa o conteúdo escaneado de fora.

### Cancelamento e retomada

No terminal, `Ctrl+C` cancela; na interface, use o botão **Cancelar** da aba. No OCR para Markdown/TXT, cada página concluída fica salva temporariamente: rodar de novo com as mesmas configurações continua de onde parou.

### Formato PDF/A (opcional)

O tipo de saída `pdfa` exige o Ghostscript instalado no Windows. Sem ele, use `pdf` (padrão).

## Onde ficam os dados

Tudo dentro da pasta de instalação:

| O quê | Onde |
|---|---|
| Ambiente Python | `venv\` |
| Configuração do OCR | `dados\config.json` |
| Logs | `dados\logs` |
| Retomada do OCR | `dados\work` |
| Idiomas do OCR | `dados\tessdata` |
| Modelos de transcrição | `dados\transcricao\modelos` |

Logs e cache de retomada com mais de 7 dias são apagados ao abrir o programa. O botão **Limpar cache e logs** apaga tudo na hora.

## Desenvolvimento

### Estrutura do repositório

```text
src/          motor (extrator_core.py, transcricao_core.py), ponte (extrator_api.py) e janela (extrator.py)
src/web/      interface em HTML, CSS e JavaScript (web/dev/mock.js é só para desenvolvimento)
src/assets/   ícone do aplicativo
tests/        testes automatizados (pytest)
scripts/      lançadores e instalação (.bat, install.ps1); scripts/dev: ferramentas de desenvolvimento
installer/    instalador .exe (Inno Setup) e publicação da Release
requirements.txt, requirements-dev.txt, VERSION.txt, CHANGELOG.md
```

`venv\` e `dados\` são gerados na instalação e não entram no Git.

### Rodar a partir do código

```bat
scripts\Instalar.bat
scripts\Abrir_Extrator.bat
```

### Desenvolver a interface

A interface é HTML/CSS/JavaScript puro (sem build nem dependências externas) em `src/web/`, exibida pelo `pywebview` no WebView2. O JavaScript chama o Python por `pywebview.api.<método>` (veja `src/extrator_api.py`) e recebe o progresso por `window.__onEvents`.

Para editar o visual sem o motor, sirva a pasta e abra com `?mock`, que usa o simulador `src/web/dev/mock.js`:

```bash
python -m http.server 8765 --directory src/web
# abra http://127.0.0.1:8765/index.html?mock
```

Para abrir a janela real com ferramentas de depuração: `venv\Scripts\python.exe src\extrator.py --debug`.

### Testes

```bat
scripts\dev\Executar_Testes.bat
```

Cobrem o motor de OCR, a interface (incluindo legibilidade e barra de progresso) e a transcrição (formatação, fila, pausa, cancelamento, instalação automática), sem executar OCR ou transcrição reais.

### Lançar uma versão

1. Atualize `VERSION.txt` e o `CHANGELOG.md` (seção `## [X.Y.Z]`) e faça o commit.
2. Crie e envie a tag:

   ```bash
   git tag vX.Y.Z
   git push origin master vX.Y.Z
   ```

3. O GitLab CI gera `Extrator-de-Texto-Setup-X.Y.Z.exe` e publica a Release com ele anexado. Isso exige um runner Windows com Inno Setup 6 e a tag `windows`.

Para gerar o instalador localmente: `powershell -File installer\build.ps1` (saída em `dist\`).

## Solução de problemas

**O Python não é reconhecido.** Recrie o ambiente a partir da pasta de instalação, no PowerShell:

```powershell
Remove-Item -Recurse -Force .\venv
scripts\Instalar.bat
```
