/* Simulador do backend, só para desenvolver/testar a interface num navegador comum.
   Não é instalado com o programa. */
"use strict";
(function () {
  const emit = (list) => window.__onEvents && window.__onEvents(list);
  const later = (ms, fn) => setTimeout(fn, ms);
  let cancelled = false, paused = false;

  const modes = [
    ["auto", "OCR automático", "Mantém o texto nativo e aplica OCR só onde falta. Gera Markdown ou TXT.", "pdf", true, true, false],
    ["force", "OCR forçado", "Reconhece todas as páginas, mesmo as que já têm texto. Mais lento.", "pdf", true, true, false],
    ["direct", "Conversão direta", "Converte PDF, Word, Excel, PowerPoint, HTML e outros para Markdown, sem OCR.", "direct", false, false, false],
    ["searchable_skip", "PDF pesquisável", "Mantém a aparência do PDF e adiciona uma camada de texto pesquisável.", "pdf", true, false, false],
    ["searchable_skip_md", "PDF pesquisável + Markdown", "Gera o PDF pesquisável e também o Markdown dele.", "pdf", true, false, false],
    ["searchable_force", "PDF pesquisável (OCR forçado)", "Refaz o OCR de todas as páginas ao gerar o PDF pesquisável.", "pdf", true, false, false],
    ["searchable_redo", "Refazer OCR existente", "Descarta o OCR antigo do PDF e faz de novo.", "pdf", true, false, false],
    ["images", "OCR de imagens", "Reconhece texto em PNG, JPG, TIFF, BMP e WEBP, podendo juntar tudo em um arquivo.", "images", false, true, true],
  ].map(([key, label, description, accepts, pages, format, combine]) => ({ key, label, description, accepts, pages, format, combine }));

  const config = { language: "por+eng", dpi: 220, psm: 3, native_text_threshold: 80, verbosity: "detalhado", cpu_mode: "normal",
    rotate_pages: true, deskew: true, pdf_output_type: "pdf", resume: true, auto_open_output: false };
  const models = {
    tiny: { tamanho_mb: 76, precisao: "Baixa", velocidade: "Muito rapida", descricao: "Para experimentacao rapida", instalado: false },
    base: { tamanho_mb: 142, precisao: "Razoavel", velocidade: "Rapida", descricao: "Equilibrio basico", instalado: false },
    small: { tamanho_mb: 460, precisao: "Boa", velocidade: "Moderada", descricao: "Melhor equilibrio (padrao)", instalado: true },
    medium: { tamanho_mb: 1500, precisao: "Muito boa", velocidade: "Lenta", descricao: "Alta precisao para PT-BR", instalado: false },
  };
  const trState = () => ({ default_model: "small", installed: Object.keys(models).filter((m) => models[m].instalado), models,
    languages: ["pt", "en", "es", "fr", "de", "ja", "ko", "auto"], formats: ["txt", "md", "srt", "json"] });

  const api = {
    bootstrap: async () => ({ ok: true, app_name: "Extrator de Texto", version: "0.1.0", convert_modes: modes,
      batch_modes: [
        { key: "auto_md", label: "OCR automático para Markdown", description: "Usa o texto nativo e faz OCR só nas páginas sem texto." },
        { key: "force_md", label: "OCR forçado para Markdown", description: "Reconhece todas as páginas. Mais lento." },
        { key: "direct", label: "Conversão direta", description: "Sem OCR. Aceita PDF, Word, Excel, PowerPoint, HTML e mais." },
        { key: "searchable", label: "PDF pesquisável", description: "Adiciona camada de texto mantendo a aparência." },
        { key: "searchable_md", label: "PDF pesquisável + Markdown", description: "Gera o PDF pesquisável e o Markdown dele." },
      ],
      config: { ...config }, defaults: { ...config },
      extensions: { pdf: [".pdf"], direct: [".pdf", ".docx", ".pptx", ".xlsx", ".html", ".htm", ".txt", ".csv", ".json", ".xml"],
        images: [".png", ".jpg", ".jpeg", ".tif", ".tiff", ".bmp", ".webp"], media: [".mp3", ".wav", ".m4a", ".mp4", ".mkv", ".ogg", ".flac"] },
      data_dir: "C:\\Users\\voce\\AppData\\Local\\Programs\\ExtratorDeTexto\\dados", log_file: "C:\\...\\dados\\logs\\extrator_20261023.log",
      transcricao: trState(), busy: { ocr: false, transcricao: false } }),

    pick_files: async (accepts) => ({ ok: true, paths: accepts === "media"
      ? ["C:\\Audiencias\\audiencia_2026-10-20.mp3", "C:\\Audiencias\\reuniao cliente.mp4"]
      : accepts === "images" ? ["C:\\Scans\\pagina1.png", "C:\\Scans\\pagina2.png"]
      : ["C:\\Processos\\Petição inicial - Fulano de Tal vs Ciclano.pdf", "C:\\Processos\\Contrato de prestação de serviços (assinado).pdf", "C:\\Processos\\Anexos\\Procuração.pdf"] }),
    pick_folder: async () => ({ ok: true, path: "C:\\Processos\\Lote outubro" }),
    open_path: async () => ({ ok: true }), open_data_folder: async () => ({ ok: true }),

    start_convert: async (p) => {
      if (!p.files.length) return { ok: false, error: "Selecione ao menos um arquivo." };
      if (/assinado/i.test(p.files.join()) && p.mode.startsWith("searchable") && !p.confirm_signed)
        return { ok: true, status: "needs_confirm", signed: ["Contrato de prestação de serviços (assinado).pdf"] };
      cancelled = false;
      const total = p.files.length; let i = 0;
      const step = () => {
        if (cancelled) return emit([{ type: "cancelled" }]);
        if (i >= total) return emit([{ type: "done" }]);
        const name = p.files[i].split("\\").pop(); i++;
        emit([{ type: "file_start", index: i, total, name }, { type: "start", file: name, total: 3 }]);
        [1, 2, 3].forEach((n) => later(n * 500, () => !cancelled && emit([{ type: "page", current: n, total: 3, source: n === 2 ? "OCR" : "texto" }])));
        later(1800, step);
      };
      later(300, step);
      return { ok: true, status: "started" };
    },
    cancel_job: async () => { cancelled = true; return { ok: true }; },

    scan_batch: async (p) => (p.input_folder ? { ok: true, count: 14, signed: p.mode.startsWith("searchable") ? ["Contrato (assinado).pdf"] : [], output_folder: p.output_folder || p.input_folder + "\\Extrator_saida", sample: [] }
      : { ok: false, error: "Escolha uma pasta de entrada válida." }),
    start_batch: async (p) => api.start_convert({ mode: "auto", files: ["a.pdf", "b.pdf", "c.pdf"], confirm_signed: true }),

    transcricao_state: async () => ({ ok: true, ...trState() }),
    transcricao_deps: async () => ({ ok: true, deps: [{ name: "ffmpeg", ok: true }, { name: "faster-whisper", ok: true }, { name: "soundfile", ok: true }] }),
    start_transcricao: async (p) => {
      cancelled = false; paused = false;
      let idx = 0;
      const next = () => {
        if (cancelled || idx >= p.items.length) return emit([{ type: "transc_queue_done" }]);
        const it = p.items[idx++]; const total = 600; let done = 0;
        emit([{ type: "transc_item_start", id: it.id, index: idx, total: p.items.length, name: it.path.split("\\").pop() }]);
        const tick = () => {
          if (cancelled) { emit([{ type: "transc_item_cancelled", id: it.id }, { type: "transc_queue_done" }]); return; }
          if (!paused) done += 60;
          emit([{ type: "transc_progress", id: it.id, pct: (100 * done) / total, processado_s: done, total_s: total, restante_s: total - done, elapsed_s: done / 4, eta_s: (total - done) / 4 }]);
          if (done >= total) { emit([{ type: "transc_item_done", id: it.id, output_path: "C:\\Audiencias\\saida.txt" }]); later(300, next); } else later(350, tick);
        };
        later(300, tick);
      };
      later(200, next);
      return { ok: true, status: "started" };
    },
    toggle_pause_transcricao: async () => { paused = !paused; return { ok: true, paused }; },
    cancel_transcricao: async () => { cancelled = true; return { ok: true }; },
    download_model: async (name) => { later(1200, () => { models[name].instalado = true; emit([{ type: "model_download", name, status: "done" }]); }); return { ok: true, status: "started" }; },

    diagnose: async () => ({ ok: true, rows: [["Arquivo", "Petição inicial.pdf"], ["Páginas", "42"], ["Texto nativo", "38 de 42 páginas"], ["Imagem de página inteira", "4 páginas"], ["Assinatura digital", "Não detectada"], ["Protegido por senha", "Não"]].map(([item, result]) => ({ item, result })) }),

    save_settings: async (v) => { Object.assign(config, v, { dpi: +v.dpi, psm: +v.psm, native_text_threshold: +v.native_text_threshold }); return { ok: true, config: { ...config } }; },
    restore_settings: async () => ({ ok: true, config: { language: "por+eng", dpi: 220, psm: 3, native_text_threshold: 80, verbosity: "detalhado", cpu_mode: "normal", rotate_pages: true, deskew: true, pdf_output_type: "pdf", resume: true, auto_open_output: false } }),

    start_check_installation: async () => {
      const rows = [
        { name: "Python", ok: true, detail: "3.14.0" }, { name: "MarkItDown", ok: true, detail: "0.1.6" },
        { name: "PyMuPDF", ok: true, detail: "instalado" }, { name: "pytesseract", ok: true, detail: "instalado" },
        { name: "Pillow", ok: true, detail: "instalado" }, { name: "OCRmyPDF", ok: true, detail: "17.8.1" },
        { name: "Rich", ok: true, detail: "instalado" }, { name: "pywebview", ok: true, detail: "instalado" },
        { name: "Tesseract", ok: true, detail: "C:\Tesseract-OCR\tesseract.exe" }, { name: "Idiomas OCR", ok: true, detail: "eng, osd, por" },
        { name: "Ghostscript", ok: false, detail: "nao encontrado" }, { name: "Modelo de transcricao", ok: true, detail: "small" }];
      emit([{ type: "check_start" }]);
      rows.forEach((r, i) => later(250 * (i + 1), () => emit([{ type: "check_row", index: i + 1, ...r }])));
      later(250 * (rows.length + 1), () => emit([{ type: "check_done", ok: true, rows, missing_pip: [], missing_model: false }]));
      return { ok: true, status: "started" };
    },
    install_missing: async () => { later(900, () => emit([{ type: "install_fix_done" }])); return { ok: true, status: "started" }; },
    clear_data: async () => ({ ok: true, logs: 3, jobs: 1 }),

    // Central de execuções e log (simulados: uma tarefa determinada, uma indeterminada e algumas antigas)
    exec_listar: async () => {
      const t = (Date.now() - t0) / 1000;
      const pct = Math.min(100, (t / 60) * 100);
      const base = { ferramenta: "converter", ferramenta_nome: "Converter", variante: "auto", avisos: 0, mensagem: "", erro: "",
        cancelavel: false, cancelando: false, repetivel: false, log_arquivo: "", log_ini: null, log_fim: null, linhas: 2, eta_s: null,
        eta_min_s: null, eta_max_s: null, eta_fonte: "", criada_em: t0 / 1000, inicio_em: t0 / 1000, fim_em: null };
      return { ok: true, versao: 1, rodando: 2, na_fila: 0, total: 4, ferramentas: { converter: "Converter", verificacao: "Verificação", transcricao: "Transcrição" },
        estados: {}, execucoes: [
          { ...base, id: "m1", rotulo: "OCR automático", entrada: "Petição inicial.pdf", estado: "rodando", estado_texto: "Rodando",
            etapa: `Página ${Math.floor(pct / 100 * 42)} de 42 (OCR)`, determinada: true, progresso: Math.round(pct * 10) / 10,
            progresso_texto: `${Math.floor(pct / 100 * 42)} de 42 páginas`, decorrido_s: t, cancelavel: true,
            eta_s: 60 - t, eta_min_s: 50 - t, eta_max_s: 80 - t, eta_texto: t < 4 ? "estimando…" : "cerca de 1 min", eta_fonte: "andamento" },
          { ...base, id: "m2", ferramenta: "converter", rotulo: "PDF pesquisável", entrada: "Contrato.pdf", estado: "rodando", estado_texto: "Rodando",
            etapa: "Gerando PDF pesquisável (OCRmyPDF)", determinada: false, progresso: null, progresso_texto: "", decorrido_s: t, cancelavel: true,
            eta_texto: t < 3 ? "estimando…" : "sem estimativa" },
          { ...base, id: "m3", ferramenta: "verificacao", ferramenta_nome: "Verificação", rotulo: "Verificar instalação", entrada: "Componentes do programa",
            estado: "concluida", estado_texto: "Concluída", etapa: "Verificado: Rich", determinada: true, progresso: 100, progresso_texto: "11 de 11 componentes",
            decorrido_s: 7.4, fim_em: t0 / 1000 - 60, mensagem: "11 de 11 componente(s) OK.", repetivel: true, linhas: 3 },
          { ...base, id: "m4", rotulo: "OCR forçado", entrada: "Procuração.pdf", estado: "falhou", estado_texto: "Falhou", determinada: false, progresso: null,
            decorrido_s: 3.1, fim_em: t0 / 1000 - 600, erro: "PDF protegido por senha.", repetivel: true },
        ] };
    },
    exec_detalhe: async (id, desde) => ({ ok: true, proximo: 2, ativo: true, linhas: desde >= 2 ? [] : [
      { t: "10:00:01", n: "info", x: "Iniciando: OCR automático (1 arquivo(s))" }, { t: "10:00:02", n: "info", x: "Modo: auto | Páginas: 42" }] }),
    exec_cancelar: async () => ({ ok: true }), exec_repetir: async () => ({ ok: true, status: "started", acao: "start_check_installation" }),
    exec_resumo: async (id) => ({ ok: true, texto: "Execução simulada " + id }), exec_limpar_concluidas: async () => ({ ok: true, removidas: 2 }),
    log_arquivos: async () => ({ ok: true, atual: "extrator_20261116_100000.log", pasta: "C:\dados\logs", arquivos: [
      { nome: "extrator_20261116_100000.log", tamanho: 4096, modificado: 0, data: "2026-11-16", hora: "10:00", atual: true, tipo: "sessao" },
      { nome: "instalacao.log", tamanho: 9000, modificado: 0, data: "2026-11-15", hora: "18:30", atual: false, tipo: "instalador" }] }),
    log_ler: async (p) => ({ ok: true, nome: p.nome, tamanho: 300, ini: 0, fim: 300, ao_vivo: true, mais: false, reiniciado: false, linhas: p.modo === "desde" ? [] : [
      { o: 0, t: "2026-11-16 10:00:00", nv: "INFO", x: "Inicio Extrator de Texto" }, { o: 80, t: "2026-11-16 10:00:05", nv: "WARNING", x: "Tesseract sem o idioma por" },
      { o: 160, t: "2026-11-16 10:00:09", nv: "ERROR", x: "Falha ao processar exemplo.pdf" }, { o: 240, t: "", nv: "ERROR", x: "Traceback (simulado)" }] }),
    log_buscar: async () => ({ ok: true, nome: "x", tamanho: 300, ini: 0, fim: 300, linhas: [], total_achadas: 0, cortada: false }),
    log_registrar: async () => ({ ok: true, nome: "extrator_20261116_100000.log", offset: 160 }),
    log_abrir_pasta: async () => ({ ok: true }),
  };
  const t0 = Date.now();
  window.pywebview = { api: new Proxy(api, { get: (t, k) => (k in t ? t[k] : async () => ({ ok: false, error: "mock: " + String(k) })) }) };
})();
