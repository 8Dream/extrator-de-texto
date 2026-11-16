/* Extrator de Texto - lógica da interface.
   Conversa com o Python por pywebview.api.* e recebe progresso por window.__onEvents. */
"use strict";

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const baseName = (p) => String(p).split(/[\\/]/).pop();
const dirName = (p) => { const parts = String(p).split(/[\\/]/); parts.pop(); return parts.join("\\"); };
const extOf = (p) => { const n = baseName(p); const i = n.lastIndexOf("."); return i < 0 ? "" : n.slice(i).toLowerCase(); };
const hms = (s) => {
  if (s == null || !isFinite(s)) return "--:--:--";
  s = Math.max(0, Math.round(s));
  return [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60].map((n) => String(n).padStart(2, "0")).join(":");
};
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

const state = {
  boot: null,
  page: "convert",
  convertMode: "auto",
  convertFormat: "md",
  convertFiles: [],
  batchMode: "auto_md",
  jobRunning: false,
  jobErrors: false,
  trItems: [],
  trSelected: null,
  trRunning: false,
  trPaused: false,
  trLastOutput: null,
  nextId: 1,
  install: null,
};

/* ---------------------------------------------------------------- ponte */

async function call(name, ...args) {
  try {
    const res = await window.pywebview.api[name](...args);
    if (KICK_RE.test(name)) kickExec(); // a Central mostra a tarefa recém-criada na hora
    return res ?? { ok: true };
  } catch (err) {
    return { ok: false, error: String(err && err.message ? err.message : err) };
  }
}
async function callOrNotify(name, ...args) {
  const res = await call(name, ...args);
  if (!res.ok) notify(res.error || "Não foi possível concluir.", "err");
  return res;
}

/* Executa uma ação com retorno visível: botão desabilitado com texto de
   andamento e spinner, barra indeterminada e status no rodapé; ao fim, o
   resultado (sucesso ou erro) aparece na própria tela. Nada de caixas. */
async function busyAction(btn, busyText, fn, doneText) {
  if (btn.disabled) return null;
  const orig = btn.textContent;
  btn.disabled = true; btn.classList.add("loading"); btn.textContent = busyText;
  setStatus(busyText, "busy"); setProgress(null); log(busyText);
  let res = null;
  try { res = await fn(); }
  catch (err) { res = { ok: false, error: String(err && err.message ? err.message : err) }; }
  finally {
    btn.disabled = false; btn.classList.remove("loading"); btn.textContent = orig;
    if (!state.jobRunning && !state.trRunning) hideProgress();
  }
  if (res && res.ok === false) notify(res.error || "Falha.", "err");
  else if (doneText) notify(typeof doneText === "function" ? doneText(res) : doneText);
  return res;
}

/* ------------------------------------------------------------ componentes */

function toast(text, kind = "") {
  const el = document.createElement("div");
  el.className = "toast " + kind;
  const msg = document.createElement("span");
  msg.textContent = text;
  el.appendChild(msg);
  $("#toasts").appendChild(el);
  setTimeout(() => el.remove(), kind === "err" || kind === "warn" ? 12000 : 4200);
  // Avisos e erros gravam uma linha no log da sessão e oferecem "Ver no log", que abre a tela Log nessa linha.
  if (kind === "err" || kind === "warn") {
    call("log_registrar", kind === "err" ? "ERROR" : "WARNING", text).then((res) => {
      if (!res.ok || !res.nome || res.offset == null || !el.isConnected) return;
      const b = document.createElement("button");
      b.type = "button"; b.className = "toast-link"; b.textContent = "Ver no log";
      b.addEventListener("click", () => { el.remove(); irParaLogLinha(res.nome, res.offset); });
      el.appendChild(b);
    });
  }
  return el;
}

/* Aviso/erro DENTRO da janela: aviso rápido, status no rodapé e linha no log. */
function notify(text, kind = "") {
  toast(text, kind);
  setStatus(text, kind === "err" ? "err" : "");
  log((kind === "err" ? "ERRO: " : "") + text);
  if (kind === "err") toggleDock(true);
}

function dialog({ title, body = "", ok = "OK", cancel = "Cancelar", danger = false, html = "" }) {
  return new Promise((resolve) => {
    const back = document.createElement("div");
    back.className = "modal-back";
    back.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-label="${esc(title)}">
      <h3>${esc(title)}</h3><div class="body">${html || esc(body)}</div>
      <div class="modal-actions">${cancel ? `<button class="btn" data-r="0">${esc(cancel)}</button>` : ""}
      <button class="btn ${danger ? "btn-danger" : "btn-primary"}" data-r="1">${esc(ok)}</button></div></div>`;
    const done = (v) => { back.remove(); document.removeEventListener("keydown", onKey); resolve(v); };
    const onKey = (e) => { if (e.key === "Escape") done(false); };
    back.addEventListener("click", (e) => {
      if (e.target === back) done(false);
      const r = e.target.closest("[data-r]");
      if (r) done(r.dataset.r === "1");
    });
    document.addEventListener("keydown", onKey);
    $("#modalRoot").appendChild(back);
    $(`[data-r="1"]`, back).focus();
  });
}

/* ------------------------------------------------------------- navegação */

function showPage(name) {
  state.page = name;
  $$(".page").forEach((p) => p.classList.toggle("active", p.id === "page-" + name));
  $$(".nav-item").forEach((b) => b.classList.toggle("active", b.dataset.page === name));
  $(".pages").scrollTop = 0;
  if (name === "exec") kickExec();
  if (name === "log") abrirPaginaLog(); else clearTimeout(lg.timer);
}

function initTheme() {
  let saved = null;
  try { saved = localStorage.getItem("theme"); } catch (e) { /* armazenamento indisponível */ }
  const dark = saved ? saved === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
  applyTheme(dark);
  $("#themeBtn").addEventListener("click", () => {
    const next = document.documentElement.dataset.theme !== "dark";
    applyTheme(next);
    try { localStorage.setItem("theme", next ? "dark" : "light"); } catch (e) { /* ignora */ }
  });
}
function applyTheme(dark) {
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  $("#themeLabel").textContent = dark ? "Tema claro" : "Tema escuro";
}

/* --------------------------------------------------------------- execução */

function log(text) {
  const pre = $("#dockLog");
  const t = new Date().toTimeString().slice(0, 8);
  pre.textContent += `[${t}] ${text}\n`;
  const lines = pre.textContent.split("\n");
  if (lines.length > 2500) pre.textContent = lines.slice(-2000).join("\n");
  pre.scrollTop = pre.scrollHeight;
}
function setStatus(text, mode) {
  $("#dockStatus").textContent = text;
  $("#dockDot").className = "dot" + (mode === "busy" ? " busy" : mode === "err" ? " err" : "");
}
function setProgress(value, max) {
  const wrap = $("#dockProgress"), track = wrap.querySelector(".track");
  wrap.hidden = false;
  if (value == null) {
    track.classList.add("indet");
    $("#dockPercent").textContent = "";
    $("#dockFill").style.width = "";
  } else {
    track.classList.remove("indet");
    const pct = Math.round((100 * value) / Math.max(1, max));
    $("#dockFill").style.width = pct + "%";
    $("#dockPercent").textContent = pct + "%";
  }
}
function hideProgress() { $("#dockProgress").hidden = true; }
function toggleDock(open) {
  const wrap = $("#dockLogWrap");
  const show = open ?? wrap.hidden;
  wrap.hidden = !show;
  $("#dockToggle").textContent = show ? "Ocultar log" : "Ver log";
  $("#dockToggle").setAttribute("aria-expanded", String(show));
}

function setJobRunning(running) {
  state.jobRunning = running;
  state.jobErrors = false;
  for (const id of ["convertStart", "batchStart"]) $("#" + id).disabled = running;
  for (const id of ["convertCancel", "batchCancel"]) $("#" + id).hidden = !running;
  if (running) { setStatus("Processando...", "busy"); setProgress(null); } else { hideProgress(); }
}

/* -------------------------------------------------------------- converter */

function modeInfo(key) { return state.boot.convert_modes.find((m) => m.key === key); }

function renderModeGrid(container, modes, current, onPick) {
  container.innerHTML = modes.map((m) => `
    <button type="button" class="mode ${m.key === current ? "on" : ""}" role="radio" aria-checked="${m.key === current}" data-key="${m.key}">
      <span class="radio"></span><span><strong>${esc(m.label)}</strong><span class="d">${esc(m.description)}</span></span>
    </button>`).join("");
  container.onclick = (e) => {
    const b = e.target.closest(".mode");
    if (b) onPick(b.dataset.key);
  };
}

function acceptedExts(mode) {
  const ext = state.boot.extensions;
  return ext[mode.accepts] || ext.pdf;
}
const ACCEPT_TEXT = { pdf: "Aceita arquivos PDF.", direct: "Aceita PDF, Word, PowerPoint, Excel, HTML, TXT, CSV, JSON e XML.", images: "Aceita PNG, JPG, TIFF, BMP e WEBP." };

function refreshConvert() {
  const mode = modeInfo(state.convertMode);
  renderModeGrid($("#convertModes"), state.boot.convert_modes, state.convertMode, (key) => { state.convertMode = key; refreshConvert(); });
  $("#convertAccepts").textContent = ACCEPT_TEXT[mode.accepts] || "";
  $("#pagesField").hidden = !mode.pages;
  $("#formatField").hidden = !mode.format;
  $("#combineField").hidden = !mode.combine;

  const exts = acceptedExts(mode);
  const list = $("#convertFiles");
  list.innerHTML = state.convertFiles.map((p, i) => {
    const bad = !exts.includes(extOf(p));
    return `<li><span class="name" ${bad ? 'style="color:var(--danger)"' : ""}>${esc(baseName(p))}</span>
      <span class="dir" title="${esc(p)}">${esc(dirName(p))}</span>
      ${bad ? '<span class="status-chip err">incompatível</span>' : ""}
      <button class="x" data-i="${i}" aria-label="Remover ${esc(baseName(p))}">×</button></li>`;
  }).join("");
  const n = state.convertFiles.length;
  const badCount = state.convertFiles.filter((p) => !exts.includes(extOf(p))).length;
  $("#convertCount").textContent = n ? plural(n, "arquivo", "arquivos") : "";
  $("#convertFilesActions").hidden = !n;
  $("#convertSummary").textContent = !n ? "Selecione ao menos um arquivo."
    : badCount ? `${plural(badCount, "arquivo incompatível", "arquivos incompatíveis")} com “${mode.label}”. Remova-os ou troque o modo.`
    : `${plural(n, "arquivo", "arquivos")} · ${mode.label}`;
}

function addConvertFiles(paths) {
  const exts = acceptedExts(modeInfo(state.convertMode));
  let ignored = 0;
  for (const p of paths) {
    if (!exts.includes(extOf(p))) { ignored++; continue; }
    if (!state.convertFiles.includes(p)) state.convertFiles.push(p);
  }
  if (ignored) toast(`${plural(ignored, "arquivo ignorado", "arquivos ignorados")} (formato incompatível com este modo).`, "err");
  refreshConvert();
}

async function pickConvertFiles() {
  const res = await callOrNotify("pick_files", modeInfo(state.convertMode).accepts);
  if (res.ok && res.paths.length) addConvertFiles(res.paths);
}

async function startConvert(confirmSigned = false) {
  if (state.jobRunning) return;
  const mode = modeInfo(state.convertMode);
  const same = $("#sameFolder").checked;
  const res = await call("start_convert", {
    mode: mode.key,
    files: state.convertFiles,
    same_folder: same,
    output_folder: $("#outFolder").value,
    pages: mode.pages ? $("#pages").value : "",
    format: state.convertFormat,
    combine_images: $("#combineImages").checked,
    confirm_signed: confirmSigned,
  });
  if (!res.ok) return notify(res.error, "err");
  if (res.status === "needs_confirm") {
    const yes = await dialog({
      title: "Assinatura digital detectada",
      body: `${res.signed.join("\n")}\n\nO OCR invalida a assinatura nesses arquivos. O original nunca é sobrescrito. Continuar?`,
      ok: "Continuar", cancel: "Cancelar",
    });
    if (yes) startConvert(true);
    return;
  }
  setJobRunning(true);
}

/* ------------------------------------------------------------------- lote */

async function batchParams(extra = {}) {
  return { mode: state.batchMode, input_folder: $("#batchIn").value, output_folder: $("#batchOut").value, recursive: $("#batchRecursive").checked, ...extra };
}
async function startBatch(confirmSigned = false) {
  if (state.jobRunning) return;
  const scan = await call("scan_batch", await batchParams());
  if (!scan.ok) return notify(scan.error, "err");
  if (!scan.count) return notify("Nada para processar: nenhum arquivo compatível foi encontrado nessa pasta.", "err");
  if (!confirmSigned) {
    const yes = await dialog({
      title: "Iniciar o lote?",
      body: `${plural(scan.count, "arquivo encontrado", "arquivos encontrados")}.\nSaída: ${scan.output_folder}`,
      ok: "Iniciar lote",
    });
    if (!yes) return;
  }
  let signedOk = false;
  if (scan.signed.length) {
    signedOk = await dialog({
      title: "Assinatura digital detectada",
      body: `${scan.signed.slice(0, 12).join("\n")}${scan.signed.length > 12 ? `\n… e mais ${scan.signed.length - 12}` : ""}\n\nO OCR invalida a assinatura nesses arquivos. O original nunca é sobrescrito. Continuar?`,
      ok: "Continuar",
    });
    if (!signedOk) return;
  }
  const res = await callOrNotify("start_batch", await batchParams({ confirm_signed: signedOk }));
  if (res.ok) setJobRunning(true);
}

/* ------------------------------------------------------------- transcrição */

function trMapStatus(s) { return s === "Em andamento" ? "run" : s === "Concluído" ? "ok" : s === "Erro" || s === "Cancelado" ? "err" : ""; }

function refreshTrQueue() {
  const list = $("#trQueue");
  list.innerHTML = state.trItems.map((it, i) => `
    <li data-id="${it.id}" class="${it.id === state.trSelected ? "sel" : ""}">
      <span class="idx">${i + 1}</span><span class="name" title="${esc(it.path)}">${esc(baseName(it.path))}</span>
      <span class="status-chip ${trMapStatus(it.status)}">${esc(it.status)}</span></li>`).join("");
  const pending = state.trItems.filter((i) => i.status === "Pendente").length;
  $("#trSummary").textContent = state.trItems.length ? `${plural(pending, "pendente", "pendentes")} de ${state.trItems.length}` : "";
  $("#trStatus").textContent = state.trRunning ? $("#trStatus").textContent : state.trItems.length ? "Pronto para iniciar." : "Fila vazia.";
  const editable = !state.trRunning;
  for (const id of ["trUp", "trDown", "trRemove", "trClear"]) $("#" + id).disabled = !editable;
  $("#trStart").disabled = state.trRunning;
}

function addTrFiles(paths) {
  const ok = state.boot.extensions.media;
  let ignored = 0;
  for (const p of paths) {
    if (!ok.includes(extOf(p))) { ignored++; continue; }
    if (state.trItems.some((i) => i.path === p)) continue;
    state.trItems.push({ id: "t" + state.nextId++, path: p, status: "Pendente" });
  }
  if (ignored) toast(`${plural(ignored, "arquivo ignorado", "arquivos ignorados")} (não é áudio/vídeo).`, "err");
  refreshTrQueue();
}

function renderTrOptions() {
  const t = state.boot.transcricao;
  const installed = t.installed.length ? t.installed : [t.default_model];
  const cur = $("#trModel").value;
  $("#trModel").innerHTML = installed.map((m) => `<option value="${esc(m)}">${esc(m)}</option>`).join("");
  $("#trModel").value = installed.includes(cur) ? cur : installed.includes(t.default_model) ? t.default_model : installed[0];
  if (!$("#trLang").options.length) {
    $("#trLang").innerHTML = t.languages.map((l) => `<option value="${l}">${l === "auto" ? "automático" : l}</option>`).join("");
    $("#trLang").value = "pt";
    $("#trFormat").innerHTML = t.formats.map((f) => `<option value="${f}">${f}</option>`).join("");
    $("#trFormat").value = "txt";
  }
  refreshModelInfo();
}
function refreshModelInfo() {
  const t = state.boot.transcricao;
  const name = $("#trModel").value;
  const info = t.models[name] || {};
  const where = t.installed.includes(name) ? "instalado neste computador" : "será baixado na primeira vez (exige internet)";
  $("#trModelInfo").textContent = `${info.tamanho_mb ?? "?"} MB · precisão ${String(info.precisao ?? "?").toLowerCase()} · velocidade ${String(info.velocidade ?? "?").toLowerCase()} · ${where}`;
}
async function refreshTrState() {
  const res = await call("transcricao_state");
  if (res.ok) { state.boot.transcricao = res; renderTrOptions(); }
}
async function refreshDeps() {
  const res = await call("transcricao_deps");
  if (!res.ok) return;
  $("#trDeps").innerHTML = res.deps.map((d) => `<span class="chip ${d.ok ? "ok" : "err"}">${d.ok ? "✓" : "✕"} ${esc(d.name)}</span>`).join("");
}

async function startTranscricao() {
  const pending = state.trItems.filter((i) => i.status === "Pendente");
  const res = await call("start_transcricao", {
    items: pending.map((i) => ({ id: i.id, path: i.path })),
    model: $("#trModel").value, language: $("#trLang").value, format: $("#trFormat").value,
    beams: Number($("#trBeams").value) || 5, word_level: $("#trWord").checked,
  });
  if (!res.ok) return notify(res.error, "err");
  setTrRunning(true);
}
function setTrRunning(running) {
  state.trRunning = running;
  state.trPaused = false;
  $("#trPause").disabled = !running;
  $("#trPause").textContent = "Pausar";
  $("#trCancel").disabled = !running;
  $("#navBadgeTr").hidden = !running;
  if (!running) hideProgress(); else setProgress(null);
  refreshTrQueue();
}
function trItem(id) { return state.trItems.find((i) => i.id === id); }

/* ---------------------------------------------------------- diagnóstico */

let diagPath = "";
async function runDiagnose() {
  if (!diagPath) return notify("Diagnóstico: escolha um arquivo primeiro.", "err");
  const res = await busyAction($("#diagRun"), "Analisando…", () => call("diagnose", diagPath), "Diagnóstico concluído.");
  if (!res || !res.ok) return;
  mostrarDiagnostico(res.rows);
}
function mostrarDiagnostico(rows) {
  $("#diagTable tbody").innerHTML = rows.map((r) => `<tr><td>${esc(r.item)}</td><td>${esc(r.result)}</td></tr>`).join("");
  $("#diagCard").hidden = false;
}

/* -------------------------------------------------------- configurações */

const SETTINGS_MAP = { language: "#setLanguage", dpi: "#setDpi", psm: "#setPsm", native_text_threshold: "#setThreshold",
  cpu_mode: "#setCpu", pdf_output_type: "#setPdf", verbosity: "#setVerbosity" };
const SETTINGS_BOOL = { rotate_pages: "#setRotate", deskew: "#setDeskew", resume: "#setResume", auto_open_output: "#setOpen" };

function loadSettings(cfg) {
  for (const [k, sel] of Object.entries(SETTINGS_MAP)) $(sel).value = cfg[k] ?? "";
  for (const [k, sel] of Object.entries(SETTINGS_BOOL)) $(sel).checked = !!cfg[k];
}
function readSettings() {
  const v = {};
  for (const [k, sel] of Object.entries(SETTINGS_MAP)) v[k] = $(sel).value;
  for (const [k, sel] of Object.entries(SETTINGS_BOOL)) v[k] = $(sel).checked;
  return v;
}

/* ----------------------------------------------------- ajuda / instalação */

function checkRowHtml(r) {
  return `<tr><td>${esc(r.name)}</td><td><span class="chip ${r.ok ? "ok" : "err"}">${r.ok ? "OK" : "Falta"}</span></td><td class="muted">${esc(r.detail)}</td></tr>`;
}
function setCheckLine(text, mode) {
  const line = $("#checkLine");
  line.hidden = !text;
  line.className = "checkline " + (mode || "");
  $("#checkLineText").textContent = text || "";
}

/* A verificação roda em thread no Python; cada componente chega por evento
   (check_row) e a tabela vai sendo preenchida na hora. */
async function checkInstallation({ silent = false } = {}) {
  if (state.checking) {
    if (!silent) { state.checkSilent = false; setCheckLine(`Verificando… ${state.checkCount || 0} componente(s) verificado(s)`, "busy"); }
    return;
  }
  state.checking = true; state.checkSilent = silent; state.checkCount = 0;
  const btn = $("#helpCheck");
  btn.disabled = true; btn.classList.add("loading"); btn.textContent = "Verificando…";
  $("#installTable tbody").innerHTML = `<tr><td colspan="3" class="muted empty"><span class="spin"></span> Verificando componentes…</td></tr>`;
  setCheckLine("Verificando… isso pode levar alguns segundos.", "busy");
  if (!silent) { setStatus("Verificando instalação…", "busy"); setProgress(null); log("Verificando instalação…"); }
  const res = await call("start_check_installation");
  if (!res.ok) onCheckError(res.error);
}
function onCheckStart() { $("#installTable tbody").innerHTML = ""; }
function onCheckRow(r) {
  state.checkCount = r.index;
  const body = $("#installTable tbody");
  if (r.index === 1) body.innerHTML = "";
  body.insertAdjacentHTML("beforeend", checkRowHtml(r));
  setCheckLine(`Verificando… ${r.index} componente(s) verificado(s) (último: ${r.name})`, "busy");
  if (!state.checkSilent) setStatus(`Verificando instalação… ${r.name}`, "busy");
}
function endCheck() {
  state.checking = false;
  const btn = $("#helpCheck");
  btn.disabled = false; btn.classList.remove("loading"); btn.textContent = "Verificar instalação";
  if (!state.jobRunning && !state.trRunning) hideProgress();
}
function onCheckDone(res) {
  state.install = res;
  endCheck();
  $("#installTable tbody").innerHTML = res.rows.map(checkRowHtml).join("");
  const missing = [...res.missing_pip, ...(res.missing_model ? ["modelo de transcrição"] : [])];
  $("#installBanner").hidden = !missing.length;
  $("#installBannerText").textContent = missing.length ? "Faltando: " + missing.join(", ") + "." : "";
  const bad = res.rows.filter((r) => !r.ok);
  $("#navBadgeHelp").hidden = !bad.length;
  const summary = bad.length
    ? `Verificação concluída: ${res.rows.length - bad.length} de ${res.rows.length} componentes OK. Faltam: ${bad.map((r) => r.name).join(", ")}.`
    : `Verificação concluída: todos os ${res.rows.length} componentes estão OK.`;
  setCheckLine(summary, bad.length ? "err" : "ok");
  if (!state.checkSilent) { setStatus(summary, bad.length ? "err" : ""); log(summary); toast(summary, bad.length ? "err" : ""); }
}
function onCheckError(text) {
  endCheck();
  const msg = "Não foi possível verificar a instalação: " + (text || "erro desconhecido");
  setCheckLine(msg, "err");
  $("#installTable tbody").innerHTML = `<tr><td colspan="3" class="muted empty">${esc(msg)}</td></tr>`;
  notify(msg, "err");
}

/* ---------------------------------------------------- central de execuções */

/* A Central consulta o Python em intervalos curtos (exec_listar) e atualiza os
   cartões no lugar, sem recriar a lista: foco, cartões abertos e rolagem
   continuam intactos. A consulta pausa sozinha quando a janela está oculta. */
const KICK_RE = /^(start_|install_|clear_|save_|restore_|diagnose|download_|exec_(repetir|cancelar|limpar))/;
const execo = {
  cards: new Map(), dados: new Map(), abertos: new Set(), linhasVistas: new Map(), estadoAnterior: new Map(),
  rodando: 0, naFila: 0, total: 0, timer: null, ocupado: false, pendente: false, ferramentasOk: false,
  filtroEstado: "todas", ferramenta: "", busca: "",
};
const ESTADO_CHIP = { rodando: "run", concluida: "ok", falhou: "err", interrompida: "warn" };
const ETA_FONTE = { andamento: "pelo ritmo atual", historico: "pelo histórico de execuções anteriores", misto: "pelo ritmo atual e pelo histórico" };

function fmtDur(s) {
  if (s == null || !isFinite(s)) return "—";
  s = Math.max(0, Math.round(s));
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = s % 60;
  if (h) return `${h} h ${String(m).padStart(2, "0")} min ${String(sec).padStart(2, "0")} s`;
  if (m) return `${m} min ${String(sec).padStart(2, "0")} s`;
  return `${sec} s`;
}
const fmtData = (t) => (t ? new Date(t * 1000).toLocaleString("pt-BR") : "—");
function fmtBytes(n) {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1048576).toFixed(1)} MB`;
}
async function copyText(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) { await navigator.clipboard.writeText(text); return true; }
  } catch (e) { /* tenta o plano B */ }
  try {
    const ta = document.createElement("textarea");
    ta.value = text; ta.setAttribute("readonly", ""); ta.style.position = "fixed"; ta.style.opacity = "0";
    document.body.appendChild(ta); ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  } catch (e) { return false; }
}

function kickExec() { clearTimeout(execo.timer); execo.timer = setTimeout(pollExec, 0); }
function agendarExec() {
  clearTimeout(execo.timer);
  if (document.hidden) return;
  const rapido = execo.rodando > 0 || execo.naFila > 0 || state.page === "exec";
  execo.timer = setTimeout(pollExec, rapido ? 1000 : 4000);
}
async function pollExec() {
  if (document.hidden) return;
  if (execo.ocupado) { execo.pendente = true; return; }
  execo.ocupado = true;
  try {
    const res = await call("exec_listar");
    if (res.ok) { aplicarExec(res); await Promise.all([...execo.abertos].map(atualizarDetalhe)); }
  } finally { execo.ocupado = false; }
  if (execo.pendente) { execo.pendente = false; kickExec(); return; }
  agendarExec();
}

function passaExec(ex, estado, ferramenta, busca) {
  const ativo = ex.estado === "rodando" || ex.estado === "fila";
  if (estado === "ativas" ? !ativo : estado !== "todas" && ex.estado !== estado) return false;
  if (ferramenta && ex.ferramenta !== ferramenta) return false;
  if (!busca) return true;
  return [ex.rotulo, ex.entrada, ex.etapa, ex.ferramenta_nome, ex.mensagem, ex.erro].join(" ").toLowerCase().includes(busca);
}

function criarCard(ex) {
  const li = document.createElement("li");
  li.className = "exec";
  li.dataset.id = ex.id;
  const dom = "exec-" + ex.id;
  li.innerHTML = `
    <div class="exec-top">
      <div class="exec-title"><strong class="e-nome"></strong><span class="e-entrada muted small"></span></div>
      <span class="status-chip e-estado"></span>
      <button type="button" class="btn btn-sm btn-link e-toggle" aria-expanded="false" aria-controls="${dom}-det">Detalhes</button>
    </div>
    <div class="e-etapa small"></div>
    <div class="exec-bar"><div class="track e-track" role="progressbar" aria-valuemin="0" aria-valuemax="100"><div class="fill e-fill"></div></div><span class="e-pct"></span></div>
    <div class="exec-meta small muted">
      <span class="e-dec-wrap"><span class="e-dec-lbl">Decorrido</span>: <b class="e-dec"></b></span>
      <span class="e-eta-wrap">Restante: <b class="e-eta"></b></span>
      <span class="e-prog"></span>
    </div>
    <div class="exec-acoes">
      <button type="button" class="btn btn-sm btn-danger e-cancelar">Cancelar</button>
      <button type="button" class="btn btn-sm e-repetir">Repetir</button>
      <button type="button" class="btn btn-sm e-copiar">Copiar resumo</button>
      <button type="button" class="btn btn-sm e-log">Ver no log</button>
    </div>
    <div class="exec-det" id="${dom}-det" hidden>
      <dl class="exec-dl"></dl>
      <div class="exec-log-head small muted">Log desta execução</div>
      <pre class="log exec-log" tabindex="0" aria-label="Log desta execução"></pre>
    </div>`;
  return li;
}

function atualizarCard(li, ex) {
  const q = (s) => li.querySelector(s);
  const ativo = ex.estado === "rodando" || ex.estado === "fila";
  li.dataset.estado = ex.estado;
  q(".e-nome").textContent = ex.rotulo;
  q(".e-entrada").textContent = [ex.ferramenta_nome, ex.entrada].filter(Boolean).join(" · ");
  q(".e-entrada").title = ex.entrada || "";
  const chip = q(".e-estado");
  chip.textContent = ex.estado_texto;
  chip.className = "status-chip e-estado " + (ESTADO_CHIP[ex.estado] || "");

  q(".e-etapa").textContent = ex.estado === "fila" ? "Aguardando a vez na fila."
    : ex.estado === "rodando" ? (ex.etapa || "Em andamento…")
    : (ex.erro || ex.mensagem || ex.etapa || "");

  // Barra: determinada só com total conhecido; sem total, indicador indeterminado (nunca porcentagem inventada).
  const barra = q(".exec-bar"), trilha = q(".e-track"), fill = q(".e-fill");
  const mostraBarra = ex.estado === "rodando" || (!ativo && ex.determinada);
  barra.hidden = !mostraBarra;
  if (mostraBarra) {
    const det = ex.determinada;
    trilha.classList.toggle("indet", !det && ex.estado === "rodando");
    trilha.classList.toggle("ok", ex.estado === "concluida");
    trilha.classList.toggle("err", ex.estado === "falhou");
    trilha.classList.toggle("stopped", ex.estado === "cancelada" || ex.estado === "interrompida");
    fill.style.width = det ? ex.progresso + "%" : "";
    q(".e-pct").textContent = det ? Math.round(ex.progresso) + "%" : "";
    trilha.setAttribute("aria-label", "Progresso de " + ex.rotulo);
    if (det) {
      trilha.setAttribute("aria-valuenow", String(Math.round(ex.progresso)));
      trilha.setAttribute("aria-valuetext", Math.round(ex.progresso) + "%" + (ex.progresso_texto ? ", " + ex.progresso_texto : ""));
    } else {
      trilha.removeAttribute("aria-valuenow");
      trilha.setAttribute("aria-valuetext", "em andamento, porcentagem desconhecida");
    }
  }

  q(".e-dec-wrap").hidden = ex.decorrido_s == null;
  q(".e-dec-lbl").textContent = ativo ? "Decorrido" : "Duração";
  q(".e-dec").textContent = fmtDur(ex.decorrido_s);
  const mostraEta = ex.estado === "rodando" && !!ex.eta_texto;
  q(".e-eta-wrap").hidden = !mostraEta;
  q(".e-eta").textContent = ex.eta_texto || "";
  q(".e-eta-wrap").title = ex.eta_s != null
    ? `Entre ${fmtDur(ex.eta_min_s)} e ${fmtDur(ex.eta_max_s)} (${ETA_FONTE[ex.eta_fonte] || "estimativa"})` : "";
  q(".e-prog").textContent = ex.progresso_texto || "";

  const bc = q(".e-cancelar");
  bc.hidden = !(ex.cancelavel || ex.cancelando);
  bc.disabled = !!ex.cancelando;
  bc.textContent = ex.cancelando ? "Cancelando…" : "Cancelar";
  q(".e-repetir").hidden = !ex.repetivel;
  q(".e-log").hidden = !(ex.log_arquivo && ex.log_ini != null);

  if (execo.abertos.has(ex.id)) {
    const dl = [["Ferramenta", ex.ferramenta_nome], ["Entrada", ex.entrada], ["Estado", ex.estado_texto], ["Etapa", ex.etapa],
      ["Progresso", ex.progresso_texto], ["Início", fmtData(ex.inicio_em)], ["Fim", ex.fim_em ? fmtData(ex.fim_em) : ""],
      ["Resultado", ex.mensagem], ["Erro", ex.erro], ["Log", ex.log_arquivo], ["Identificador", ex.id]];
    q(".exec-dl").innerHTML = dl.filter(([, v]) => v).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
  }

  // anúncio para leitores de tela quando o estado muda (aria-live)
  const ant = execo.estadoAnterior.get(ex.id);
  if (ant !== undefined && ant !== ex.estado) {
    $("#execLive").textContent = `${ex.rotulo}: ${ex.estado === "rodando" ? "iniciada" : ex.estado_texto.toLowerCase()}`;
  }
  execo.estadoAnterior.set(ex.id, ex.estado);
}

function aplicarExec(res) {
  execo.rodando = res.rodando; execo.naFila = res.na_fila; execo.total = res.total;
  if (!execo.ferramentasOk) {
    const sel = $("#execFerramenta");
    sel.innerHTML = `<option value="">Todas as ferramentas</option>` + Object.entries(res.ferramentas || {})
      .map(([k, v]) => `<option value="${esc(k)}">${esc(v)}</option>`).join("");
    execo.ferramentasOk = true;
  }
  const ul = $("#execLista");
  const vistos = new Set();
  res.execucoes.forEach((ex, i) => {
    vistos.add(ex.id);
    execo.dados.set(ex.id, ex);
    let li = execo.cards.get(ex.id);
    if (!li) { li = criarCard(ex); execo.cards.set(ex.id, li); }
    atualizarCard(li, ex);
    if (ul.children[i] !== li) ul.insertBefore(li, ul.children[i] || null);
  });
  for (const [id, li] of [...execo.cards]) {
    if (vistos.has(id)) continue;
    li.remove(); execo.cards.delete(id); execo.dados.delete(id);
    execo.abertos.delete(id); execo.linhasVistas.delete(id); execo.estadoAnterior.delete(id);
  }
  aplicarFiltroExec();
  atualizarIndicadores();
}

function aplicarFiltroExec() {
  const busca = execo.busca.trim().toLowerCase();
  let visiveis = 0;
  for (const [id, li] of execo.cards) {
    const ok = passaExec(execo.dados.get(id), execo.filtroEstado, execo.ferramenta, busca);
    li.hidden = !ok;
    if (ok) visiveis++;
  }
  const filtrando = execo.filtroEstado !== "todas" || execo.ferramenta || busca;
  $("#execResumo").textContent = execo.total
    ? `${visiveis} de ${plural(execo.total, "execução", "execuções")} · ${execo.rodando} rodando · ${execo.naFila} na fila`
    : "Nenhuma execução registrada ainda.";
  const vazio = $("#execVazio");
  vazio.hidden = visiveis > 0;
  vazio.textContent = !execo.total ? "Nada foi executado ainda. Converta um arquivo, rode um lote ou verifique a instalação: cada tarefa aparece aqui, ao vivo."
    : filtrando ? "Nenhuma execução corresponde aos filtros." : "";
}

function atualizarIndicadores() {
  const n = execo.rodando, f = execo.naFila;
  const badge = $("#navCountExec");
  badge.hidden = n + f === 0;
  badge.textContent = String(n + f);
  badge.title = `${n} rodando, ${f} na fila`;
  const b = $("#dockExec");
  b.hidden = n + f === 0;
  b.textContent = `${n} rodando` + (f ? ` · ${f} na fila` : "");
}

async function atualizarDetalhe(id) {
  const li = execo.cards.get(id);
  if (!li) return;
  const r = await call("exec_detalhe", id, execo.linhasVistas.get(id) || 0);
  if (!r.ok) return;
  const pre = li.querySelector(".exec-log");
  if (r.linhas.length) {
    if (pre.dataset.vazio) { pre.textContent = ""; delete pre.dataset.vazio; }
    const perto = pre.scrollHeight - pre.scrollTop - pre.clientHeight < 30;
    const rot = { erro: "ERRO: ", aviso: "AVISO: " };
    pre.textContent += r.linhas.map((l) => `[${l.t}] ${rot[l.n] || ""}${l.x}`).join("\n") + "\n";
    if (perto) pre.scrollTop = pre.scrollHeight;
  } else if (!pre.textContent) {
    pre.dataset.vazio = "1";
    pre.textContent = "Esta execução ainda não registrou linhas de log.";
  }
  execo.linhasVistas.set(id, r.proximo);
}

async function repetirExecucao(btn, ex) {
  if (ex.ferramenta === "verificacao" && ex.variante === "") { checkInstallation(); return; }
  const r = await busyAction(btn, "Repetindo…", () => call("exec_repetir", ex.id));
  if (!r || !r.ok) return;
  if (r.status === "needs_confirm") {
    notify("Há PDFs com assinatura digital nesta execução. Repita pela tela Converter, onde a confirmação é pedida.", "err");
    return;
  }
  if (r.acao === "start_convert" || r.acao === "start_batch") setJobRunning(true);
  else if (r.acao === "start_transcricao") setTrRunning(true);
  else if (r.acao === "diagnose" && r.rows) mostrarDiagnostico(r.rows);
  toast("Execução repetida: acompanhe aqui na lista.");
  kickExec();
}

function bindExec() {
  $("#execEstados").addEventListener("click", (e) => {
    const b = e.target.closest("button"); if (!b) return;
    execo.filtroEstado = b.dataset.value;
    $$("#execEstados button").forEach((x) => { x.classList.toggle("on", x === b); x.setAttribute("aria-checked", String(x === b)); });
    aplicarFiltroExec();
  });
  $("#execFerramenta").addEventListener("change", () => { execo.ferramenta = $("#execFerramenta").value; aplicarFiltroExec(); });
  $("#execBusca").addEventListener("input", () => { execo.busca = $("#execBusca").value; aplicarFiltroExec(); });
  $("#execLimpar").addEventListener("click", async () => {
    if (!(await dialog({ title: "Limpar execuções concluídas", body: "Remove da lista (e do histórico em disco) as execuções já terminadas. As que estão rodando ou na fila continuam. O histórico usado nas estimativas de tempo é mantido.", ok: "Limpar" }))) return;
    await busyAction($("#execLimpar"), "Limpando…", () => call("exec_limpar_concluidas"), (r) => `${plural(r.removidas, "execução removida", "execuções removidas")} da lista.`);
    kickExec();
  });
  $("#dockExec").addEventListener("click", () => showPage("exec"));
  $("#execLista").addEventListener("click", async (e) => {
    const li = e.target.closest(".exec"); if (!li) return;
    const id = li.dataset.id, ex = execo.dados.get(id);
    if (!ex) return;
    if (e.target.closest(".e-toggle")) {
      const btn = li.querySelector(".e-toggle"), det = li.querySelector(".exec-det");
      const abrir = det.hidden;
      det.hidden = !abrir;
      btn.setAttribute("aria-expanded", String(abrir));
      btn.textContent = abrir ? "Ocultar" : "Detalhes";
      if (abrir) { execo.abertos.add(id); atualizarCard(li, ex); atualizarDetalhe(id); } else execo.abertos.delete(id);
    } else if (e.target.closest(".e-cancelar")) {
      const b = e.target.closest(".e-cancelar");
      await busyAction(b, "Cancelando…", () => call("exec_cancelar", id));
      kickExec();
    } else if (e.target.closest(".e-repetir")) {
      await repetirExecucao(e.target.closest(".e-repetir"), ex);
    } else if (e.target.closest(".e-copiar")) {
      const r = await callOrNotify("exec_resumo", id);
      if (r.ok) notify((await copyText(r.texto)) ? "Resumo da execução copiado." : "Não foi possível copiar. Abra os detalhes e copie o texto.");
    } else if (e.target.closest(".e-log")) {
      abrirLogDaExecucao(ex);
    }
  });
}

/* --------------------------------------------------------------------- log */

const NV_ROTULO = { ERROR: "ERRO", WARNING: "AVISO", INFO: "INFO", DEBUG: "DEBUG", CRITICAL: "CRÍTICO" };
const NV_PESO = { DEBUG: 10, INFO: 20, WARNING: 30, ERROR: 40, CRITICAL: 50 };
const LOG_MAX_BUFFER = 20000, LOG_MAX_RENDER = 4000;
const lg = {
  iniciado: false, nome: "", atual: "", linhas: [], ini: 0, fim: 0, tamanho: 0, nvAnt: "INFO", pausado: false,
  aoVivo: true, timer: null, ocupado: false, foco: null, marcaLinha: null, modoBusca: false, userTs: 0,
};

function passaLog(l) {
  const lim = { tudo: 0, avisos: 30, erros: 40 }[$("#logNivel").value] || 0;
  if ((NV_PESO[l.nv] || 20) < lim) return false;
  const b = $("#logBusca").value.trim().toLowerCase();
  return !b || (l.t + " " + l.x).toLowerCase().includes(b);
}
function linhaLogHtml(l) {
  let cls = "ll nv-" + l.nv;
  if (!l.t) cls += " cont";
  if ((lg.foco && lg.foco.tag && l.x.includes(`[exec ${lg.foco.tag}]`)) || (lg.marcaLinha != null && l.o === lg.marcaLinha)) cls += " marca";
  const cab = l.t ? `<span class="lt">${esc(l.t.slice(11))}</span><span class="lv">${esc(NV_ROTULO[l.nv] || l.nv)}</span>` : "";
  return `<div class="${cls}" data-o="${l.o}">${cab}<span class="lx">${esc(l.x)}</span></div>`;
}
function infoLog(visiveis, mostradas) {
  const partes = [lg.nome || "—"];
  if (lg.modoBusca) partes.push("resultado de busca");
  else partes.push(lg.pausado ? "pausado" : lg.aoVivo ? "ao vivo" : "arquivo fechado");
  if (visiveis != null) partes.push(`${plural(mostradas, "linha exibida", "linhas exibidas")}${visiveis > mostradas ? ` (últimas de ${visiveis})` : ""}`);
  partes.push(`${plural(lg.linhas.length, "linha carregada", "linhas carregadas")}`);
  if (lg.tamanho) partes.push(fmtBytes(lg.tamanho));
  if (lg.ini > 0 && !lg.modoBusca) partes.push("há linhas anteriores ainda não carregadas");
  $("#logInfo").textContent = partes.join(" · ");
  $("#logAntigas").disabled = lg.ini <= 0 || lg.modoBusca;
}
function renderLog({ rolar = true } = {}) {
  const view = $("#logView");
  const vis = lg.linhas.filter(passaLog);
  const mostrar = vis.length > LOG_MAX_RENDER ? vis.slice(-LOG_MAX_RENDER) : vis;
  view.innerHTML = mostrar.length ? mostrar.map(linhaLogHtml).join("") : `<div class="muted">Nenhuma linha corresponde aos filtros.</div>`;
  view.dataset.vazio = mostrar.length ? "" : "1";
  infoLog(vis.length, mostrar.length);
  if (rolar && $("#logAuto").checked) view.scrollTop = view.scrollHeight;
}
function anexarLog(novas) {
  for (const l of novas) lg.linhas.push(l);
  if (lg.linhas.length > LOG_MAX_BUFFER) { lg.linhas.splice(0, lg.linhas.length - LOG_MAX_BUFFER); renderLog(); return; }
  const vis = novas.filter(passaLog);
  const view = $("#logView");
  if (vis.length) {
    if (view.dataset.vazio) { view.innerHTML = ""; view.dataset.vazio = ""; }
    view.insertAdjacentHTML("beforeend", vis.map(linhaLogHtml).join(""));
    while (view.childElementCount > LOG_MAX_RENDER) view.firstElementChild.remove();
    if ($("#logAuto").checked) view.scrollTop = view.scrollHeight;
  }
  infoLog(lg.linhas.filter(passaLog).length, Math.min(LOG_MAX_RENDER, view.childElementCount));
}
function mostrarFoco(texto) {
  $("#logFoco").hidden = !texto;
  $("#logFocoTexto").textContent = texto || "";
}

async function carregarListaLogs(selecionar) {
  const res = await call("log_arquivos");
  if (!res.ok) return;
  lg.atual = res.atual;
  const grupos = new Map();
  for (const a of res.arquivos) { if (!grupos.has(a.data)) grupos.set(a.data, []); grupos.get(a.data).push(a); }
  const sel = $("#logArquivo");
  sel.innerHTML = [...grupos].map(([data, itens]) => `<optgroup label="${esc(data.split("-").reverse().join("/"))}">` + itens.map((a) =>
    `<option value="${esc(a.nome)}">${esc(a.hora)} · ${esc(a.nome)}${a.atual ? " (sessão atual)" : ""}${a.tipo === "instalacao" ? " (instalador)" : ""} · ${fmtBytes(a.tamanho)}</option>`).join("") + "</optgroup>").join("");
  const alvo = selecionar || lg.nome || lg.atual;
  if (![...sel.options].some((o) => o.value === alvo) && alvo) sel.insertAdjacentHTML("afterbegin", `<option value="${esc(alvo)}">${esc(alvo)} (não está mais na pasta)</option>`);
  sel.value = alvo;
}

async function carregarLog(nome, params = { modo: "cauda" }, opts = {}) {
  lg.ocupado = true;
  let res;
  try { res = await call("log_ler", { nome, ...params }); } finally { lg.ocupado = false; }
  if (!res.ok) {
    lg.linhas = []; lg.fim = 0; lg.ini = 0; lg.aoVivo = false; lg.nome = nome; lg.marcaLinha = null;
    renderLog();
    $("#logInfo").textContent = res.error || "Não foi possível ler o log.";
    return false;
  }
  lg.nome = res.nome; lg.linhas = res.linhas; lg.ini = res.ini; lg.fim = res.fim; lg.tamanho = res.tamanho;
  lg.nvAnt = res.linhas.length ? res.linhas[res.linhas.length - 1].nv : "INFO";
  lg.aoVivo = opts.aoVivo ?? !!res.ao_vivo;
  lg.modoBusca = false; lg.marcaLinha = null;
  if (opts.marcaOffset != null) {
    const alvo = res.linhas.find((l) => l.o >= opts.marcaOffset) || res.linhas[res.linhas.length - 1];
    lg.marcaLinha = alvo ? alvo.o : null;
  }
  const sel = $("#logArquivo");
  if ([...sel.options].some((o) => o.value === res.nome)) sel.value = res.nome;
  renderLog({ rolar: opts.rolar !== false });
  agendarLog();
  return true;
}

function agendarLog() {
  clearTimeout(lg.timer);
  if (state.page !== "log" || document.hidden || lg.pausado || !lg.aoVivo) return;
  lg.timer = setTimeout(pollLog, 1000);
}
async function pollLog() {
  if (state.page !== "log" || document.hidden || lg.pausado || !lg.aoVivo) return;
  if (lg.ocupado) { agendarLog(); return; }
  lg.ocupado = true;
  try {
    for (let i = 0; i < 20; i++) { // se o trecho veio cheio, continua até alcançar o fim do arquivo
      const res = await call("log_ler", { nome: lg.nome, modo: "desde", offset: lg.fim, nv_ant: lg.nvAnt });
      if (!res.ok) break;
      lg.tamanho = res.tamanho;
      if (res.reiniciado) { lg.linhas = res.linhas; lg.ini = res.ini; lg.fim = res.fim; renderLog(); }
      else {
        lg.fim = res.fim;
        if (res.linhas.length) anexarLog(res.linhas); else infoLog(lg.linhas.filter(passaLog).length, $("#logView").childElementCount);
      }
      if (res.linhas.length) lg.nvAnt = res.linhas[res.linhas.length - 1].nv;
      if (!res.mais) break;
    }
  } finally { lg.ocupado = false; }
  agendarLog();
}

async function abrirPaginaLog() {
  if (!lg.iniciado) {
    lg.iniciado = true;
    await carregarListaLogs();
    await carregarLog(lg.atual);
  } else {
    carregarListaLogs(lg.nome);
    if (lg.aoVivo && !lg.pausado) pollLog();
  }
}

async function abrirLogDaExecucao(ex) {
  lg.iniciado = true;
  showPage("log");
  clearTimeout(lg.timer);
  await carregarListaLogs(ex.log_arquivo);
  $("#logNivel").value = "tudo"; $("#logBusca").value = "";
  lg.pausado = false; $("#logPausar").textContent = "Pausar"; $("#logPausar").setAttribute("aria-pressed", "false");
  lg.foco = { tag: ex.id, titulo: ex.rotulo };
  mostrarFoco(`Mostrando o período da execução “${ex.rotulo}”. As linhas com contorno foram gravadas por ela; as outras são de tarefas que rodaram ao mesmo tempo.`);
  const ok = await carregarLog(ex.log_arquivo, { modo: "intervalo", ini: ex.log_ini, fim: ex.log_fim }, { aoVivo: ex.log_fim == null, rolar: false });
  if (!ok) return;
  const el = $(".ll.marca", $("#logView"));
  if (el) el.scrollIntoView({ block: "center" });
  else $("#logView").scrollTop = $("#logView").scrollHeight;
}

/* "Ver no log" dos avisos rápidos: abre o log na linha exata que o aviso gravou. */
async function irParaLogLinha(nome, offset) {
  lg.iniciado = true;
  showPage("log");
  clearTimeout(lg.timer);
  await carregarListaLogs(nome);
  lg.foco = null; mostrarFoco("");
  $("#logNivel").value = "tudo"; $("#logBusca").value = "";
  $("#logAuto").checked = false;
  const ok = await carregarLog(nome, { modo: "ao_redor", offset }, { aoVivo: nome === lg.atual, marcaOffset: offset, rolar: false });
  if (!ok) return;
  const el = $(".ll.marca", $("#logView"));
  if (el) { el.scrollIntoView({ block: "center" }); el.focus?.(); }
}

async function carregarAnteriores() {
  if (lg.ini <= 0 || lg.modoBusca) return;
  const res = await call("log_ler", { nome: lg.nome, modo: "antes", offset: lg.ini });
  if (!res.ok) return;
  lg.linhas = res.linhas.concat(lg.linhas);
  lg.ini = res.ini;
  renderLog({ rolar: false });
  $("#logView").scrollTop = 0;
}

async function buscarNoArquivoInteiro() {
  const res = await call("log_buscar", { nome: lg.nome, nivel: $("#logNivel").value, texto: $("#logBusca").value.trim(), limite: 2000 });
  if (!res.ok) return notify(res.error || "Não foi possível buscar no arquivo.", "err");
  lg.linhas = res.linhas; lg.ini = 0; lg.fim = res.fim; lg.tamanho = res.tamanho; lg.modoBusca = true; lg.aoVivo = false;
  lg.foco = null; lg.marcaLinha = null;
  mostrarFoco(`Busca no arquivo inteiro: ${plural(res.total_achadas, "linha encontrada", "linhas encontradas")}${res.cortada ? ` (mostrando as últimas ${res.linhas.length})` : ""}. O filtro de nível e o texto acima foram aplicados ao arquivo todo.`);
  renderLog();
  clearTimeout(lg.timer);
}

function bindLog() {
  $("#logArquivo").addEventListener("change", async () => {
    lg.foco = null; mostrarFoco("");
    await carregarLog($("#logArquivo").value);
  });
  $("#logNivel").addEventListener("change", () => renderLog());
  $("#logBusca").addEventListener("input", () => renderLog());
  $("#logPausar").addEventListener("click", () => {
    lg.pausado = !lg.pausado;
    $("#logPausar").textContent = lg.pausado ? "Continuar" : "Pausar";
    $("#logPausar").setAttribute("aria-pressed", String(lg.pausado));
    infoLog(lg.linhas.filter(passaLog).length, $("#logView").childElementCount);
    if (!lg.pausado) pollLog(); else clearTimeout(lg.timer);
  });
  $("#logAuto").addEventListener("change", () => { if ($("#logAuto").checked) $("#logView").scrollTop = $("#logView").scrollHeight; });
  const view = $("#logView");
  for (const tipo of ["wheel", "keydown", "pointerdown", "touchmove"]) view.addEventListener(tipo, () => { lg.userTs = Date.now(); }, { passive: true });
  view.addEventListener("scroll", () => {
    if (Date.now() - lg.userTs > 600) return; // só rolagem feita pelo usuário muda o "seguir o fim"
    const perto = view.scrollHeight - view.scrollTop - view.clientHeight < 40;
    if ($("#logAuto").checked !== perto) $("#logAuto").checked = perto;
  });
  $("#logAntigas").addEventListener("click", () => busyAction($("#logAntigas"), "Carregando…", carregarAnteriores));
  $("#logBuscarTudo").addEventListener("click", () => busyAction($("#logBuscarTudo"), "Buscando…", buscarNoArquivoInteiro));
  $("#logCopiar").addEventListener("click", async () => {
    const linhas = lg.linhas.filter(passaLog);
    if (!linhas.length) return notify("Não há linhas visíveis para copiar.");
    const texto = linhas.map((l) => (l.t ? `${l.t} | ${l.nv} | ${l.x}` : l.x)).join("\n");
    notify((await copyText(texto)) ? `Log copiado: ${plural(linhas.length, "linha", "linhas")}.` : "Não foi possível copiar o log.");
  });
  $("#logPasta").addEventListener("click", async () => {
    const r = await callOrNotify("log_abrir_pasta");
    if (r.ok) notify("Pasta dos logs aberta no Explorador de Arquivos.");
  });
  $("#logFocoSair").addEventListener("click", async () => {
    lg.foco = null; lg.marcaLinha = null; mostrarFoco("");
    await carregarLog(lg.nome);
  });
}

/* --------------------------------------------------------------- eventos */

function handleEvent(ev) {
  switch (ev.type) {
    case "line": log(ev.text); break;
    case "status": setStatus(ev.text, "busy"); break;
    case "file_start":
      log(`[${ev.index}/${ev.total}] ${ev.name}`);
      setStatus(`Arquivo ${ev.index}/${ev.total}: ${ev.name}`, "busy");
      if (ev.total > 1) setProgress(ev.index - 1, ev.total); else setProgress(null);
      break;
    case "start": log(`Iniciando OCR: ${ev.file} (${ev.total} página(s))`); break;
    case "page":
      setStatus(`Página ${ev.current}/${ev.total}${ev.source ? " (" + ev.source + ")" : ""}`, "busy");
      setProgress(ev.current, ev.total); break;
    case "image":
      setStatus(`Imagem ${ev.current}/${ev.total}: ${ev.name || ""}`, "busy");
      setProgress(ev.current, ev.total); break;
    case "file_error": log(`FALHA em ${ev.name}: ${ev.text}`); state.jobErrors = true; break;
    case "done": {
      const errs = state.jobErrors;
      setJobRunning(false);
      setStatus(errs ? "Concluído com falhas (veja o log)." : "Concluído.", errs ? "err" : "");
      log("Operação concluída.");
      toast(errs ? "Concluído com falhas. Veja o log." : "Concluído.", errs ? "err" : "");
      if (errs) toggleDock(true);
      break; }
    case "cancelled": setJobRunning(false); setStatus("Cancelado."); log("Operação cancelada pelo usuário."); break;
    case "error":
      setJobRunning(false); notify(ev.text || "Erro desconhecido.", "err"); break;
    case "close_blocked":
      notify("Há uma operação em andamento. Feche a janela de novo para encerrar mesmo assim (ela será cancelada).", "err"); break;
    case "check_start": onCheckStart(); break;
    case "check_row": onCheckRow(ev); break;
    case "check_done": onCheckDone(ev); break;
    case "check_error": onCheckError(ev.text); break;

    case "transc_item_start": {
      const it = trItem(ev.id); if (it) it.status = "Em andamento";
      state.trSelected = null;
      log(`[${ev.index}/${ev.total}] Transcrevendo: ${ev.name}`);
      $("#trStatus").textContent = `Arquivo ${ev.index}/${ev.total}: ${ev.name}`;
      setStatus(`Transcrevendo ${ev.index}/${ev.total}: ${ev.name}`, "busy");
      $("#trBar").style.width = "0%";
      $("#trDone").textContent = "00:00:00 / 00:00:00"; $("#trLeft").textContent = "00:00:00";
      $("#trElapsed").textContent = "00:00:00"; $("#trEta").textContent = "--:--:--";
      refreshTrQueue(); break; }
    case "transc_progress":
      $("#trBar").style.width = Math.min(100, ev.pct) + "%";
      $("#trDone").textContent = `${hms(ev.processado_s)} / ${hms(ev.total_s)} (${Math.round(ev.pct)}%)`;
      $("#trLeft").textContent = hms(ev.restante_s);
      $("#trElapsed").textContent = hms(ev.elapsed_s);
      $("#trEta").textContent = hms(ev.eta_s);
      setProgress(ev.pct, 100); break;
    case "transc_item_done": {
      const it = trItem(ev.id); if (it) it.status = "Concluído";
      log("Transcrição concluída: " + ev.output_path);
      state.trLastOutput = ev.output_path; $("#trOpen").disabled = false; refreshTrQueue(); break; }
    case "transc_item_cancelled": { const it = trItem(ev.id); if (it) it.status = "Cancelado"; log("Item de transcrição cancelado."); refreshTrQueue(); break; }
    case "transc_item_error": { const it = trItem(ev.id); if (it) it.status = "Erro"; log("FALHA na transcrição: " + ev.text); refreshTrQueue(); break; }
    case "transc_queue_done":
      setTrRunning(false);
      $("#trStatus").textContent = "Fila concluída.";
      setStatus("Fila de transcrição concluída."); log("Fila de transcrição encerrada.");
      toast("Fila de transcrição concluída."); break;

    case "model_download": {
      const st = $("#dlStatus");
      if (ev.status === "done") { if (st) st.textContent = `Modelo '${ev.name}' pronto.`; toast(`Modelo '${ev.name}' pronto.`); refreshTrState(); }
      else if (st) st.textContent = "Falha: " + ev.text;
      const b = $("#dlGo"); if (b) b.disabled = false;
      break; }
    case "install_fix_done":
      log("Instalação automática concluída. Verificando novamente...");
      { const b = $("#installFix"); b.disabled = false; b.classList.remove("loading"); b.textContent = "Instalar o que falta"; }
      if (!state.jobRunning && !state.trRunning) hideProgress();
      checkInstallation({ silent: true }); refreshTrState(); refreshDeps(); break;
    case "files_dropped": onFilesDropped(ev.paths || []); break;
    default: break;
  }
}
window.__onEvents = (events) => events.forEach(handleEvent);

function onFilesDropped(paths) {
  if (state.page === "convert") addConvertFiles(paths);
  else if (state.page === "transcribe") addTrFiles(paths);
  else if (state.page === "diagnose" && paths[0]) { diagPath = paths[0]; $("#diagPath").value = paths[0]; }
}

/* --------------------------------------------------------------- ligações */

function bindDropzone(el, onClick) {
  el.addEventListener("click", onClick);
  el.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onClick(); } });
  el.addEventListener("dragover", (e) => { e.preventDefault(); el.classList.add("over"); });
  el.addEventListener("dragleave", () => el.classList.remove("over"));
  el.addEventListener("drop", () => el.classList.remove("over"));
}

function bind() {
  $("#nav").addEventListener("click", (e) => { const b = e.target.closest(".nav-item"); if (b) showPage(b.dataset.page); });

  // Converter
  bindDropzone($("#convertDrop"), pickConvertFiles);
  $("#convertFiles").addEventListener("click", (e) => {
    const x = e.target.closest(".x"); if (!x) return;
    state.convertFiles.splice(Number(x.dataset.i), 1); refreshConvert();
  });
  $("#convertClear").addEventListener("click", () => { state.convertFiles = []; refreshConvert(); });
  $("#sameFolder").addEventListener("change", () => { $("#outFolderRow").hidden = $("#sameFolder").checked; });
  $("#outFolderPick").addEventListener("click", async () => { const r = await callOrNotify("pick_folder"); if (r.ok && r.path) $("#outFolder").value = r.path; });
  $("#convertFormat").addEventListener("click", (e) => {
    const b = e.target.closest("button"); if (!b) return;
    state.convertFormat = b.dataset.value;
    $$("#convertFormat button").forEach((x) => x.classList.toggle("on", x === b));
  });
  $("#convertStart").addEventListener("click", () => startConvert());
  $("#convertCancel").addEventListener("click", () => { call("cancel_job"); setStatus("Cancelando…", "busy"); });

  // Lote
  $("#batchInPick").addEventListener("click", async () => {
    const r = await callOrNotify("pick_folder"); if (!(r.ok && r.path)) return;
    $("#batchIn").value = r.path;
    if (!$("#batchOut").value) $("#batchOut").value = r.path + "\\Extrator_saida";
    $("#batchSummary").textContent = "Pasta escolhida. Selecione o modo e inicie.";
  });
  $("#batchOutPick").addEventListener("click", async () => { const r = await callOrNotify("pick_folder"); if (r.ok && r.path) $("#batchOut").value = r.path; });
  $("#batchStart").addEventListener("click", () => startBatch());
  $("#batchCancel").addEventListener("click", () => { call("cancel_job"); setStatus("Cancelando…", "busy"); });

  // Transcrição
  bindDropzone($("#trDrop"), async () => { const r = await callOrNotify("pick_files", "media"); if (r.ok && r.paths.length) addTrFiles(r.paths); });
  $("#trAdd").addEventListener("click", async () => { const r = await callOrNotify("pick_files", "media"); if (r.ok && r.paths.length) addTrFiles(r.paths); });
  $("#trQueue").addEventListener("click", (e) => { const li = e.target.closest("li"); if (li) { state.trSelected = li.dataset.id; refreshTrQueue(); } });
  $("#trRemove").addEventListener("click", () => { state.trItems = state.trItems.filter((i) => i.id !== state.trSelected); state.trSelected = null; refreshTrQueue(); });
  $("#trClear").addEventListener("click", () => { state.trItems = []; state.trSelected = null; refreshTrQueue(); });
  const move = (d) => {
    const i = state.trItems.findIndex((x) => x.id === state.trSelected), j = i + d;
    if (i < 0 || j < 0 || j >= state.trItems.length) return;
    [state.trItems[i], state.trItems[j]] = [state.trItems[j], state.trItems[i]]; refreshTrQueue();
  };
  $("#trUp").addEventListener("click", () => move(-1));
  $("#trDown").addEventListener("click", () => move(1));
  $("#trModel").addEventListener("change", refreshModelInfo);
  $("#trStart").addEventListener("click", () => {
    if (!state.trItems.some((i) => i.status === "Pendente")) return notify("Fila vazia: adicione ao menos um arquivo pendente.", "err");
    startTranscricao();
  });
  $("#trPause").addEventListener("click", async () => {
    const r = await callOrNotify("toggle_pause_transcricao"); if (!r.ok) return;
    state.trPaused = r.paused; $("#trPause").textContent = r.paused ? "Continuar" : "Pausar";
    setStatus(r.paused ? "Transcrição pausada." : "Transcrevendo...", r.paused ? "" : "busy");
  });
  $("#trCancel").addEventListener("click", () => { call("cancel_transcricao"); $("#trStatus").textContent = "Cancelando..."; setStatus("Cancelando…", "busy"); });
  $("#trOpen").addEventListener("click", () => state.trLastOutput && callOrNotify("open_path", state.trLastOutput));
  $("#trDownload").addEventListener("click", async () => {
    const t = state.boot.transcricao;
    const candidates = Object.keys(t.models).filter((m) => !t.installed.includes(m));
    if (!candidates.length) return notify("Todos os modelos já estão instalados neste computador.");
    const html = `<div class="field"><label for="dlModel">Modelo a baixar (exige internet)</label>
      <select class="input" id="dlModel">${candidates.map((m) => `<option value="${m}">${m} · ${t.models[m].tamanho_mb} MB · ${esc(t.models[m].descricao)}</option>`).join("")}</select></div>
      <div class="muted small" id="dlStatus"></div>`;
    const back = document.createElement("div");
    back.className = "modal-back";
    back.innerHTML = `<div class="modal" role="dialog" aria-modal="true"><h3>Baixar outro modelo</h3><div class="body">${html}</div>
      <div class="modal-actions"><button class="btn" id="dlClose">Fechar</button><button class="btn btn-primary" id="dlGo">Baixar</button></div></div>`;
    $("#modalRoot").appendChild(back);
    $("#dlClose").onclick = () => back.remove();
    $("#dlGo").onclick = async () => {
      $("#dlGo").disabled = true; $("#dlStatus").textContent = "Baixando… pode demorar bastante.";
      const r = await call("download_model", $("#dlModel").value);
      if (!r.ok) { $("#dlStatus").textContent = r.error; $("#dlGo").disabled = false; }
    };
  });

  // Diagnóstico
  $("#diagPick").addEventListener("click", async () => {
    const r = await callOrNotify("pick_files", "diagnose"); if (r.ok && r.paths.length) { diagPath = r.paths[0]; $("#diagPath").value = diagPath; }
  });
  $("#diagRun").addEventListener("click", runDiagnose);

  // Configurações
  $("#setSave").addEventListener("click", async () => {
    const r = await busyAction($("#setSave"), "Salvando…", () => call("save_settings", readSettings()), "Configurações salvas.");
    if (r && r.ok) { loadSettings(r.config); $("#settingsMsg").textContent = "Configurações salvas."; }
  });
  $("#setRestore").addEventListener("click", async () => {
    if (!(await dialog({ title: "Restaurar padrões", body: "Voltar todas as configurações ao padrão?", ok: "Restaurar" }))) return;
    const r = await busyAction($("#setRestore"), "Restaurando…", () => call("restore_settings"), "Padrões restaurados.");
    if (r && r.ok) { loadSettings(r.config); $("#settingsMsg").textContent = "Padrões restaurados."; }
  });

  // Ajuda
  $("#helpCheck").addEventListener("click", () => checkInstallation());
  $("#helpData").addEventListener("click", async () => {
    const r = await callOrNotify("open_data_folder");
    if (r.ok) notify("Pasta de dados aberta no Explorador de Arquivos.");
  });
  $("#helpClear").addEventListener("click", async () => {
    if (!(await dialog({ title: "Limpar cache e logs", body: `Remove todos os logs e o cache de retomada do OCR em:\n${state.boot.data_dir}\n\nA configuração é mantida.`, ok: "Limpar", danger: true }))) return;
    await busyAction($("#helpClear"), "Limpando…", () => call("clear_data"), (r) => `Cache limpo: removidos ${r.logs} log(s) e ${r.jobs} pasta(s) de cache.`);
  });
  $("#installFix").addEventListener("click", async () => {
    if (!(await dialog({ title: "Instalar o que falta", body: "As bibliotecas ausentes serão instaladas via pip e o modelo de transcrição baixado (exige internet). Continuar?", ok: "Instalar" }))) return;
    const r = await callOrNotify("install_missing");
    if (r.ok && r.status === "started") {
      const b = $("#installFix"); b.disabled = true; b.classList.add("loading"); b.textContent = "Instalando…";
      setStatus("Instalando o que falta…", "busy"); setProgress(null); toggleDock(true); log("Instalando o que falta…");
    } else if (r.ok) notify("Nada a instalar: tudo já está presente.");
  });

  // Rodapé de execução
  $("#dockToggle").addEventListener("click", () => toggleDock());
  $("#dockClear").addEventListener("click", () => { $("#dockLog").textContent = ""; });

  // Central de execuções e tela de log
  bindExec();
  bindLog();
  // Janela oculta/minimizada: pausa consultas e animações; ao voltar, atualiza na hora.
  document.addEventListener("visibilitychange", () => {
    document.documentElement.classList.toggle("oculta", document.hidden);
    if (document.hidden) { clearTimeout(execo.timer); clearTimeout(lg.timer); return; }
    kickExec();
    if (state.page === "log" && lg.iniciado) pollLog();
  });
}

/* ------------------------------------------------------------------ início */

async function init() {
  initTheme();
  const boot = await call("bootstrap");
  if (!boot.ok) {
    document.body.innerHTML = `<div style="padding:40px;max-width:560px;margin:auto"><h1>Não foi possível iniciar</h1><p>${esc(boot.error)}</p></div>`;
    return;
  }
  state.boot = boot;
  document.title = `Extrator de Texto · v${boot.version}`;
  $("#appVersion").textContent = `v${boot.version}`;
  $("#helpPaths").textContent = `Pasta de dados: ${boot.data_dir}  ·  Log desta sessão: ${boot.log_file}`;

  bind();
  const pickBatch = (key) => { state.batchMode = key; renderModeGrid($("#batchModes"), boot.batch_modes, key, pickBatch); };
  renderModeGrid($("#batchModes"), boot.batch_modes, state.batchMode, pickBatch);

  refreshConvert();
  renderTrOptions();
  refreshTrQueue();
  loadSettings(boot.config);
  refreshDeps();
  if (boot.busy && (boot.busy.ocr || boot.busy.transcricao)) setStatus("Operação em andamento...", "busy");
  kickExec();
  setTimeout(() => checkInstallation({ silent: true }), 600);
}

(function start() {
  let started = false;
  const go = () => { if (!started) { started = true; init(); } };
  if (window.pywebview && window.pywebview.api) return go();
  window.addEventListener("pywebviewready", go);
  // Desenvolvimento: abrindo index.html?mock num navegador comum, usa o simulador.
  // Nunca por tempo: num computador lento o pywebview pode demorar a ficar pronto.
  if (new URLSearchParams(location.search).has("mock")) {
    const s = document.createElement("script");
    s.src = "dev/mock.js";
    s.onload = go;
    document.head.appendChild(s);
  }
})();
