"""
Extrator de Texto - ponte entre a interface HTML e o motor.

A classe Api e entregue ao pywebview (js_api): cada metodo publico pode ser
chamado do JavaScript como `await pywebview.api.metodo(...)` e devolve sempre
um dicionario serializavel em JSON. Todo trabalho pesado roda em threads; o
progresso volta para a interface como eventos (ver `drain_events`), que o
extrator.py empurra para o JavaScript.

Este modulo nao importa o pywebview no topo: assim ele pode ser testado sem
janela. A janela e injetada por `attach_window()`.

Atencao: o pywebview expoe ao JS todo atributo publico. Por isso todo estado
interno comeca com underscore.
"""
from __future__ import annotations

import contextlib
import copy
import functools
import logging
import queue
import subprocess
import sys
import threading
import uuid
from pathlib import Path

import extrator_core as core
import execucoes
import leitorlog
import transcricao_core as transcricao

CONVERT_MODES = [
    {
        "key": "auto", "label": "OCR automático",
        "description": "Mantém o texto nativo e aplica OCR só onde falta. Gera Markdown ou TXT.",
        "accepts": "pdf", "pages": True, "format": True, "combine": False,
    },
    {
        "key": "force", "label": "OCR forçado",
        "description": "Reconhece todas as páginas, mesmo as que já têm texto. Mais lento.",
        "accepts": "pdf", "pages": True, "format": True, "combine": False,
    },
    {
        "key": "direct", "label": "Conversão direta",
        "description": "Converte PDF, Word, Excel, PowerPoint, HTML e outros para Markdown, sem OCR.",
        "accepts": "direct", "pages": False, "format": False, "combine": False,
    },
    {
        "key": "searchable_skip", "label": "PDF pesquisável",
        "description": "Mantém a aparência do PDF e adiciona uma camada de texto pesquisável.",
        "accepts": "pdf", "pages": True, "format": False, "combine": False,
    },
    {
        "key": "searchable_skip_md", "label": "PDF pesquisável + Markdown",
        "description": "Gera o PDF pesquisável e também o Markdown dele.",
        "accepts": "pdf", "pages": True, "format": False, "combine": False,
    },
    {
        "key": "searchable_force", "label": "PDF pesquisável (OCR forçado)",
        "description": "Refaz o OCR de todas as páginas ao gerar o PDF pesquisável.",
        "accepts": "pdf", "pages": True, "format": False, "combine": False,
    },
    {
        "key": "searchable_redo", "label": "Refazer OCR existente",
        "description": "Descarta o OCR antigo do PDF e faz de novo.",
        "accepts": "pdf", "pages": True, "format": False, "combine": False,
    },
    {
        "key": "images", "label": "OCR de imagens",
        "description": "Reconhece texto em PNG, JPG, TIFF, BMP e WEBP, podendo juntar tudo em um arquivo.",
        "accepts": "images", "pages": False, "format": True, "combine": True,
    },
]
_MODE_BY_KEY = {mode["key"]: mode for mode in CONVERT_MODES}
SEARCHABLE_MODE_KEYS = {"searchable_skip", "searchable_skip_md", "searchable_force", "searchable_redo"}

BATCH_MODES = [
    {"key": "auto_md", "label": "OCR automático para Markdown",
     "description": "Usa o texto nativo e faz OCR só nas páginas sem texto."},
    {"key": "force_md", "label": "OCR forçado para Markdown",
     "description": "Reconhece todas as páginas. Mais lento."},
    {"key": "direct", "label": "Conversão direta",
     "description": "Sem OCR. Aceita PDF, Word, Excel, PowerPoint, HTML e mais."},
    {"key": "searchable", "label": "PDF pesquisável",
     "description": "Adiciona camada de texto mantendo a aparência."},
    {"key": "searchable_md", "label": "PDF pesquisável + Markdown",
     "description": "Gera o PDF pesquisável e o Markdown dele."},
]
SEARCHABLE_BATCH_KEYS = {"searchable", "searchable_md"}

_VERBOSITY = ["normal", "detalhado", "depuracao"]
_CPU_MODES = ["economico", "normal", "maximo"]
_PDF_TYPES = ["pdf", "pdfa"]
_INT_SETTINGS = {"dpi": (100, 600), "psm": (0, 13), "native_text_threshold": (0, 2000)}
_BOOL_SETTINGS = ("rotate_pages", "deskew", "resume", "auto_open_output")

_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def _safe(func):
    """Converte qualquer excecao em {"ok": False, "error": ...} para a interface
    mostrar a mensagem em vez de ficar com uma promessa rejeitada."""

    @functools.wraps(func)
    def wrapper(self, *args, **kwargs):
        try:
            return func(self, *args, **kwargs)
        except Exception as exc:  # noqa: BLE001 - fronteira com a interface
            logging.exception("Erro em Api.%s", func.__name__)
            return {"ok": False, "error": str(exc) or exc.__class__.__name__}

    return wrapper


def _error(message: str) -> dict:
    return {"ok": False, "error": message}


class _OuEvento:
    """Evento somente leitura: vale quando qualquer um dos eventos esta sinalizado
    (cancelar so este item da fila OU a fila inteira)."""

    def __init__(self, *eventos) -> None:
        self._eventos = eventos

    def is_set(self) -> bool:
        return any(evento.is_set() for evento in self._eventos)

    def set(self) -> None:
        self._eventos[0].set()


def _resumo_entrada(caminhos) -> str:
    nomes = [Path(str(c)).name for c in caminhos]
    if len(nomes) <= 3:
        return ", ".join(nomes)
    return f"{len(nomes)} arquivos: " + ", ".join(nomes[:3]) + "…"


# Acoes que o botao "Repetir" da Central pode reexecutar (so as que o usuario ja
# confirmou na tela de origem; limpeza, instalacao e configuracoes ficam de fora).
_REPETIVEIS = {"start_convert", "start_batch", "start_transcricao", "start_check_installation",
               "diagnose", "download_model"}
_NIVEIS_LOG = {"ERROR": logging.ERROR, "WARNING": logging.WARNING, "INFO": logging.INFO}


def _signed(path: Path) -> bool:
    try:
        return core.pdf_is_signed(path)
    except Exception:
        return False


def _file_filter(accepts: str) -> tuple[str, ...]:
    if accepts == "images":
        return ("Imagens (*.png;*.jpg;*.jpeg;*.tif;*.tiff;*.bmp;*.webp)", "Todos os arquivos (*.*)")
    if accepts == "direct":
        return (
            "Documentos (*.pdf;*.docx;*.pptx;*.xlsx;*.html;*.htm;*.txt;*.csv;*.json;*.xml)",
            "Todos os arquivos (*.*)",
        )
    if accepts == "media":
        exts = ";".join(f"*{ext}" for ext in sorted(transcricao.ENTRADA_EXTS))
        return (f"Áudio e vídeo ({exts})", "Todos os arquivos (*.*)")
    if accepts == "diagnose":
        return ("PDF e imagens (*.pdf;*.png;*.jpg;*.jpeg;*.tif;*.tiff;*.bmp;*.webp)", "Todos os arquivos (*.*)")
    return ("PDF (*.pdf)", "Todos os arquivos (*.*)")


class Api:
    def __init__(self) -> None:
        self._events: queue.Queue = queue.Queue()
        self._window = None
        self._job_thread: threading.Thread | None = None
        self._cancel = threading.Event()
        self._job_errors = False
        self._tr_thread: threading.Thread | None = None
        self._tr_cancel = threading.Event()
        self._tr_pause = threading.Event()
        self._install_thread: threading.Thread | None = None
        self._check_thread: threading.Thread | None = None
        self._download_thread: threading.Thread | None = None
        self._reg = execucoes.REGISTRO
        self._job_exec: execucoes.Execucao | None = None
        self._tr_execs: list = []

    # ------------------------------------------------------------ infraestrutura

    def attach_window(self, window) -> None:
        self._window = window

    def drain_events(self) -> list[dict]:
        events = []
        try:
            while True:
                events.append(self._events.get_nowait())
        except queue.Empty:
            pass
        return events

    def is_busy(self) -> dict:
        """Usado ao fechar a janela (nao e chamado pelo JS)."""
        return {"ocr": self._alive(self._job_thread), "transcricao": self._alive(self._tr_thread)}

    def request_stop_all(self) -> None:
        self._cancel.set()
        self._tr_cancel.set()
        self._tr_pause.clear()

    @staticmethod
    def _alive(thread: threading.Thread | None) -> bool:
        return bool(thread and thread.is_alive())

    def _emit(self, **event) -> None:
        self._events.put(event)

    def _dialog(self, kind: str, **kwargs):
        import webview  # importado aqui para o modulo funcionar sem janela

        if self._window is None:
            raise RuntimeError("Janela indisponível.")
        dialog_type = getattr(getattr(webview, "FileDialog", None), kind, None)
        if dialog_type is None:
            dialog_type = getattr(webview, f"{kind}_DIALOG")
        return self._window.create_file_dialog(dialog_type, **kwargs)

    # ------------------------------------------------------------------ inicio

    @_safe
    def bootstrap(self) -> dict:
        return {
            "ok": True,
            "app_name": core.APP_NAME,
            "version": core.APP_VERSION,
            "convert_modes": CONVERT_MODES,
            "batch_modes": BATCH_MODES,
            "config": dict(core.CONFIG),
            "defaults": dict(core.DEFAULT_CONFIG),
            "extensions": {
                "pdf": [".pdf"],
                "direct": sorted(core.SUPPORTED_DIRECT),
                "images": sorted(core.SUPPORTED_IMAGES),
                "media": sorted(transcricao.ENTRADA_EXTS),
            },
            "data_dir": str(core.APP_DATA_DIR),
            "log_file": str(core.SESSION_LOG),
            "log_dir": str(core.LOG_DIR),
            "transcricao": self._transcricao_state(),
            "busy": self.is_busy(),
        }

    # --------------------------------------------------------------- selecao

    @_safe
    def pick_files(self, accepts: str = "pdf") -> dict:
        chosen = self._dialog("OPEN", allow_multiple=True, file_types=_file_filter(accepts))
        return {"ok": True, "paths": [str(p) for p in (chosen or [])]}

    @_safe
    def pick_folder(self, title: str = "") -> dict:
        chosen = self._dialog("FOLDER")
        return {"ok": True, "path": str(chosen[0]) if chosen else ""}

    @_safe
    def open_path(self, path: str) -> dict:
        target = Path(path)
        core.open_folder(target if target.is_dir() else target.parent)
        return {"ok": True}

    @_safe
    def open_data_folder(self) -> dict:
        core.open_folder(core.APP_DATA_DIR)
        return {"ok": True}

    # ---------------------------------------------------------- conversao

    @_safe
    def start_convert(self, params: dict) -> dict:
        if self._alive(self._job_thread):
            return _error("Já existe uma operação em andamento.")
        mode_key = params.get("mode")
        mode = _MODE_BY_KEY.get(mode_key)
        if mode is None:
            return _error("Modo inválido.")
        files = [Path(p) for p in params.get("files", [])]
        if not files:
            return _error("Selecione ao menos um arquivo.")

        accepts = mode["accepts"]
        if accepts == "images":
            bad = [p for p in files if p.suffix.lower() not in core.SUPPORTED_IMAGES]
        elif accepts == "direct":
            bad = [p for p in files if p.suffix.lower() not in core.SUPPORTED_DIRECT]
        else:
            bad = [p for p in files if p.suffix.lower() != ".pdf"]
        if bad:
            return _error("Estes arquivos não são compatíveis com o modo escolhido:\n" + "\n".join(p.name for p in bad))

        page_spec = str(params.get("pages", "")).strip()
        if page_spec and mode["pages"]:
            for path in files:
                try:
                    info = core.get_pdf_info(path)
                except Exception as exc:
                    return _error(f"Não foi possível ler {path.name}: {exc}")
                if info.get("encrypted"):
                    return _error(f"{path.name} está protegido por senha.")
                try:
                    core.parse_page_range(page_spec, int(info["pages"]))
                except ValueError as exc:
                    return _error(f"{path.name}: {exc}")

        invalidate = False
        if mode_key in SEARCHABLE_MODE_KEYS:
            signed = [p.name for p in files if _signed(p)]
            if signed:
                if not params.get("confirm_signed"):
                    return {"ok": True, "status": "needs_confirm", "signed": signed}
                invalidate = True

        same_folder = bool(params.get("same_folder", True))
        custom = Path(params["output_folder"]) if params.get("output_folder") else None
        if not same_folder and not custom:
            return _error("Escolha a pasta de saída ou salve na pasta de origem.")

        self._begin_job()
        ex = self._nova_execucao_job("converter", mode["label"], files, mode_key, "start_convert", params)
        self._linha(ex, f"Iniciando: {mode['label']} ({len(files)} arquivo(s))")
        self._job_thread = threading.Thread(
            target=self._run_convert,
            args=(mode_key, files, same_folder, custom, page_spec, str(params.get("format", "md")),
                  bool(params.get("combine_images", True)), invalidate, ex),
            daemon=True,
        )
        self._job_thread.start()
        return {"ok": True, "status": "started"}

    def _begin_job(self) -> None:
        self._cancel = threading.Event()
        self._job_errors = False

    def _linha(self, ex, text: str) -> None:
        """Linha para o log da tela (evento) e para o log da execucao."""
        self._emit(type="line", text=text)
        if ex is not None:
            ex.log(text)

    def _nova_execucao_job(self, ferramenta, rotulo, files, mode_key, acao, params):
        """Registra a execucao de um trabalho de conversao/lote (fica 'na fila' ate a thread comecar)."""
        cancel = self._cancel
        ex = self._reg.criar(
            ferramenta, rotulo, _resumo_entrada(files), variante=mode_key,
            repetir={"acao": acao, "args": [copy.deepcopy(params)]},
        )
        ex.definir_cancelamento(lambda: self._cancelar_evento(cancel))
        self._job_exec = ex
        return ex

    def _cancelar_evento(self, cancel: threading.Event) -> None:
        cancel.set()
        self._emit(type="line", text="Cancelamento solicitado. Aguardando a etapa atual terminar...")
        self._emit(type="status", text="Cancelando...")

    @staticmethod
    def _concluir_itens(ex, total: int, falhas: int) -> None:
        if total and falhas >= total:
            ex.falhar(f"Todos os {total} arquivo(s) falharam. Veja o log da execução.")
        elif falhas:
            ex.concluir(f"{total - falhas} de {total} arquivo(s) processado(s); {falhas} com falha.", avisos=falhas)
        else:
            ex.concluir(f"{total} arquivo(s) processado(s).")

    def _run_convert(self, mode_key, files, same_folder, custom, page_spec, output_format, combine, invalidate,
                     ex) -> None:
        core.set_progress_queue(self._events)
        falhas = 0
        with self._reg.executar(ex):
            try:
                total = len(files)
                if mode_key == "images":
                    folder = custom if not same_folder else files[0].parent
                    ex.definir_etapa("Reconhecendo texto nas imagens")
                    saidas = core.ocr_images(files, folder, output_format, combine, cancel_event=self._cancel)
                    ex.concluir(f"{len(saidas or [])} arquivo(s) de saída criado(s).")
                else:
                    if total > 1:
                        ex.definir_geral(total, "arquivo")
                    for index, path in enumerate(files, 1):
                        if self._cancel.is_set():
                            raise core.OperationCancelled("Cancelado.")
                        folder = path.parent if same_folder else custom
                        ex.novo_item(index, path.name)
                        ex.definir_etapa(f"Arquivo {index} de {total}: {path.name}", registrar=True)
                        self._emit(type="file_start", index=index, total=total, name=path.name)
                        try:
                            self._convert_one(mode_key, path, folder, page_spec, output_format, invalidate)
                        except core.OperationCancelled:
                            raise
                        except Exception as exc:
                            falhas += 1
                            logging.exception("Falha ao processar %s", path)
                            ex.log(f"FALHA em {path.name}: {exc}", "erro")
                            self._emit(type="file_error", name=path.name, text=str(exc))
                        ex.avancar_geral(index)
                    self._concluir_itens(ex, total, falhas)
                self._emit(type="done")
            except core.OperationCancelled:
                ex.cancelar("Cancelada pelo usuário.")
                self._emit(type="cancelled")
            except Exception as exc:
                logging.exception("Erro na conversão: %s", exc)
                ex.falhar(str(exc) or exc.__class__.__name__)
                self._emit(type="error", text=str(exc))

    def _convert_one(self, mode_key, path, folder, page_spec, output_format, invalidate) -> None:
        cancel = self._cancel
        if mode_key == "direct":
            core.direct_convert(path, folder, cancel_event=cancel)
        elif mode_key in ("auto", "force"):
            core.ocr_pdf_to_text(path, folder, mode_key, page_spec, output_format, cancel_event=cancel)
        elif mode_key == "searchable_skip":
            core.searchable_pdf(path, folder, "skip", page_spec, invalidate, cancel_event=cancel)
        elif mode_key == "searchable_skip_md":
            generated = core.searchable_pdf(path, folder, "skip", page_spec, invalidate, cancel_event=cancel)
            core.direct_convert(generated, folder, cancel_event=cancel)
        elif mode_key == "searchable_force":
            core.searchable_pdf(path, folder, "force", page_spec, invalidate, cancel_event=cancel)
        elif mode_key == "searchable_redo":
            core.searchable_pdf(path, folder, "redo", page_spec, invalidate, cancel_event=cancel)

    @_safe
    def cancel_job(self) -> dict:
        if self._alive(self._job_thread):
            ex = self._job_exec
            if not (ex is not None and ex.pedir_cancelamento()):
                self._cancelar_evento(self._cancel)
        return {"ok": True}

    # ------------------------------------------------------------------ lote

    def _batch_files(self, params: dict) -> tuple[list[Path], Path, Path]:
        input_folder = Path(params.get("input_folder") or "")
        if not params.get("input_folder") or not input_folder.is_dir():
            raise ValueError("Escolha uma pasta de entrada válida.")
        output_folder = Path(params["output_folder"]) if params.get("output_folder") else input_folder / "Extrator_saida"
        mode_key = params.get("mode")
        allowed = core.SUPPORTED_DIRECT if mode_key == "direct" else {".pdf"}
        pattern = "**/*" if params.get("recursive") else "*"
        files = sorted(
            path for path in input_folder.glob(pattern)
            if path.is_file() and path.suffix.lower() in allowed
            and not path.stem.endswith(core.OUTPUT_NAME_SUFFIXES)
        )
        return files, input_folder, output_folder

    @_safe
    def scan_batch(self, params: dict) -> dict:
        with self._reg.tarefa("lote", "Analisar pasta do lote", str(params.get("input_folder") or ""),
                              variante="analise", instantanea=True) as ex:
            ex.definir_etapa("Procurando arquivos compatíveis")
            try:
                files, _input, output = self._batch_files(params)
            except ValueError as exc:
                ex.falhar(str(exc))
                return _error(str(exc))
            signed = []
            if params.get("mode") in SEARCHABLE_BATCH_KEYS:
                ex.definir_etapa("Verificando assinaturas digitais")
                ex.definir_geral(len(files), "arquivo")
                for index, path in enumerate(files, 1):
                    if _signed(path):
                        signed.append(path.name)
                    ex.avancar_geral(index)
            ex.concluir(f"{len(files)} arquivo(s) encontrado(s).")
        return {"ok": True, "count": len(files), "signed": signed, "output_folder": str(output),
                "sample": [p.name for p in files[:5]]}

    @_safe
    def start_batch(self, params: dict) -> dict:
        if self._alive(self._job_thread):
            return _error("Já existe uma operação em andamento.")
        mode_key = params.get("mode")
        if mode_key not in {m["key"] for m in BATCH_MODES}:
            return _error("Modo inválido.")
        try:
            files, _input, output = self._batch_files(params)
        except ValueError as exc:
            return _error(str(exc))
        if not files:
            return _error("Nenhum arquivo compatível encontrado nessa pasta.")
        invalidate = False
        if mode_key in SEARCHABLE_BATCH_KEYS and any(_signed(p) for p in files):
            if not params.get("confirm_signed"):
                return _error("Há PDFs com assinatura digital; confirme para continuar.")
            invalidate = True
        self._begin_job()
        label = next(m["label"] for m in BATCH_MODES if m["key"] == mode_key)
        ex = self._nova_execucao_job("lote", label, files, mode_key, "start_batch", params)
        ex.entrada = f"{params.get('input_folder')} ({len(files)} arquivo(s))"
        self._linha(ex, f"Iniciando lote: {label} ({len(files)} arquivo(s))")
        self._job_thread = threading.Thread(
            target=self._run_batch, args=(mode_key, files, output, invalidate, ex), daemon=True
        )
        self._job_thread.start()
        return {"ok": True, "status": "started", "count": len(files)}

    def _executar_arquivo_lote(self, mode_key, path, output, invalidate, cancel) -> None:
        if mode_key == "direct":
            core.direct_convert(path, output, cancel_event=cancel)
        elif mode_key == "auto_md":
            core.ocr_pdf_to_text(path, output, "auto", "", "md", cancel_event=cancel)
        elif mode_key == "force_md":
            core.ocr_pdf_to_text(path, output, "force", "", "md", cancel_event=cancel)
        elif mode_key == "searchable":
            core.searchable_pdf(path, output, "skip", "", invalidate, cancel_event=cancel)
        elif mode_key == "searchable_md":
            generated = core.searchable_pdf(path, output, "skip", "", invalidate, cancel_event=cancel)
            core.direct_convert(generated, output, cancel_event=cancel)

    def _run_batch(self, mode_key, files, output, invalidate, ex) -> None:
        core.set_progress_queue(self._events)
        cancel = self._cancel
        total = len(files)
        falhas = 0
        with self._reg.executar(ex):
            try:
                if total > 1:
                    ex.definir_geral(total, "arquivo")
                for index, path in enumerate(files, 1):
                    if cancel.is_set():
                        raise core.OperationCancelled("Lote cancelado.")
                    ex.novo_item(index, path.name)
                    ex.definir_etapa(f"Arquivo {index} de {total}: {path.name}", registrar=True)
                    self._emit(type="file_start", index=index, total=total, name=path.name)
                    try:
                        self._executar_arquivo_lote(mode_key, path, output, invalidate, cancel)
                    except core.OperationCancelled:
                        raise
                    except Exception as exc:
                        falhas += 1
                        logging.exception("Falha no lote: %s", path)
                        ex.log(f"FALHA em {path.name}: {exc}", "erro")
                        self._emit(type="file_error", name=path.name, text=str(exc))
                    ex.avancar_geral(index)
                self._concluir_itens(ex, total, falhas)
                self._emit(type="done")
            except core.OperationCancelled:
                ex.cancelar("Lote cancelado pelo usuário.")
                self._emit(type="cancelled")
            except Exception as exc:
                logging.exception("Erro no lote: %s", exc)
                ex.falhar(str(exc) or exc.__class__.__name__)
                self._emit(type="error", text=str(exc))

    # ------------------------------------------------------------ transcricao

    def _transcricao_state(self) -> dict:
        instalados = transcricao.modelos_instalados()
        models = {
            name: {**info, "instalado": name in instalados}
            for name, info in transcricao.MODELOS_DISPONIVEIS.items()
        }
        return {
            "default_model": transcricao.MODELO_PADRAO,
            "installed": instalados,
            "models": models,
            "languages": list(transcricao.IDIOMAS),
            "formats": list(transcricao.FORMATOS_SAIDA),
        }

    @_safe
    def transcricao_state(self) -> dict:
        return {"ok": True, **self._transcricao_state()}

    @_safe
    def transcricao_deps(self) -> dict:
        with self._reg.tarefa("verificacao", "Verificar dependências da transcrição", "ffmpeg, faster-whisper, soundfile",
                              variante="deps", instantanea=True) as ex:
            ex.definir_etapa("Procurando ffmpeg e bibliotecas")
            deps = transcricao.verificar_dependencias()
            faltam = [name for name, ok in deps.items() if not ok]
            ex.concluir("Tudo presente." if not faltam else "Faltam: " + ", ".join(faltam), avisos=len(faltam))
        return {"ok": True, "deps": [{"name": name, "ok": bool(ok)} for name, ok in deps.items()]}

    @_safe
    def start_transcricao(self, params: dict) -> dict:
        if self._alive(self._tr_thread):
            return _error("A fila já está em execução.")
        items = [{"id": str(i["id"]), "path": Path(i["path"])} for i in params.get("items", [])]
        if not items:
            return _error("Adicione ao menos um arquivo pendente na fila.")
        faltando = [name for name, ok in transcricao.verificar_dependencias().items() if not ok]
        if faltando:
            return _error("Dependências ausentes para transcrição: " + ", ".join(faltando))
        options = {
            "modelo": params.get("model") or transcricao.MODELO_PADRAO,
            "idioma": params.get("language") or "pt",
            "formato": params.get("format") or "txt",
            "beams": max(1, min(10, int(params.get("beams") or 5))),
            "word_level": bool(params.get("word_level")),
        }
        self._tr_cancel = threading.Event()
        self._tr_pause = threading.Event()
        execs = []
        for item in items:  # uma execucao por arquivo; todas "na fila" ate chegar a vez
            ex = self._reg.criar(
                "transcricao", f"Transcrever {item['path'].name}", str(item["path"]), variante=options["modelo"],
                repetir={"acao": "start_transcricao", "args": [{
                    "items": [{"id": item["id"], "path": str(item["path"])}], "model": options["modelo"],
                    "language": options["idioma"], "format": options["formato"], "beams": options["beams"],
                    "word_level": options["word_level"]}]},
            )
            item_cancel = threading.Event()
            ex.definir_cancelamento(self._cancelador_item(ex, item_cancel))
            execs.append((ex, item_cancel))
        self._tr_execs = execs
        self._emit(type="line", text=f"Iniciando fila de transcrição: {len(items)} arquivo(s)")
        self._tr_thread = threading.Thread(target=self._run_transcricao, args=(items, options, execs), daemon=True)
        self._tr_thread.start()
        return {"ok": True, "status": "started"}

    @staticmethod
    def _cancelador_item(ex, item_cancel: threading.Event):
        def cancelar() -> None:
            item_cancel.set()
            if ex.estado == "fila":
                ex.cancelar("Removida da fila antes de começar.")

        return cancelar

    def _run_transcricao(self, items: list[dict], options: dict, execs: list) -> None:
        total = len(items)
        cancel, pause = self._tr_cancel, self._tr_pause
        for index, item in enumerate(items, 1):
            if cancel.is_set():
                break
            ex, item_cancel = execs[index - 1]
            if item_cancel.is_set() or not ex.ativo:
                ex.cancelar("Removida da fila antes de começar.")
                self._emit(type="transc_item_cancelled", id=item["id"])
                continue
            path = item["path"]
            self._emit(type="transc_item_start", id=item["id"], index=index, total=total, name=path.name)
            with self._reg.executar(ex):
                ex.definir_etapa(f"Arquivo {index} de {total}: preparando a transcrição", registrar=True)
                try:
                    def log(msg, _ex=ex):
                        self._emit(type="line", text=msg)
                        _ex.log(msg)

                    def progress(info, _id=item["id"], _i=index, _ex=ex):
                        self._emit(type="transc_progress", id=_id, index=_i, total=total, **info)
                        if (info.get("total_s") or 0) > 0:
                            _ex.definir_total(info["total_s"], "segundo", "duracao")
                            _ex.avancar(info.get("processado_s") or 0)

                    result = transcricao.transcrever_com_progresso(
                        arquivo=path, modelo=options["modelo"], idioma=options["idioma"],
                        format_saida=options["formato"], word_level=options["word_level"], device="cpu",
                        beam_size=options["beams"], log=log, progresso_cb=progress,
                        cancel_event=_OuEvento(item_cancel, cancel), pause_event=pause,
                    )
                    self._emit(type="transc_item_done", id=item["id"], output_path=str(result["output_path"]))
                    ex.concluir(f"Salvo em {result['output_path']}")
                except core.OperationCancelled:
                    ex.cancelar("Transcrição cancelada pelo usuário.")
                    self._emit(type="transc_item_cancelled", id=item["id"])
                    if cancel.is_set():
                        break
                except Exception as exc:
                    logging.exception("Falha na transcrição de %s", path)
                    ex.falhar(str(exc) or exc.__class__.__name__)
                    self._emit(type="transc_item_error", id=item["id"], text=str(exc))
        for ex, _evento in execs:  # o que sobrou na fila quando ela foi cancelada
            if ex.ativo:
                ex.cancelar("Fila cancelada antes de chegar a vez deste arquivo.")
        self._emit(type="transc_queue_done")

    @_safe
    def toggle_pause_transcricao(self) -> dict:
        if not self._alive(self._tr_thread):
            return {"ok": True, "paused": False}
        if self._tr_pause.is_set():
            self._tr_pause.clear()
            self._emit(type="line", text="Transcrição retomada.")
            return {"ok": True, "paused": False}
        self._tr_pause.set()
        self._emit(type="line", text="Transcrição pausada.")
        return {"ok": True, "paused": True}

    @_safe
    def cancel_transcricao(self) -> dict:
        if self._alive(self._tr_thread):
            self._tr_cancel.set()
            self._tr_pause.clear()
            self._emit(type="line", text="Cancelamento da fila de transcrição solicitado...")
        return {"ok": True}

    @_safe
    def download_model(self, name: str) -> dict:
        if self._alive(self._tr_thread):
            return _error("Aguarde a fila atual terminar antes de baixar outro modelo.")
        if self._alive(self._download_thread):
            return _error("Já há um modelo sendo baixado.")
        if name not in transcricao.MODELOS_DISPONIVEIS:
            return _error("Modelo desconhecido.")

        info = transcricao.MODELOS_DISPONIVEIS[name]
        ex = self._reg.criar("modelo", f"Baixar modelo {name}", name, variante=name,
                             repetir={"acao": "download_model", "args": [name]})
        ex.definir_dimensao(info.get("tamanho_mb") or 0, "MB")  # so alimenta o ETA historico: sem progresso real

        def worker() -> None:
            with self._reg.executar(ex):
                ex.definir_etapa(f"Baixando o modelo {name} (exige internet; sem progresso detalhado)", registrar=True)
                try:
                    transcricao.baixar_modelo(name, log=lambda m: ex.log(str(m)))
                    ex.concluir("Modelo pronto.")
                    self._emit(type="model_download", name=name, status="done")
                except Exception as exc:
                    logging.exception("Falha ao baixar modelo %s", name)
                    ex.falhar(str(exc) or exc.__class__.__name__)
                    self._emit(type="model_download", name=name, status="error", text=str(exc))

        self._download_thread = threading.Thread(target=worker, daemon=True)
        self._download_thread.start()
        return {"ok": True, "status": "started"}

    # ------------------------------------------------------------ diagnostico

    @_safe
    def diagnose(self, path: str) -> dict:
        alvo = Path(path)
        with self._reg.tarefa("diagnostico", "Diagnosticar arquivo", alvo.name,
                              repetir={"acao": "diagnose", "args": [str(path)]}) as ex:
            ex.definir_etapa("Lendo o arquivo")
            with contextlib.suppress(Exception):
                ex.definir_dimensao(alvo.stat().st_size / 1_000_000, "MB")
            rows = core.diagnose_rows(alvo)
            ex.concluir(f"{len(rows)} item(ns) analisado(s).")
        return {"ok": True, "rows": [{"item": item, "result": str(result)} for item, result in rows]}

    # ------------------------------------------------------------ configuracoes

    @_safe
    def save_settings(self, values: dict) -> dict:
        with self._reg.tarefa("configuracoes", "Salvar configurações", instantanea=True) as ex:
            clean: dict = {}
            language = str(values.get("language", "")).strip()
            if not language:
                ex.falhar("Informe o idioma do OCR (por exemplo, por+eng).")
                return _error("Informe o idioma do OCR (por exemplo, por+eng).")
            clean["language"] = language
            for key, (low, high) in _INT_SETTINGS.items():
                try:
                    number = int(values.get(key))
                except (TypeError, ValueError):
                    ex.falhar(f"Valor inválido em '{key}'.")
                    return _error(f"Valor inválido em '{key}'.")
                clean[key] = max(low, min(high, number))
            if values.get("verbosity") in _VERBOSITY:
                clean["verbosity"] = values["verbosity"]
            if values.get("cpu_mode") in _CPU_MODES:
                clean["cpu_mode"] = values["cpu_mode"]
            if values.get("pdf_output_type") in _PDF_TYPES:
                clean["pdf_output_type"] = values["pdf_output_type"]
            for key in _BOOL_SETTINGS:
                clean[key] = bool(values.get(key))
            ex.definir_etapa("Gravando config.json")
            core.CONFIG.update(clean)
            core.save_config(core.CONFIG)
            ex.concluir(f"{len(clean)} ajuste(s) salvo(s).")
        return {"ok": True, "config": dict(core.CONFIG)}

    @_safe
    def restore_settings(self) -> dict:
        with self._reg.tarefa("configuracoes", "Restaurar configurações padrão", instantanea=True) as ex:
            ex.definir_etapa("Gravando config.json")
            core.CONFIG.clear()
            core.CONFIG.update(core.DEFAULT_CONFIG)
            core.save_config(core.CONFIG)
            ex.concluir("Padrões restaurados.")
        return {"ok": True, "config": dict(core.CONFIG)}

    # ----------------------------------------------------- instalacao / ajuda

    def _install_rows(self) -> list[tuple[str, bool, str]]:
        return core.installation_rows() + transcricao.installation_rows()

    @staticmethod
    def _summarize_install(rows) -> dict:
        pip_specs = {**core.PIP_REQUIREMENTS, **transcricao.PIP_REQUIREMENTS}
        missing_pip = [name for name, ok, _ in rows if not ok and name in pip_specs]
        missing_model = any(name == "Modelo de transcricao" and not ok for name, ok, _ in rows)
        return {
            "ok": True,
            "rows": [{"name": n, "ok": bool(ok), "detail": d} for n, ok, d in rows],
            "missing_pip": missing_pip,
            "missing_model": missing_model,
        }

    @_safe
    def check_installation(self) -> dict:
        """Versao sincrona (usada em testes e por quem nao precisa de progresso)."""
        with self._reg.tarefa("verificacao", "Verificar instalação", "Componentes do programa") as ex:
            ex.definir_etapa("Verificando componentes")
            rows = self._install_rows()
            resumo = self._summarize_install(rows)
            faltam = [n for n, ok, _ in rows if not ok]
            ex.concluir(f"{len(rows) - len(faltam)} de {len(rows)} componente(s) OK.", avisos=len(faltam))
        return resumo

    @staticmethod
    def _iter_install_rows():
        yield from core.iter_installation_rows()
        yield from transcricao.iter_installation_rows()

    @_safe
    def start_check_installation(self) -> dict:
        """Verifica a instalacao em thread, sem travar a interface. Cada
        componente chega como evento `check_row`; o resultado completo, como
        `check_done` (ou `check_error`)."""
        if self._alive(self._check_thread):
            return _error("A verificação já está em andamento.")
        ex = self._reg.criar("verificacao", "Verificar instalação", "Componentes do programa",
                             repetir={"acao": "start_check_installation", "args": []})
        # total conhecido: cada componente verificado avanca a barra de verdade
        ex.definir_geral(core.INSTALLATION_ROW_COUNT + transcricao.INSTALLATION_ROW_COUNT, "componente")
        self._check_thread = threading.Thread(target=self._run_check, args=(ex,), daemon=True)
        self._check_thread.start()
        return {"ok": True, "status": "started"}

    def _run_check(self, ex) -> None:
        self._emit(type="check_start")
        rows: list[tuple[str, bool, str]] = []
        with self._reg.executar(ex):
            ex.definir_etapa("Verificando componentes")
            try:
                for index, (name, ok, detail) in enumerate(self._iter_install_rows(), 1):
                    rows.append((name, ok, detail))
                    ex.avancar_geral(index)
                    ex.definir_etapa(f"Verificado: {name}")
                    ex.log(f"{name}: {'OK' if ok else 'FALTA'} - {detail}", "info" if ok else "aviso")
                    self._emit(type="check_row", index=index, name=name, ok=bool(ok), detail=str(detail))
                faltam = [n for n, ok, _ in rows if not ok]
                ex.concluir(f"{len(rows) - len(faltam)} de {len(rows)} componente(s) OK."
                            + (f" Faltam: {', '.join(faltam)}." if faltam else ""), avisos=len(faltam))
                self._emit(type="check_done", **self._summarize_install(rows))
            except Exception as exc:  # noqa: BLE001
                logging.exception("Falha ao verificar a instalação")
                ex.falhar(str(exc) or exc.__class__.__name__)
                self._emit(type="check_error", text=str(exc) or exc.__class__.__name__)

    def notify_close_blocked(self) -> None:
        """Usado pelo extrator.py: avisa DENTRO da janela (sem caixa)."""
        self._emit(type="close_blocked")

    @_safe
    def install_missing(self) -> dict:
        if self._alive(self._install_thread):
            return _error("A instalação já está em andamento.")
        pip_specs = {**core.PIP_REQUIREMENTS, **transcricao.PIP_REQUIREMENTS}
        rows = self._install_rows()
        missing_pip = [(n, pip_specs[n]) for n, ok, _ in rows if not ok and n in pip_specs]
        missing_model = any(n == "Modelo de transcricao" and not ok for n, ok, _ in rows)
        if not missing_pip and not missing_model:
            return {"ok": True, "status": "nothing"}
        nomes = [n for n, _spec in missing_pip] + (["modelo de transcrição"] if missing_model else [])
        ex = self._reg.criar("instalacao", "Instalar componentes faltantes", ", ".join(nomes))
        ex.definir_geral(len(nomes), "componente")  # total conhecido: um passo por componente
        cancel = threading.Event()
        ex.definir_cancelamento(cancel.set)  # cancela entre componentes (o passo atual termina)
        self._install_thread = threading.Thread(
            target=self._run_install, args=(missing_pip, missing_model, ex, cancel), daemon=True
        )
        self._install_thread.start()
        return {"ok": True, "status": "started"}

    def _run_install(self, missing_pip: list[tuple[str, str]], missing_model: bool, ex, cancel) -> None:
        feitos = falhas = 0
        total = len(missing_pip) + (1 if missing_model else 0)
        with self._reg.executar(ex):
            for name, spec in missing_pip:
                if cancel.is_set():
                    break
                ex.definir_etapa(f"Instalando {name} via pip", registrar=True)
                self._linha(ex, f"Instalando {name} ({spec}) via pip...")
                try:
                    subprocess.run(
                        [sys.executable, "-m", "pip", "install", spec],
                        check=True, capture_output=True, text=True, creationflags=_NO_WINDOW,
                    )
                    self._linha(ex, f"{name} instalado com sucesso.")
                except subprocess.CalledProcessError as exc:
                    falhas += 1
                    self._emit(type="line", text=f"Falha ao instalar {name}: {(exc.stderr or str(exc))[-600:]}")
                    ex.log(f"Falha ao instalar {name}: {(exc.stderr or str(exc))[-600:]}", "erro")
                feitos += 1
                ex.avancar_geral(feitos)
            if missing_model and not cancel.is_set():
                ex.definir_etapa("Baixando o modelo de transcrição (exige internet; sem progresso detalhado)", registrar=True)
                self._linha(ex, f"Baixando modelo de transcrição '{transcricao.MODELO_PADRAO}'...")
                try:
                    transcricao.baixar_modelo(transcricao.MODELO_PADRAO)
                    self._linha(ex, "Modelo pronto.")
                except Exception as exc:
                    falhas += 1
                    self._emit(type="line", text=f"Falha ao baixar o modelo: {exc}")
                    ex.log(f"Falha ao baixar o modelo: {exc}", "erro")
                feitos += 1
                ex.avancar_geral(feitos)
            if cancel.is_set() and feitos < total:
                ex.cancelar(f"Cancelada: {feitos} de {total} componente(s) tratado(s).")
            elif total and falhas >= total:
                ex.falhar("Nenhum componente foi instalado. Veja o log da execução.")
            elif falhas:
                ex.concluir(f"{total - falhas} de {total} componente(s) instalado(s); {falhas} com falha.", avisos=falhas)
            else:
                ex.concluir(f"{total} componente(s) instalado(s).")
        self._emit(type="install_fix_done")

    @_safe
    def clear_data(self) -> dict:
        with self._reg.tarefa("limpeza", "Limpar cache e logs", str(core.APP_DATA_DIR), instantanea=True) as ex:
            ex.definir_etapa("Removendo logs antigos e cache de retomada do OCR")
            logs, jobs = core.cleanup_data(retention_days=0)
            ex.concluir(f"Removidos {logs} log(s) e {jobs} pasta(s) de cache.")
        return {"ok": True, "logs": logs, "jobs": jobs}

    # ----------------------------------------------------- central de execucoes

    @_safe
    def exec_listar(self) -> dict:
        """Foto de todas as execucoes (a interface consulta em intervalos curtos)."""
        return {"ok": True, **self._reg.snapshot()}

    @_safe
    def exec_detalhe(self, id: str, desde: int = 0) -> dict:
        detalhe = self._reg.detalhe(id, desde)
        if detalhe is None:
            return _error("Execução não encontrada (talvez tenha sido limpa).")
        return {"ok": True, **detalhe}

    @_safe
    def exec_cancelar(self, id: str) -> dict:
        if self._reg.cancelar(id):
            return {"ok": True}
        return _error("Esta execução não pode ser cancelada agora.")

    @_safe
    def exec_repetir(self, id: str) -> dict:
        ex = self._reg.obter(id)
        if ex is None or not ex.repetir:
            return _error("Esta execução não pode ser repetida.")
        acao = ex.repetir.get("acao")
        if acao not in _REPETIVEIS:
            return _error("Esta execução não pode ser repetida.")
        args = copy.deepcopy(ex.repetir.get("args") or [])
        if acao == "start_transcricao":  # ids novos: nao mexe na fila exibida na tela de transcricao
            for item in args[0].get("items", []):
                item["id"] = "r" + uuid.uuid4().hex[:8]
        resultado = getattr(self, acao)(*args)
        return {**resultado, "acao": acao}

    @_safe
    def exec_resumo(self, id: str) -> dict:
        ex = self._reg.obter(id)
        if ex is None:
            return _error("Execução não encontrada.")
        return {"ok": True, "texto": ex.resumo_texto()}

    @_safe
    def exec_limpar_concluidas(self) -> dict:
        return {"ok": True, "removidas": self._reg.limpar_concluidas()}

    # ----------------------------------------------------------------- logs

    @_safe
    def log_arquivos(self) -> dict:
        atual = core.SESSION_LOG.name
        return {"ok": True, "atual": atual, "pasta": str(core.LOG_DIR),
                "arquivos": leitorlog.listar_logs(core.LOG_DIR, atual)}

    @_safe
    def log_ler(self, params: dict) -> dict:
        """Le o log por trechos. modo: cauda | desde | antes | intervalo | ao_redor."""
        nome = str(params.get("nome") or core.SESSION_LOG.name)
        try:
            caminho = leitorlog.nome_valido(core.LOG_DIR, nome)
        except (ValueError, FileNotFoundError) as exc:
            return _error(str(exc))
        ao_vivo = nome == core.SESSION_LOG.name
        modo = params.get("modo") or "cauda"
        maximo = int(params.get("max_bytes") or leitorlog.TAMANHO_TRECHO)
        if modo == "desde":
            res = leitorlog.ler_desde(caminho, int(params.get("offset") or 0), maximo,
                                      str(params.get("nv_ant") or "INFO"), ao_vivo)
        elif modo == "antes":
            res = leitorlog.ler_antes(caminho, int(params.get("offset") or 0), maximo)
        elif modo == "intervalo":
            fim = params.get("fim")
            res = leitorlog.ler_intervalo(caminho, int(params.get("ini") or 0), None if fim is None else int(fim))
        elif modo == "ao_redor":
            res = leitorlog.ler_ao_redor(caminho, int(params.get("offset") or 0))
        else:
            res = leitorlog.ler_cauda(caminho, maximo, ao_vivo)
        return {**res, "ao_vivo": ao_vivo}

    @_safe
    def log_buscar(self, params: dict) -> dict:
        nome = str(params.get("nome") or core.SESSION_LOG.name)
        try:
            caminho = leitorlog.nome_valido(core.LOG_DIR, nome)
        except (ValueError, FileNotFoundError) as exc:
            return _error(str(exc))
        res = leitorlog.buscar(caminho, str(params.get("nivel") or "tudo"), str(params.get("texto") or ""),
                               int(params.get("limite") or 2000))
        return {**res, "ao_vivo": nome == core.SESSION_LOG.name}

    @_safe
    def log_registrar(self, nivel: str, texto: str) -> dict:
        """Grava no log da sessao um aviso/erro mostrado na tela e devolve onde ele ficou ("Ver no log")."""
        nome, antes = core._posicao_log()
        logging.log(_NIVEIS_LOG.get(str(nivel).upper(), logging.INFO), "Interface: %s", str(texto)[:2000])
        return {"ok": True, "nome": nome, "offset": antes}

    @_safe
    def log_abrir_pasta(self) -> dict:
        core.open_folder(core.LOG_DIR)
        return {"ok": True}
