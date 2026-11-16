# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

## [0.1.0] - 2026-11-16

Primeira versão do Extrator de Texto.

### Adicionado
- Conversão de documentos para Markdown, TXT ou PDF pesquisável, com OCR automático ou forçado, em lote e com diagnóstico prévio do arquivo.
- Transcrição de áudio e vídeo para texto, localmente, com fila, pausa, cancelamento e saídas `txt`, `md`, `srt` e `json`.
- Interface em janela nativa (WebView2 via `pywebview`), com menu lateral, tema claro e escuro e arrastar e soltar.
- Central de execuções e log dentro do programa.
- Instalador `.exe` sem pop-ups: o PowerShell roda oculto, o andamento aparece dentro do próprio instalador e o log completo fica no botão "Mostrar detalhes" e em `dados\logs\instalacao.log`.
