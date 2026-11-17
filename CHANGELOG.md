# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

## [0.1.1] - 2026-11-17

Primeira versão publicada do Extrator de Texto.

### Adicionado
- Conversão de documentos para Markdown, TXT ou PDF pesquisável, com OCR automático ou forçado, em lote e com diagnóstico prévio do arquivo.
- Transcrição de áudio e vídeo para texto, localmente, com fila, pausa, cancelamento e saídas `txt`, `md`, `srt` e `json`.
- Interface nativa em Rust (egui), em janela única, com menu lateral, tema claro e escuro e arrastar e soltar; sem navegador nem WebView2.
- Central de execuções, com ETA e barra de progresso quando o total é conhecido, e log dentro do programa.
- Executável `Extrator de Texto.exe` (sem console, instância única, reparo em janela própria com `--reparar`), atalho "Desinstalar" e atalho "Reparar instalacao" no Menu Iniciar, e pasta de instalação organizada (ambiente, scripts e idiomas do OCR ocultos).
- Instalador `.exe` (Inno Setup, sem administrador) sem pop-ups: o andamento aparece dentro do próprio instalador e o log completo fica no botão "Mostrar detalhes" e em `dados\logs\instalacao.log`.

### Alterado
- O Python passou a ser um motor sem janela (`src\motor.py`), iniciado pela interface e controlado por JSON por linha na entrada e saída padrão (protocolo em `docs\PROTOCOLO-MOTOR.md`).
- A verificação de instalação mostra AVISO, e não erro, para o modelo de transcrição que será baixado no primeiro uso.
- O download do ffmpeg (cerca de 72 MB) aparece como uma execução na Central de execuções, com progresso.

### Corrigido
- A transcrição falhava no Python 3.14 com a biblioteca `av` 19 (`TypeError ... metadata_errors`). O `requirements.txt` agora fixa `av>=11,<17`.
- O instalador não mostra mais a caixa "A Pasta Existe".
