#!/usr/bin/env python3
"""
Extrator de Texto
Aplicativo local para conversao de documentos e OCR no Windows.

Modos principais:
- Conversao direta para Markdown com MarkItDown
- OCR automatico ou forcado de PDF para Markdown/TXT
- Criacao de PDF pesquisavel com OCRmyPDF
- OCR de imagens
- Processamento em lote
- Diagnostico e configuracoes
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import logging
import multiprocessing
import os
import re
import shlex
import shutil
import subprocess
import sys
import threading
import time
import traceback
from concurrent.futures import ProcessPoolExecutor, as_completed
from datetime import datetime
from pathlib import Path
from typing import Iterable, Sequence

import execucoes
import mensagens

try:
    from rich import box
    from rich.align import Align
    from rich.console import Console
    from rich.panel import Panel
    from rich.progress import (
        BarColumn,
        MofNCompleteColumn,
        Progress,
        SpinnerColumn,
        TaskProgressColumn,
        TextColumn,
        TimeElapsedColumn,
        TimeRemainingColumn,
    )
    from rich.prompt import Confirm, IntPrompt, Prompt
    from rich.table import Table
    from rich.text import Text
except ImportError:
    print("Dependências ausentes. Use o atalho 'Reparar instalação' do Menu Iniciar.")
    raise SystemExit(2)

APP_NAME = "Extrator de Texto"
def _read_version() -> str:
    """A versao vem do VERSION.txt (linha 'Versao X.Y.Z'), fonte unica usada
    tambem pelo instalador e pelo CI."""
    try:
        text = (Path(__file__).resolve().parent.parent / "VERSION.txt").read_text(encoding="utf-8")
        for line in text.splitlines():
            if line.strip().lower().startswith("versao "):
                return line.split(None, 1)[1].strip()
    except OSError:
        pass
    return "desconhecida"


APP_VERSION = _read_version()
SUPPORTED_DIRECT = {
    ".pdf", ".docx", ".pptx", ".xlsx", ".html", ".htm", ".txt", ".csv", ".json", ".xml"
}
SUPPORTED_IMAGES = {".png", ".jpg", ".jpeg", ".tif", ".tiff", ".bmp", ".webp"}
OUTPUT_NAME_SUFFIXES = ("_OCR", "_PESQUISAVEL", "_OCR_FORCADO", "_OCR_REFEITO")
DATA_RETENTION_DAYS = 7

# Mapeia nomes de linha de installation_rows() para o especificador pip
# correspondente. Usado pela GUI para instalar automaticamente o que
# estiver faltando (Python, Tesseract e Ghostscript ficam de fora: nao sao
# pacotes pip, sao o interprete e binarios de sistema).
PIP_REQUIREMENTS = {
    "MarkItDown": "markitdown[pdf,docx,xlsx,pptx]==0.1.6",
    "PyMuPDF": "PyMuPDF>=1.26,<2",
    "pytesseract": "pytesseract>=0.3.13,<1",
    "Pillow": "Pillow>=11,<13",
    "OCRmyPDF": "ocrmypdf>=17.8,<18",
    "Rich": "rich>=13.9,<15",
}

console = Console(highlight=False)

# Tudo roda dentro da propria pasta do programa: nada e gravado no perfil do
# usuario ou em outro local externo. Isso torna a pasta autocontida e portavel.
APP_ROOT = Path(__file__).resolve().parent.parent
APP_DATA_DIR = APP_ROOT / "dados"
CONFIG_PATH = APP_DATA_DIR / "config.json"
LOG_DIR = APP_DATA_DIR / "logs"
WORK_DIR = APP_DATA_DIR / "work"
TESSDATA_DIR = APP_DATA_DIR / "tessdata"

DEFAULT_CONFIG = {
    "language": "por+eng",
    "dpi": 220,
    "psm": 3,
    "native_text_threshold": 80,
    "verbosity": "detalhado",
    "cpu_mode": "normal",
    "rotate_pages": True,
    "deskew": True,
    "pdf_output_type": "pdf",
    "optimize": 1,
    "resume": True,
    "auto_open_output": False,
    "output_location": "same",
    "rasterizer": "pypdfium",
}

VERBOSITY_LEVELS = {"normal": 0, "detalhado": 1, "depuracao": 2}


def ensure_dirs() -> None:
    for directory in (APP_DATA_DIR, LOG_DIR, WORK_DIR, TESSDATA_DIR):
        directory.mkdir(parents=True, exist_ok=True)


def load_config() -> dict:
    ensure_dirs()
    data = DEFAULT_CONFIG.copy()
    if CONFIG_PATH.exists():
        try:
            loaded = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                data.update(loaded)
        except Exception:
            pass
    return data


def save_config(config: dict) -> None:
    ensure_dirs()
    CONFIG_PATH.write_text(json.dumps(config, ensure_ascii=False, indent=2), encoding="utf-8")


def setup_logging() -> Path:
    ensure_dirs()
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_path = LOG_DIR / f"extrator_{stamp}.log"
    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s | %(levelname)s | %(message)s",
        handlers=[logging.FileHandler(log_path, encoding="utf-8")],
        force=True,
    )
    logging.info("Início %s v%s", APP_NAME, APP_VERSION)
    return log_path


def cleanup_data(retention_days: int = DATA_RETENTION_DAYS) -> tuple[int, int]:
    """Remove logs e cache de retomada mais antigos que retention_days.

    Roda automaticamente no inicio (best-effort) para que dados/work e
    dados/logs nao cresçam indefinidamente com o conteudo integral de
    documentos processados.
    """
    removed_logs = 0
    removed_jobs = 0
    # 0 dias significa "apagar tudo": sem comparar com o relogio, que no Windows
    # tem resolucao menor que a do sistema de arquivos e poupava arquivos recem-escritos.
    cutoff = float("inf") if retention_days <= 0 else time.time() - retention_days * 86400
    if LOG_DIR.exists():
        for log_file in LOG_DIR.glob("*.log"):
            try:
                if log_file.stat().st_mtime < cutoff:
                    log_file.unlink()
                    removed_logs += 1
            except Exception:
                pass
    if WORK_DIR.exists():
        for job_dir in WORK_DIR.iterdir():
            try:
                if job_dir.is_dir() and job_dir.stat().st_mtime < cutoff:
                    shutil.rmtree(job_dir)
                    removed_jobs += 1
            except Exception:
                pass
    return removed_logs, removed_jobs


CONFIG = load_config()
SESSION_LOG = setup_logging()


def _posicao_log() -> tuple[str, int | None]:
    """(nome do log da sessao, tamanho atual em bytes): marca onde cada execucao comeca e termina."""
    for handler in logging.getLogger().handlers:
        with contextlib.suppress(Exception):
            handler.flush()
    try:
        return SESSION_LOG.name, SESSION_LOG.stat().st_size
    except OSError:
        return SESSION_LOG.name, None


# Registro central de execucoes (tela "Execucoes"); o historico fica em dados/.
# Os processos-filho do OCR (multiprocessing) reimportam este modulo e nao precisam dele.
if multiprocessing.parent_process() is None:
    execucoes.REGISTRO.configurar(APP_DATA_DIR, _posicao_log)
with contextlib.suppress(Exception):
    cleanup_data()


class OperationCancelled(Exception):
    """Levantada quando cancel_event e sinalizado durante uma operacao longa.

    Usada pela interface grafica para interromper um trabalho de forma
    cooperativa entre paginas/arquivos. O terminal nao usa isto: continua
    cancelando via Ctrl+C (KeyboardInterrupt), sem passar cancel_event.
    """


_progress_state = threading.local()


def set_progress_queue(target) -> None:
    """Registra, para a thread atual, um destino (com metodo .put) que recebe
    eventos de progresso estruturados. So a interface grafica usa isto; o
    terminal nunca chama esta funcao, entao seu comportamento nao muda.
    """
    _progress_state.target = target


def emit_progress(**event) -> None:
    target = getattr(_progress_state, "target", None)
    if target is not None:
        with contextlib.suppress(Exception):
            target.put(event)
    with contextlib.suppress(Exception):
        execucoes.evento_progresso(event)


def _progress_context():
    """Captura o destino de progresso e a execucao da thread atual e devolve um
    `emit(**event)` utilizavel em threads auxiliares (por exemplo, a que le a
    saida de um subprocesso), onde os valores thread-local nao existem."""
    target = getattr(_progress_state, "target", None)
    ex = execucoes.atual()

    def emit(**event) -> None:
        if target is not None:
            with contextlib.suppress(Exception):
                target.put(event)
        if ex is not None:
            with contextlib.suppress(Exception):
                execucoes.evento_progresso(event, ex)

    return emit


def verbosity_at_least(level: str) -> bool:
    current = VERBOSITY_LEVELS.get(str(CONFIG.get("verbosity", "detalhado")), 1)
    return current >= VERBOSITY_LEVELS[level]


def log_line(message: str, *, style: str | None = None, level: str = "normal") -> None:
    logging.info(message)
    if verbosity_at_least(level):
        timestamp = datetime.now().strftime("%H:%M:%S")
        console.print(f"[dim][{timestamp}][/dim] {message}", style=style)


def log_debug(message: str) -> None:
    logging.debug(message)
    if verbosity_at_least("depuracao"):
        timestamp = datetime.now().strftime("%H:%M:%S")
        console.print(f"[dim][{timestamp}] DEBUG[/dim] {message}", style="dim")


def show_header() -> None:
    title = Text(APP_NAME, style="bold cyan")
    subtitle = Text(f"v{APP_VERSION} | OCR local | terminal visível", style="dim")
    console.print(Panel(Align.center(Text.assemble(title, "\n", subtitle)), box=box.ROUNDED))


def clear_screen() -> None:
    os.system("cls" if os.name == "nt" else "clear")


def pause() -> None:
    console.input("\n[dim]Pressione ENTER para continuar...[/dim]")


def human_size(value: int) -> str:
    size = float(value)
    units = ["B", "KB", "MB", "GB", "TB"]
    for unit in units:
        if size < 1024 or unit == units[-1]:
            return f"{size:.1f} {unit}"
        size /= 1024
    return f"{value} B"


def human_duration(seconds: float) -> str:
    seconds = int(max(0, seconds))
    hours, rem = divmod(seconds, 3600)
    minutes, sec = divmod(rem, 60)
    if hours:
        return f"{hours:02d}:{minutes:02d}:{sec:02d}"
    return f"{minutes:02d}:{sec:02d}"


def unique_path(path: Path) -> Path:
    if not path.exists():
        return path
    counter = 1
    while True:
        candidate = path.with_name(f"{path.stem} ({counter}){path.suffix}")
        if not candidate.exists():
            return candidate
        counter += 1


def command_text(cmd: Sequence[str]) -> str:
    if os.name == "nt":
        return subprocess.list2cmdline([str(item) for item in cmd])
    return shlex.join([str(item) for item in cmd])


NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0


def open_folder(path: Path) -> None:
    try:
        target = path if path.is_dir() else path.parent
        if os.name == "nt":
            os.startfile(str(target))  # type: ignore[attr-defined]
        elif sys.platform == "darwin":
            subprocess.Popen(["open", str(target)])
        else:
            subprocess.Popen(["xdg-open", str(target)])
    except Exception as exc:
        log_debug(f"Não foi possível abrir a pasta: {exc}")


def find_tesseract() -> Path | None:
    candidates = []
    found = shutil.which("tesseract")
    if found:
        candidates.append(Path(found))
    program_files = [os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)"), os.environ.get("LOCALAPPDATA")]
    for base in filter(None, program_files):
        base_path = Path(str(base))
        candidates.extend(
            [
                base_path / "Tesseract-OCR" / "tesseract.exe",
                base_path / "Programs" / "Tesseract-OCR" / "tesseract.exe",
            ]
        )
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    return None


def find_ghostscript() -> Path | None:
    for name in ("gswin64c.exe", "gswin32c.exe", "gs"):
        found = shutil.which(name)
        if found:
            return Path(found)
    return None


def list_tesseract_languages(executable: Path) -> list[str]:
    """Lista os idiomas instalados chamando 'tesseract --list-langs' direto.

    Nao usa pytesseract.get_languages(): aquela funcao decodifica a saida do
    Tesseract como UTF-8 estrito, e o binario do Tesseract no Windows escreve
    no console usando a pagina de codigo ativa (nao UTF-8). Se o caminho do
    TESSDATA_PREFIX contiver qualquer caractere acentuado (por exemplo, a
    propria pasta do programa tiver acento no nome), a decodificacao
    lanca UnicodeDecodeError e a lista de idiomas fica vazia silenciosamente.
    """
    try:
        result = subprocess.run(
            [str(executable), "--list-langs"],
            capture_output=True,
            timeout=15,
            creationflags=NO_WINDOW,
        )
        text = result.stdout.decode("utf-8", errors="replace")
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        languages = [line for line in lines if line and not line.lower().startswith("list of")]
        return sorted(languages)
    except Exception as exc:
        log_debug(f"Falha ao consultar idiomas do Tesseract: {exc}")
        return []


def configure_tesseract() -> tuple[Path | None, list[str]]:
    ensure_dirs()
    if any(TESSDATA_DIR.glob("*.traineddata")):
        os.environ["TESSDATA_PREFIX"] = str(TESSDATA_DIR)
    executable = find_tesseract()
    if not executable:
        return None, []
    # OCRmyPDF chama "tesseract" como processo externo herdando o PATH atual;
    # sem isto, o OCR de PDF pesquisavel pode falhar mesmo com o Tesseract
    # localizado aqui apenas para uso via pytesseract.
    tess_dir = str(executable.parent)
    current_path = os.environ.get("PATH", "")
    if tess_dir.lower() not in current_path.lower():
        os.environ["PATH"] = tess_dir + os.pathsep + current_path
    try:
        import pytesseract

        pytesseract.pytesseract.tesseract_cmd = str(executable)
    except Exception as exc:
        log_debug(f"Falha ao configurar pytesseract: {exc}")
    return executable, list_tesseract_languages(executable)


def required_languages(language_expression: str) -> set[str]:
    return {part.strip() for part in language_expression.split("+") if part.strip()}


def ensure_ocr_ready() -> tuple[Path, list[str]]:
    executable, languages = configure_tesseract()
    if executable is None:
        raise RuntimeError(
            "Tesseract OCR não foi encontrado. Use o atalho 'Reparar instalação' do Menu Iniciar ou instale o Tesseract antes de usar OCR."
        )
    missing = required_languages(str(CONFIG["language"])) - set(languages)
    if missing:
        raise RuntimeError(
            "Pacotes de idioma ausentes no Tesseract: " + ", ".join(sorted(missing)) + ". Use o atalho 'Reparar instalação' do Menu Iniciar."
        )
    return executable, languages


def get_jobs() -> int:
    count = os.cpu_count() or 2
    mode = CONFIG.get("cpu_mode", "normal")
    if mode == "economico":
        return max(1, count // 2)
    if mode == "maximo":
        return max(1, count)
    return max(1, count - 1)


def normalize_text(text: str) -> str:
    text = text.replace("\x00", "")
    text = re.sub(r"[ \t]+\n", "\n", text)
    text = re.sub(r"\n{4,}", "\n\n\n", text)
    return text.strip()


def parse_page_range(spec: str, total_pages: int) -> list[int]:
    spec = spec.strip().lower()
    if not spec or spec in {"todas", "todos", "all", "*"}:
        return list(range(total_pages))
    result: set[int] = set()
    for token in re.split(r"[,;\s]+", spec):
        if not token:
            continue
        token = token.replace("end", str(total_pages)).replace("fim", str(total_pages))
        if "-" in token:
            start_text, end_text = token.split("-", 1)
            try:
                start = int(start_text)
                end = int(end_text)
            except ValueError:
                raise ValueError(f"Intervalo de páginas inválido: '{token}'")
            if start > end:
                start, end = end, start
            for page in range(start, end + 1):
                if 1 <= page <= total_pages:
                    result.add(page - 1)
        else:
            try:
                page = int(token)
            except ValueError:
                raise ValueError(f"Número de página inválido: '{token}'")
            if 1 <= page <= total_pages:
                result.add(page - 1)
    if not result:
        raise ValueError("O intervalo informado não contém páginas válidas.")
    return sorted(result)


def page_spec_for_ocrmypdf(spec: str) -> str | None:
    cleaned = spec.strip()
    if not cleaned or cleaned.lower() in {"todas", "todos", "all", "*"}:
        return None
    return cleaned.replace("fim", "end")


def choose_page_range(total_pages: int) -> str:
    console.print(f"Total de páginas: [bold]{total_pages}[/bold]")
    while True:
        spec = Prompt.ask(
            "Páginas ([dim]ENTER = todas; exemplo: 1-50,75,90-120[/dim])",
            default="",
            show_default=False,
        )
        try:
            parse_page_range(spec, total_pages)
            return spec
        except ValueError as exc:
            console.print(f"[red]{exc}[/red]")


def choose_output_format() -> str:
    console.print("[1] Markdown (.md)\n[2] Texto (.txt)\n[3] Ambos")
    choice = Prompt.ask("Formato de saída", choices=["1", "2", "3"], default="1")
    return {"1": "md", "2": "txt", "3": "both"}[choice]


def choose_output_folder(inputs: Sequence[Path]) -> Path:
    if CONFIG.get("output_location") == "same":
        return inputs[0].parent
    selected = select_folder("Escolha a pasta de saída")
    return selected or inputs[0].parent


def select_files(title: str, filetypes: list[tuple[str, str]], multiple: bool = True) -> list[Path]:
    try:
        import tkinter as tk
        from tkinter import filedialog

        root = tk.Tk()
        root.withdraw()
        root.attributes("-topmost", True)
        if multiple:
            selected = filedialog.askopenfilenames(title=title, filetypes=filetypes)
            root.destroy()
            return [Path(item) for item in selected]
        selected_one = filedialog.askopenfilename(title=title, filetypes=filetypes)
        root.destroy()
        return [Path(selected_one)] if selected_one else []
    except Exception as exc:
        log_debug(f"Seletor gráfico indisponível: {exc}")
        raw = console.input("Cole o caminho completo do arquivo: ").strip().strip('"')
        return [Path(raw)] if raw else []


def select_folder(title: str) -> Path | None:
    try:
        import tkinter as tk
        from tkinter import filedialog

        root = tk.Tk()
        root.withdraw()
        root.attributes("-topmost", True)
        selected = filedialog.askdirectory(title=title)
        root.destroy()
        return Path(selected) if selected else None
    except Exception as exc:
        log_debug(f"Seletor de pasta indisponível: {exc}")
        raw = console.input("Cole o caminho completo da pasta: ").strip().strip('"')
        return Path(raw) if raw else None


def get_pdf_info(path: Path, detailed: bool = False) -> dict:
    import fitz

    doc = fitz.open(path)
    try:
        if doc.needs_pass:
            return {
                "pages": doc.page_count,
                "encrypted": True,
                "signed": False,
                "native_pages": 0,
                "scan_pages": 0,
                "image_pages": 0,
                "classification": "PDF protegido por senha",
            }
        # get_sigflags() retorna -1 quando NAO ha campos de assinatura, e 1/3
        # quando ha ao menos um. bool(-1) e True, entao nao usar bool() direto.
        sigflags = doc.get_sigflags() if hasattr(doc, "get_sigflags") else -1
        signed = isinstance(sigflags, int) and sigflags > 0
        native_pages = 0
        image_pages = 0
        low_text_pages = 0
        threshold = int(CONFIG["native_text_threshold"])
        page_iter: Iterable[int]
        if detailed or doc.page_count <= 300:
            page_iter = range(doc.page_count)
        else:
            sample = sorted(set([0, doc.page_count - 1] + [int(i * (doc.page_count - 1) / 49) for i in range(50)]))
            page_iter = sample
        sampled = list(page_iter)
        for index in sampled:
            page = doc.load_page(index)
            text_length = len(normalize_text(page.get_text("text")))
            if text_length >= threshold:
                native_pages += 1
            else:
                low_text_pages += 1
            if page.get_images(full=True):
                image_pages += 1
        ratio = native_pages / max(1, len(sampled))
        if ratio >= 0.9:
            classification = "PDF digital / texto pesquisável"
        elif ratio <= 0.1:
            classification = "PDF digitalizado / OCR necessário"
        else:
            classification = "PDF híbrido / OCR automático recomendado"
        return {
            "pages": doc.page_count,
            "encrypted": False,
            "signed": signed,
            "native_pages": native_pages,
            "scan_pages": low_text_pages,
            "image_pages": image_pages,
            "sampled_pages": len(sampled),
            "classification": classification,
        }
    finally:
        doc.close()


def work_signature(path: Path, mode: str, pages: Sequence[int], output_format: str) -> str:
    stat = path.stat()
    payload = {
        "path": str(path.resolve()),
        "size": stat.st_size,
        "mtime": stat.st_mtime_ns,
        "mode": mode,
        "pages": list(pages),
        "format": output_format,
        "language": CONFIG["language"],
        "dpi": CONFIG["dpi"],
        "psm": CONFIG["psm"],
        "threshold": CONFIG["native_text_threshold"],
    }
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode("utf-8")).hexdigest()[:20]


def render_page_to_pil(page, dpi: int):
    from PIL import Image
    import fitz

    pix = page.get_pixmap(dpi=dpi, colorspace=fitz.csRGB, alpha=False)
    return Image.frombytes("RGB", (pix.width, pix.height), pix.samples)


def ocr_image_text(image, language: str, psm: int) -> str:
    import pytesseract

    config = f"--oem 3 --psm {psm} -c preserve_interword_spaces=1"
    return normalize_text(pytesseract.image_to_string(image, lang=language, config=config))


def write_joined_output(
    base_name: str,
    source_label: str,
    chunks: Sequence[tuple[int, str, str]],
    output_folder: Path,
    output_format: str,
    mode: str,
) -> list[Path]:
    output_folder.mkdir(parents=True, exist_ok=True)
    outputs: list[Path] = []
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    ocr_pages = {page for page, source, _ in chunks if source != "texto nativo"}
    if output_format in {"md", "both"}:
        md_path = unique_path(output_folder / f"{base_name}_OCR.md")
        with md_path.open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(f"# {base_name}\n\n")
            handle.write(f"- Arquivo de origem: `{source_label}`\n")
            handle.write(f"- Modo: {mode}\n")
            handle.write(f"- Idioma OCR: {CONFIG['language']}\n")
            handle.write(f"- Gerado em: {timestamp}\n")
            if ocr_pages:
                handle.write(f"- Páginas processadas por OCR: {', '.join(str(p) for p in sorted(ocr_pages))}\n")
            handle.write("\n")
            for page_number, source, text in chunks:
                handle.write(f"---\n\n## Página {page_number} ({source})\n\n")
                handle.write(f"<!-- origem: {source} -->\n\n")
                handle.write(text or "[Página sem texto reconhecido]")
                handle.write("\n\n")
        outputs.append(md_path)
    if output_format in {"txt", "both"}:
        txt_path = unique_path(output_folder / f"{base_name}_OCR.txt")
        with txt_path.open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(f"{base_name}\nArquivo de origem: {source_label}\nModo: {mode}\nGerado em: {timestamp}\n")
            for page_number, source, text in chunks:
                handle.write(f"\n\n{'=' * 72}\nPAGINA {page_number} | {source}\n{'=' * 72}\n\n")
                handle.write(text or "[Página sem texto reconhecido]")
        outputs.append(txt_path)
    return outputs


SCANNED_IMAGE_COVERAGE_THRESHOLD = 0.5


def _scanned_image_coverage(page) -> float:
    """Fracao da area da pagina ocupada por imagens incorporadas.

    Uma pagina digitalizada (PDF gerado por scanner) normalmente e uma unica
    imagem cobrindo a pagina inteira. Um PDF nativo raramente tem uma imagem
    tao grande. Isso serve para distinguir os dois casos quando a pagina
    digitalizada carrega, alem da imagem, um carimbo de texto nativo (por
    exemplo "Documento assinado digitalmente...", numeracao de protocolo),
    que sozinho poderia passar do limiar de caracteres e enganar a deteccao.
    """
    page_area = abs(page.rect.width * page.rect.height)
    if page_area <= 0:
        return 0.0
    covered = 0.0
    for image_info in page.get_images(full=True):
        xref = image_info[0]
        try:
            rects = page.get_image_rects(xref)
        except Exception:
            rects = []
        for rect in rects:
            covered += abs(rect.width * rect.height)
    return min(1.0, covered / page_area)


def _process_single_page(
    args: tuple[str, int, str, int, int, str, int],
) -> tuple[int, str, str]:
    """Executa em processo separado: precisa ser funcao de topo de modulo
    para ser 'picklable' no multiprocessing do Windows (modo spawn)."""
    input_path_str, page_index, mode, threshold, dpi, language, psm = args
    import fitz

    doc = fitz.open(input_path_str)
    try:
        page = doc.load_page(page_index)
        page_number = page_index + 1
        native_text = normalize_text(page.get_text("text"))
        use_native = mode == "auto" and len(native_text) >= threshold
        if use_native and _scanned_image_coverage(page) >= SCANNED_IMAGE_COVERAGE_THRESHOLD:
            use_native = False
        if use_native:
            return page_number, "texto nativo", native_text
        image = render_page_to_pil(page, dpi)
        try:
            text = ocr_image_text(image, language, psm)
        finally:
            image.close()
        return page_number, "OCR", text
    finally:
        doc.close()


def ocr_pdf_to_text(
    input_path: Path,
    output_folder: Path,
    mode: str,
    page_spec: str,
    output_format: str,
    cancel_event: threading.Event | None = None,
) -> list[Path]:
    import fitz

    ensure_ocr_ready()
    started = time.monotonic()
    doc = fitz.open(input_path)
    try:
        if doc.needs_pass:
            raise RuntimeError("PDF protegido por senha. Remova a senha antes de executar o OCR.")
        pages = parse_page_range(page_spec, doc.page_count)
        signature = work_signature(input_path, mode, pages, output_format)
        job_dir = WORK_DIR / signature
        job_dir.mkdir(parents=True, exist_ok=True)
        manifest_path = job_dir / "manifest.json"
        manifest_path.write_text(
            json.dumps(
                {
                    "input": str(input_path),
                    "mode": mode,
                    "pages": [page + 1 for page in pages],
                    "created": datetime.now().isoformat(),
                },
                ensure_ascii=False,
                indent=2,
            ),
            encoding="utf-8",
        )
        threshold = int(CONFIG["native_text_threshold"])
        dpi = int(CONFIG["dpi"])
        psm = int(CONFIG["psm"])
        language = str(CONFIG["language"])
        chunks: list[tuple[int, str, str]] = []
        resumed = 0
        ocr_count = 0
        native_count = 0
        blank_count = 0
        log_line(f"Arquivo: {input_path}")
        log_line(f"Modo: {mode} | Páginas: {len(pages)} | DPI: {dpi} | Idioma: {language}")
        emit_progress(type="start", file=input_path.name, total=len(pages))
        progress = Progress(
            SpinnerColumn(),
            TextColumn("[bold blue]{task.description}"),
            BarColumn(bar_width=None),
            TaskProgressColumn(),
            MofNCompleteColumn(),
            TimeElapsedColumn(),
            TimeRemainingColumn(),
            console=console,
            transient=False,
        )
        pending: list[int] = []
        with progress:
            task = progress.add_task("Processando páginas", total=len(pages))
            for page_index in pages:
                page_number = page_index + 1
                chunk_path = job_dir / f"page_{page_number:06d}.json"
                if CONFIG.get("resume", True) and chunk_path.exists():
                    try:
                        item = json.loads(chunk_path.read_text(encoding="utf-8"))
                        chunks.append((page_number, item["source"], item["text"]))
                        resumed += 1
                        progress.console.log(f"Página {page_number}: retomada do cache")
                        progress.advance(task)
                        continue
                    except Exception:
                        with contextlib.suppress(Exception):
                            chunk_path.unlink()
                pending.append(page_index)

            if resumed:
                execucoes.avancar(resumed, base=True)
            if pending:
                worker_count = max(1, min(get_jobs(), len(pending)))
                executor = ProcessPoolExecutor(max_workers=worker_count)
                cancelled = False
                try:
                    futures = {
                        executor.submit(
                            _process_single_page,
                            (str(input_path), page_index, mode, threshold, dpi, language, psm),
                        ): page_index
                        for page_index in pending
                    }
                    for future in as_completed(futures):
                        if cancel_event is not None and cancel_event.is_set():
                            cancelled = True
                            executor.shutdown(wait=False, cancel_futures=True)
                            break
                        page_index = futures[future]
                        page_number = page_index + 1
                        page_number, source, text = future.result()
                        if source == "texto nativo":
                            native_count += 1
                            if verbosity_at_least("detalhado"):
                                progress.console.log(f"Página {page_number}: texto nativo ({len(text)} caracteres)")
                        else:
                            ocr_count += 1
                            if not text:
                                blank_count += 1
                            if verbosity_at_least("detalhado"):
                                progress.console.log(f"Página {page_number}: OCR concluído ({len(text)} caracteres)")
                        chunk_path = job_dir / f"page_{page_number:06d}.json"
                        chunk_path.write_text(
                            json.dumps({"page": page_number, "source": source, "text": text}, ensure_ascii=False),
                            encoding="utf-8",
                        )
                        chunks.append((page_number, source, text))
                        progress.advance(task)
                        emit_progress(
                            type="page", current=len(chunks), total=len(pages),
                            page=page_number, source=source,
                        )
                finally:
                    executor.shutdown(wait=True)
                if cancelled:
                    raise OperationCancelled("OCR cancelado pelo usuário.")
        chunks.sort(key=lambda item: item[0])
        outputs = write_joined_output(input_path.stem, input_path.name, chunks, output_folder, output_format, mode)
        with contextlib.suppress(Exception):
            shutil.rmtree(job_dir)
        elapsed = time.monotonic() - started
        summary = Table(title="Resumo do OCR", box=box.SIMPLE_HEAVY)
        summary.add_column("Item")
        summary.add_column("Resultado", justify="right")
        summary.add_row("Páginas processadas", str(len(pages)))
        summary.add_row("Texto nativo reutilizado", str(native_count))
        summary.add_row("Páginas submetidas ao OCR", str(ocr_count))
        summary.add_row("Páginas retomadas", str(resumed))
        summary.add_row("Páginas sem texto", str(blank_count))
        summary.add_row("Tempo total", human_duration(elapsed))
        console.print(summary)
        for output in outputs:
            log_line(f"Saída criada: {output} ({human_size(output.stat().st_size)})", style="green")
        return outputs
    finally:
        doc.close()


def _command_panel(cmd: Sequence[str]) -> Panel:
    # O nome do comando pode conter colchetes (comum em numeros de processo,
    # ex.: "Processo [123-456].pdf"). Construir via Text em vez de f-string
    # evita que o parser de markup do Rich interprete "[...]" como uma tag e
    # lance MarkupError ou corrompa a exibicao.
    rendered = command_text(cmd)
    content = Text("COMANDO EM EXECUÇÃO\n", style="bold")
    content.append(rendered)
    return Panel.fit(content, border_style="cyan")


def _start_subprocess_with_reader(cmd: Sequence[str], cwd: Path | None, on_line) -> tuple[subprocess.Popen, threading.Thread]:
    process = subprocess.Popen(
        [str(item) for item in cmd],
        cwd=str(cwd) if cwd else None,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        creationflags=NO_WINDOW,
    )

    def _drain() -> None:
        if not process.stdout:
            return
        for line in process.stdout:
            stripped = line.rstrip()
            stripped = mensagens.traduzir_linha(stripped)
            logging.info("SAÍDA: %s", stripped)
            if on_line:
                with contextlib.suppress(Exception):
                    on_line(stripped)

    reader = threading.Thread(target=_drain, daemon=True)
    reader.start()
    return process, reader


def _terminate_process(process: subprocess.Popen) -> None:
    process.terminate()
    with contextlib.suppress(subprocess.TimeoutExpired):
        process.wait(timeout=5)
    if process.poll() is None:
        process.kill()


def run_process_with_spinner(
    cmd: Sequence[str],
    description: str,
    cwd: Path | None = None,
    cancel_event: threading.Event | None = None,
) -> int:
    console.print(_command_panel(cmd))
    logging.info("COMANDO: %s", command_text(cmd))
    emit_progress(type="status", text=description)
    emit = _progress_context()
    output_lines: list[str] = []

    def _on_line(line: str) -> None:
        output_lines.append(line)
        emit(type="line", text=line)

    process, reader = _start_subprocess_with_reader(cmd, cwd, _on_line)
    started = time.monotonic()
    progress = Progress(
        SpinnerColumn(),
        TextColumn("[bold blue]{task.description}"),
        TimeElapsedColumn(),
        console=console,
        transient=True,
    )
    try:
        with progress:
            task = progress.add_task(description, total=None)
            while process.poll() is None:
                if cancel_event is not None and cancel_event.is_set():
                    _terminate_process(process)
                    reader.join(timeout=2)
                    raise OperationCancelled("Processo cancelado pelo usuário.")
                progress.update(task, description=f"{description} | {human_duration(time.monotonic() - started)}")
                time.sleep(0.15)
    except KeyboardInterrupt:
        console.print("\n[yellow]Cancelamento solicitado. Encerrando processo...[/yellow]")
        _terminate_process(process)
        raise
    reader.join(timeout=2)
    returncode = process.returncode
    if returncode not in (0, None) and output_lines:
        console.print(
            Panel(Text("\n".join(output_lines[-25:])), title="Saída do processo (últimas linhas)", border_style="red")
        )
    return int(returncode) if returncode is not None else 1


def run_process_visible(
    cmd: Sequence[str],
    cwd: Path | None = None,
    cancel_event: threading.Event | None = None,
) -> int:
    console.print(_command_panel(cmd))
    logging.info("COMANDO: %s", command_text(cmd))
    emit_progress(type="status", text="Executando processo externo")
    emit = _progress_context()

    def _on_line(line: str) -> None:
        console.out(line)
        emit(type="line", text=line)

    process, reader = _start_subprocess_with_reader(cmd, cwd, _on_line)
    try:
        while process.poll() is None:
            if cancel_event is not None and cancel_event.is_set():
                _terminate_process(process)
                reader.join(timeout=2)
                raise OperationCancelled("Processo cancelado pelo usuário.")
            time.sleep(0.1)
    except KeyboardInterrupt:
        console.print("\n[yellow]Processo interrompido pelo usuário.[/yellow]")
        _terminate_process(process)
        raise
    reader.join(timeout=2)
    returncode = process.returncode
    return int(returncode) if returncode is not None else 1


def direct_convert(
    input_path: Path, output_folder: Path, cancel_event: threading.Event | None = None
) -> Path:
    output_folder.mkdir(parents=True, exist_ok=True)
    output_path = unique_path(output_folder / f"{input_path.stem}.md")
    with contextlib.suppress(Exception):
        execucoes.dimensao(input_path.stat().st_size / 1_000_000, "MB")
    cmd = [sys.executable, "-m", "markitdown", str(input_path), "-o", str(output_path)]
    code = run_process_with_spinner(
        cmd, f"Convertendo {input_path.name}", input_path.parent, cancel_event=cancel_event
    )
    if code != 0:
        raise RuntimeError(f"MarkItDown encerrou com código {code}.")
    if not output_path.exists() or output_path.stat().st_size == 0:
        with contextlib.suppress(Exception):
            output_path.unlink()
        raise RuntimeError("A conversão terminou sem gerar texto. Use OCR automático ou forçado.")
    log_line(f"Conversão concluída: {output_path}", style="green")
    return output_path


def pdf_is_signed(path: Path) -> bool:
    try:
        return bool(get_pdf_info(path).get("signed"))
    except Exception:
        return False


def searchable_pdf(
    input_path: Path,
    output_folder: Path,
    mode: str,
    page_spec: str = "",
    invalidate_signatures: bool = False,
    cancel_event: threading.Event | None = None,
) -> Path:
    ensure_ocr_ready()
    emit_progress(type="status", text=f"Gerando PDF pesquisável: {input_path.name}")
    if str(CONFIG.get("pdf_output_type")) == "pdfa" and find_ghostscript() is None:
        raise RuntimeError(
            "O formato PDF/A requer o Ghostscript, que não foi encontrado neste computador. "
            "Instale o Ghostscript (a instalação tenta isso automaticamente) ou troque o "
            "'Tipo de PDF' para 'pdf' nas Configurações."
        )
    output_folder.mkdir(parents=True, exist_ok=True)
    suffix = {"skip": "_PESQUISAVEL", "force": "_OCR_FORCADO", "redo": "_OCR_REFEITO"}[mode]
    output_path = unique_path(output_folder / f"{input_path.stem}{suffix}.pdf")
    if pdf_is_signed(input_path) and not invalidate_signatures:
        raise RuntimeError(
            "O PDF possui assinatura digital. O OCR alteraria o documento e invalidaria a assinatura. "
            "No menu interativo, confirme expressamente para trabalhar em uma cópia."
        )
    with contextlib.suppress(Exception):
        info = get_pdf_info(input_path)
        paginas = len(parse_page_range(page_spec, int(info["pages"]))) if page_spec else int(info["pages"])
        execucoes.dimensao(paginas, "página")
    cmd: list[str] = [
        sys.executable,
        "-m",
        "ocrmypdf",
        "--mode",
        mode,
        "--language",
        str(CONFIG["language"]),
        "--jobs",
        str(get_jobs()),
        "--output-type",
        str(CONFIG["pdf_output_type"]),
        "--optimize",
        str(CONFIG["optimize"]),
        "--rasterizer",
        str(CONFIG.get("rasterizer", "pypdfium")),
    ]
    if mode != "redo":
        if CONFIG.get("rotate_pages", True):
            cmd.append("--rotate-pages")
        if CONFIG.get("deskew", True):
            cmd.append("--deskew")
    if invalidate_signatures:
        cmd.append("--invalidate-digital-signatures")
    normalized_pages = page_spec_for_ocrmypdf(page_spec)
    if normalized_pages:
        cmd.extend(["--pages", normalized_pages])
    if verbosity_at_least("depuracao"):
        cmd.extend(["-v", "2"])
    elif verbosity_at_least("detalhado"):
        cmd.extend(["-v", "1"])
    cmd.extend([str(input_path), str(output_path)])
    code = run_process_visible(cmd, input_path.parent, cancel_event=cancel_event)
    if code != 0:
        with contextlib.suppress(Exception):
            if output_path.exists() and output_path.stat().st_size == 0:
                output_path.unlink()
        raise RuntimeError(f"OCRmyPDF encerrou com código {code}. Consulte as linhas exibidas e o registro.")
    if not output_path.exists() or output_path.stat().st_size == 0:
        raise RuntimeError("OCRmyPDF terminou sem criar o PDF de saída.")
    try:
        original_info = get_pdf_info(input_path)
        output_info = get_pdf_info(output_path)
        if original_info.get("pages") != output_info.get("pages"):
            raise RuntimeError("O PDF de saída possui quantidade de páginas diferente do original.")
    except RuntimeError:
        raise
    except Exception as exc:
        log_debug(f"Validacao de páginas não concluída: {exc}")
    log_line(f"PDF pesquisável criado: {output_path}", style="green")
    return output_path


def ocr_images(
    inputs: Sequence[Path],
    output_folder: Path,
    output_format: str,
    combined: bool = False,
    cancel_event: threading.Event | None = None,
) -> list[Path]:
    from PIL import Image, ImageOps

    ensure_ocr_ready()
    execucoes.total(len(inputs), "imagem")
    outputs: list[Path] = []
    combined_chunks: list[tuple[int, str, str]] = []
    progress = Progress(
        SpinnerColumn(),
        TextColumn("[bold blue]{task.description}"),
        BarColumn(),
        TaskProgressColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        console=console,
        transient=False,
    )
    with progress:
        task = progress.add_task("OCR de imagens", total=len(inputs))
        for index, input_path in enumerate(inputs, 1):
            if cancel_event is not None and cancel_event.is_set():
                raise OperationCancelled("OCR de imagens cancelado pelo usuário.")
            progress.console.log(f"Imagem: {input_path.name}")
            with Image.open(input_path) as raw_image:
                image = ImageOps.exif_transpose(raw_image).convert("RGB")
                text = ocr_image_text(image, str(CONFIG["language"]), int(CONFIG["psm"]))
            if combined:
                combined_chunks.append((index, input_path.name, text))
            else:
                outputs.extend(
                    write_joined_output(
                        input_path.stem, input_path.name, [(1, "OCR de imagem", text)],
                        output_folder, output_format, "OCR de imagem",
                    )
                )
            progress.advance(task)
            emit_progress(type="image", current=index, total=len(inputs), name=input_path.name)
    if combined and combined_chunks:
        base_name = f"Imagens_OCR_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
        outputs.extend(
            write_joined_output(
                base_name, f"{len(inputs)} imagens", combined_chunks,
                output_folder, output_format, "OCR de imagem (lote)",
            )
        )
    return outputs


def diagnose_rows(path: Path) -> list[tuple[str, str]]:
    """Linhas (item, resultado) do diagnostico de um arquivo.

    Extraida de diagnose_file() para ser reutilizada tanto pelo terminal
    (que a exibe em uma Table do Rich) quanto pela interface grafica (que a
    exibe em um Treeview do tkinter), sem duplicar a logica.
    """
    rows: list[tuple[str, str]] = [
        ("Caminho", str(path)),
        ("Tamanho", human_size(path.stat().st_size)),
        ("Extensao", path.suffix.lower()),
    ]
    if path.suffix.lower() == ".pdf":
        info = get_pdf_info(path, detailed=True)
        rows.append(("Classificacao", str(info["classification"])))
        rows.append(("Paginas", str(info["pages"])))
        rows.append(("PDF protegido", "Sim" if info["encrypted"] else "Não"))
        rows.append(("Assinatura digital", "Detectada" if info["signed"] else "Não detectada"))
        if not info["encrypted"]:
            rows.append(("Páginas com texto nativo", f"{info['native_pages']} de {info['sampled_pages']}"))
            rows.append(("Páginas com pouco/nenhum texto", f"{info['scan_pages']} de {info['sampled_pages']}"))
            rows.append(("Páginas com imagens", f"{info['image_pages']} de {info['sampled_pages']}"))
    elif path.suffix.lower() in SUPPORTED_IMAGES:
        from PIL import Image

        with Image.open(path) as image:
            rows.append(("Dimensoes", f"{image.width} x {image.height}"))
            rows.append(("Formato", str(image.format)))
            rows.append(("Modo de cor", str(image.mode)))
    return rows


def diagnose_file(path: Path) -> None:
    table = Table(title=f"Diagnóstico: {path.name}", box=box.ROUNDED)
    table.add_column("Item", style="cyan")
    table.add_column("Resultado")
    for item, result in diagnose_rows(path):
        table.add_row(item, result)
    console.print(table)


INSTALLATION_ROW_COUNT = 10  # linhas de iter_installation_rows (total conhecido da verificacao)


def iter_installation_rows():
    """Gera, uma a uma, as linhas (componente, ok, detalhes) da verificacao.

    Gerador para a interface poder mostrar cada componente assim que e
    verificado (importar o OCRmyPDF ou consultar o Tesseract leva segundos).
    """
    yield ("Python", sys.version_info[:2] >= (3, 11), sys.version.split()[0])
    try:
        import importlib.metadata as metadata

        version = metadata.version("markitdown")
        try:
            parts = tuple(int(x) for x in re.findall(r"\d+", version)[:3])
            version_ok = parts >= (0, 1, 6)
        except Exception:
            version_ok = True
        yield ("MarkItDown", version_ok, version)
    except Exception as exc:
        yield ("MarkItDown", False, mensagens.erro_usuario(exc, "carregar o MarkItDown"))
    for package_name, import_name in [
        ("PyMuPDF", "fitz"),
        ("pytesseract", "pytesseract"),
        ("Pillow", "PIL"),
        ("OCRmyPDF", "ocrmypdf"),
        ("Rich", "rich"),
    ]:
        try:
            __import__(import_name)
            yield (package_name, True, "instalado")
        except Exception as exc:
            yield (package_name, False, mensagens.erro_usuario(exc, "carregar a biblioteca"))
    executable, languages = configure_tesseract()
    yield ("Tesseract", executable is not None, str(executable) if executable else "não encontrado")
    missing = required_languages(str(CONFIG["language"])) - set(languages)
    yield ("Idiomas OCR", not missing, ", ".join(languages) if languages else "nenhum")
    gs_path = find_ghostscript()
    gs_required = str(CONFIG.get("pdf_output_type")) == "pdfa"
    yield ("Ghostscript (PDF/A)", gs_path is not None or not gs_required, str(gs_path) if gs_path else ("não encontrado" if gs_required else "não instalado (opcional, só para PDF/A)"))


def installation_rows() -> list[tuple[str, bool, str]]:
    """Linhas (componente, ok, detalhes) da verificacao de instalacao.

    Reutilizada pela interface grafica e por --check sem depender da Table do Rich.
    """
    return list(iter_installation_rows())


def check_installation(show_panel: bool = True) -> bool:
    rows = installation_rows()
    all_ok = all(ok for _, ok, _ in rows)
    if show_panel:
        table = Table(title="Verificação da instalação", box=box.ROUNDED)
        table.add_column("Componente")
        table.add_column("Estado")
        table.add_column("Detalhes")
        for name, ok, detail in rows:
            table.add_row(name, "[green]OK[/green]" if ok else "[red]FALHA[/red]", detail)
        console.print(table)
        console.print(f"Registro da sessão: [dim]{SESSION_LOG}[/dim]")
    return all_ok


def handle_error(exc: Exception) -> None:
    logging.exception("Falha: %s", exc)
    console.print(Panel(f"[bold red]ERRO[/bold red]\n{mensagens.erro_usuario(exc)}", border_style="red"))
    if verbosity_at_least("depuracao"):
        console.print(traceback.format_exc(), style="dim")
    console.print(f"[dim]Registro: {SESSION_LOG}[/dim]")


def interactive_direct() -> None:
    files = select_files(
        "Selecione os arquivos para conversão direta",
        [("Documentos suportados", "*.pdf *.docx *.pptx *.xlsx *.html *.htm *.txt *.csv *.json *.xml"), ("Todos", "*.*")],
        multiple=True,
    )
    files = [path for path in files if path.exists() and path.suffix.lower() in SUPPORTED_DIRECT]
    if not files:
        return
    output_folder = choose_output_folder(files)
    results = []
    for index, path in enumerate(files, 1):
        log_line(f"Arquivo {index}/{len(files)}: {path.name}")
        try:
            results.append(direct_convert(path, output_folder))
        except Exception as exc:
            handle_error(exc)
    if results and CONFIG.get("auto_open_output"):
        open_folder(output_folder)


def interactive_ocr_text(mode: str) -> None:
    files = select_files("Selecione os PDFs", [("PDF", "*.pdf")], multiple=True)
    files = [path for path in files if path.exists() and path.suffix.lower() == ".pdf"]
    if not files:
        return
    output_folder = choose_output_folder(files)
    output_format = choose_output_format()
    for index, path in enumerate(files, 1):
        console.rule(f"Arquivo {index}/{len(files)}: {path.name}")
        try:
            info = get_pdf_info(path)
            if info.get("encrypted"):
                raise RuntimeError("O PDF está protegido por senha.")
            page_spec = choose_page_range(int(info["pages"]))
            ocr_pdf_to_text(path, output_folder, mode, page_spec, output_format)
        except KeyboardInterrupt:
            console.print("[yellow]OCR interrompido. O progresso foi preservado para retomada.[/yellow]")
            break
        except Exception as exc:
            handle_error(exc)
    if CONFIG.get("auto_open_output"):
        open_folder(output_folder)


def interactive_searchable(mode: str, also_markdown: bool = False) -> None:
    files = select_files("Selecione os PDFs", [("PDF", "*.pdf")], multiple=True)
    files = [path for path in files if path.exists() and path.suffix.lower() == ".pdf"]
    if not files:
        return
    output_folder = choose_output_folder(files)
    for index, path in enumerate(files, 1):
        console.rule(f"Arquivo {index}/{len(files)}: {path.name}")
        try:
            info = get_pdf_info(path)
            if info.get("encrypted"):
                raise RuntimeError("O PDF está protegido por senha. Remova a senha antes do OCR.")
            invalidate = False
            if info.get("signed"):
                console.print(
                    Panel(
                        "Este PDF possui assinatura digital. O original será preservado, mas a cópia com OCR terá a assinatura invalidada.",
                        border_style="yellow",
                    )
                )
                invalidate = Confirm.ask("Deseja prosseguir mesmo assim?", default=False)
                if not invalidate:
                    continue
            page_spec = choose_page_range(int(info["pages"]))
            pdf_output = searchable_pdf(path, output_folder, mode, page_spec, invalidate)
            if also_markdown:
                try:
                    direct_convert(pdf_output, output_folder)
                except Exception as exc:
                    console.print("[yellow]A conversão do PDF pesquisável para Markdown falhou. Tentando extração automática...[/yellow]")
                    handle_error(exc)
                    ocr_pdf_to_text(pdf_output, output_folder, "auto", "", "md")
        except KeyboardInterrupt:
            console.print("[yellow]Processamento interrompido.[/yellow]")
            break
        except Exception as exc:
            handle_error(exc)
    if CONFIG.get("auto_open_output"):
        open_folder(output_folder)


def interactive_images() -> None:
    files = select_files(
        "Selecione as imagens",
        [("Imagens", "*.png *.jpg *.jpeg *.tif *.tiff *.bmp *.webp"), ("Todos", "*.*")],
        multiple=True,
    )
    files = [path for path in files if path.exists() and path.suffix.lower() in SUPPORTED_IMAGES]
    if not files:
        return
    output_folder = choose_output_folder(files)
    output_format = choose_output_format()
    combined = False
    if len(files) > 1:
        combined = Confirm.ask("Combinar todas as imagens em um único arquivo de saída?", default=True)
    try:
        ocr_images(files, output_folder, output_format, combined)
    except Exception as exc:
        handle_error(exc)


def batch_menu() -> None:
    folder = select_folder("Selecione a pasta para processamento em lote")
    if not folder or not folder.exists():
        return
    console.print(
        "[1] Conversão direta\n"
        "[2] OCR automático para Markdown\n"
        "[3] OCR forçado para Markdown\n"
        "[4] PDF pesquisável\n"
        "[5] PDF pesquisável + Markdown"
    )
    choice = Prompt.ask("Modo do lote", choices=["1", "2", "3", "4", "5"], default="2")
    recursive = Confirm.ask("Incluir subpastas?", default=False)
    output_folder = select_folder("Escolha a pasta de saída") or (folder / "Extrator_saida")
    output_folder.mkdir(parents=True, exist_ok=True)
    pattern = "**/*" if recursive else "*"
    allowed = SUPPORTED_DIRECT if choice == "1" else {".pdf"}
    files = sorted(
        path
        for path in folder.glob(pattern)
        if path.is_file()
        and path.suffix.lower() in allowed
        and not path.stem.endswith(OUTPUT_NAME_SUFFIXES)
    )
    if not files:
        console.print("[yellow]Nenhum arquivo compatível encontrado.[/yellow]")
        return
    console.print(f"Arquivos encontrados: [bold]{len(files)}[/bold]")
    if not Confirm.ask("Iniciar o lote?", default=True):
        return
    overall = Progress(
        SpinnerColumn(),
        TextColumn("[bold cyan]{task.description}"),
        BarColumn(),
        TaskProgressColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        TimeRemainingColumn(),
        console=console,
        transient=False,
    )
    with overall:
        task = overall.add_task("Arquivos do lote", total=len(files))
        for path in files:
            overall.console.log(f"Iniciando: {path.name}")
            try:
                if choice == "1":
                    direct_convert(path, output_folder)
                elif choice == "2":
                    ocr_pdf_to_text(path, output_folder, "auto", "", "md")
                elif choice == "3":
                    ocr_pdf_to_text(path, output_folder, "force", "", "md")
                elif choice == "4":
                    searchable_pdf(path, output_folder, "skip")
                elif choice == "5":
                    generated = searchable_pdf(path, output_folder, "skip")
                    direct_convert(generated, output_folder)
            except KeyboardInterrupt:
                raise
            except Exception as exc:
                overall.console.log(f"[red]Falha em {path.name}: {mensagens.erro_usuario(exc)}[/red]")
                logging.exception("Falha no lote: %s", path)
            finally:
                overall.advance(task)
    console.print(f"[green]Lote encerrado. Saída: {output_folder}[/green]")


def settings_menu() -> None:
    global CONFIG
    while True:
        clear_screen()
        show_header()
        table = Table(title="Configurações atuais", box=box.ROUNDED)
        table.add_column("Opcao")
        table.add_column("Valor")
        labels = [
            ("1", "Idioma OCR", "language"),
            ("2", "DPI do OCR", "dpi"),
            ("3", "PSM Tesseract", "psm"),
            ("4", "Limite de texto nativo", "native_text_threshold"),
            ("5", "Nível de exibição", "verbosity"),
            ("6", "Uso de CPU", "cpu_mode"),
            ("7", "Rotação automática", "rotate_pages"),
            ("8", "Corrigir inclinação", "deskew"),
            ("9", "Tipo de PDF", "pdf_output_type"),
            ("10", "Retomar OCR interrompido", "resume"),
            ("11", "Abrir pasta ao concluir", "auto_open_output"),
            ("12", "Pasta de saída", "output_location"),
        ]
        for number, label, key in labels:
            table.add_row(f"[{number}] {label}", str(CONFIG.get(key)))
        console.print(table)
        console.print("[13] Restaurar padrões\n[0] Voltar")
        choice = Prompt.ask("Escolha", default="0")
        if choice == "0":
            save_config(CONFIG)
            return
        if choice == "1":
            CONFIG["language"] = Prompt.ask("Idiomas Tesseract", default=str(CONFIG["language"]))
        elif choice == "2":
            CONFIG["dpi"] = IntPrompt.ask("DPI (180 rápido, 220 equilibrado, 300 qualidade)", default=int(CONFIG["dpi"]))
        elif choice == "3":
            CONFIG["psm"] = IntPrompt.ask("PSM (3 automático, 6 bloco único, 11 texto disperso)", default=int(CONFIG["psm"]))
        elif choice == "4":
            CONFIG["native_text_threshold"] = IntPrompt.ask("Caracteres mínimos para considerar texto nativo", default=int(CONFIG["native_text_threshold"]))
        elif choice == "5":
            CONFIG["verbosity"] = Prompt.ask("Nivel", choices=["normal", "detalhado", "depuracao"], default=str(CONFIG["verbosity"]))
        elif choice == "6":
            CONFIG["cpu_mode"] = Prompt.ask("Uso de CPU", choices=["economico", "normal", "maximo"], default=str(CONFIG["cpu_mode"]))
        elif choice == "7":
            CONFIG["rotate_pages"] = Confirm.ask("Ativar rotação automática?", default=bool(CONFIG["rotate_pages"]))
        elif choice == "8":
            CONFIG["deskew"] = Confirm.ask("Corrigir páginas inclinadas?", default=bool(CONFIG["deskew"]))
        elif choice == "9":
            CONFIG["pdf_output_type"] = Prompt.ask("Tipo", choices=["pdf", "pdfa"], default=str(CONFIG["pdf_output_type"]))
        elif choice == "10":
            CONFIG["resume"] = Confirm.ask("Retomar trabalhos interrompidos?", default=bool(CONFIG["resume"]))
        elif choice == "11":
            CONFIG["auto_open_output"] = Confirm.ask("Abrir pasta ao concluir?", default=bool(CONFIG["auto_open_output"]))
        elif choice == "12":
            CONFIG["output_location"] = Prompt.ask("Saida", choices=["same", "ask"], default=str(CONFIG["output_location"]))
        elif choice == "13":
            CONFIG = DEFAULT_CONFIG.copy()
        save_config(CONFIG)


def show_help() -> None:
    console.print(
        Panel(
            "[bold]Conversão direta[/bold]: usa MarkItDown e não executa OCR.\n\n"
            "[bold]OCR automático para Markdown[/bold]: preserva texto nativo e aplica OCR apenas nas páginas sem texto suficiente.\n\n"
            "[bold]OCR forçado para Markdown[/bold]: rasteriza e reconhece todas as páginas. É mais lento.\n\n"
            "[bold]PDF pesquisável[/bold]: usa OCRmyPDF para manter a aparência do PDF e adicionar uma camada pesquisável.\n\n"
            "[bold]Refazer OCR[/bold]: substitui uma camada de OCR existente quando possível.\n\n"
            "O original nunca e sobrescrito. Ctrl+C cancela. O OCR para Markdown pode ser retomado depois.",
            title="Ajuda rápida",
            border_style="cyan",
        )
    )
    console.print(f"Dados e configurações (dentro da pasta do programa): [dim]{APP_DATA_DIR}[/dim]")
    console.print(f"Registro desta sessão: [dim]{SESSION_LOG}[/dim]")


def clear_data_interactive() -> None:
    console.print(
        Panel(
            f"Isto remove todos os registros e todo o cache de retomada de OCR em:\n{APP_DATA_DIR}\n\n"
            "A configuração (dados/config.json) é preservada.",
            border_style="yellow",
        )
    )
    if not Confirm.ask("Confirma a limpeza agora?", default=False):
        return
    removed_logs, removed_jobs = cleanup_data(retention_days=0)
    console.print(f"[green]Removidos {removed_logs} arquivo(s) de registro e {removed_jobs} pasta(s) de cache.[/green]")


def diagnose_interactive() -> None:
    files = select_files("Selecione um arquivo", [("PDF e imagens", "*.pdf *.png *.jpg *.jpeg *.tif *.tiff *.bmp *.webp"), ("Todos", "*.*")], multiple=False)
    if files and files[0].exists():
        diagnose_file(files[0])


def main_menu() -> None:
    clear_screen()
    while True:
        show_header()
        console.print(
            "[bold cyan][1][/bold cyan] Conversão direta para Markdown (sem OCR)\n"
            "[bold cyan][2][/bold cyan] OCR automático para Markdown/TXT\n"
            "[bold cyan][3][/bold cyan] OCR forçado para Markdown/TXT\n"
            "[bold cyan][4][/bold cyan] Criar PDF pesquisável (OCR automático)\n"
            "[bold cyan][5][/bold cyan] Criar PDF pesquisável + Markdown\n"
            "[bold cyan][6][/bold cyan] Criar PDF pesquisável com OCR forçado\n"
            "[bold cyan][7][/bold cyan] Refazer OCR de PDF existente\n"
            "[bold cyan][8][/bold cyan] OCR de imagens\n"
            "[bold cyan][9][/bold cyan] Processar pasta em lote\n"
            "[bold cyan][10][/bold cyan] Diagnosticar arquivo\n"
            "[bold cyan][11][/bold cyan] Verificar instalação\n"
            "[bold cyan][12][/bold cyan] Configurações\n"
            "[bold cyan][13][/bold cyan] Ajuda\n"
            "[bold cyan][14][/bold cyan] Limpar cache e registros\n"
            "[bold cyan][0][/bold cyan] Sair"
        )
        choice = Prompt.ask("\nEscolha uma opção", default="2")
        try:
            if choice == "0":
                return
            if choice == "1":
                interactive_direct()
            elif choice == "2":
                interactive_ocr_text("auto")
            elif choice == "3":
                interactive_ocr_text("force")
            elif choice == "4":
                interactive_searchable("skip", False)
            elif choice == "5":
                interactive_searchable("skip", True)
            elif choice == "6":
                interactive_searchable("force", False)
            elif choice == "7":
                interactive_searchable("redo", False)
            elif choice == "8":
                interactive_images()
            elif choice == "9":
                batch_menu()
            elif choice == "10":
                diagnose_interactive()
            elif choice == "11":
                check_installation()
            elif choice == "12":
                settings_menu()
                continue
            elif choice == "13":
                show_help()
            elif choice == "14":
                clear_data_interactive()
            else:
                console.print("[yellow]Opção inválida.[/yellow]")
        except KeyboardInterrupt:
            console.print("\n[yellow]Operação cancelada.[/yellow]")
        except Exception as exc:
            handle_error(exc)
        pause()


def cli_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="extrator", description=APP_NAME)
    parser.add_argument("--version", action="version", version=APP_VERSION)
    parser.add_argument("--check", action="store_true", help="verifica a instalação")
    sub = parser.add_subparsers(dest="command")

    direct = sub.add_parser("direct", help="conversão direta sem OCR")
    direct.add_argument("files", nargs="+")
    direct.add_argument("-o", "--output", type=Path)

    for name, help_text in [("auto-ocr", "OCR automático para Markdown"), ("force-ocr", "OCR forçado para Markdown")]:
        command = sub.add_parser(name, help=help_text)
        command.add_argument("files", nargs="+")
        command.add_argument("-o", "--output", type=Path)
        command.add_argument("--pages", default="")
        command.add_argument("--format", choices=["md", "txt", "both"], default="md")

    for name, mode, help_text in [
        ("searchable", "skip", "cria PDF pesquisável"),
        ("force-pdf", "force", "cria PDF pesquisável com OCR forçado"),
        ("redo-pdf", "redo", "refaz OCR existente"),
    ]:
        command = sub.add_parser(name, help=help_text)
        command.add_argument("files", nargs="+")
        command.add_argument("-o", "--output", type=Path)
        command.add_argument("--pages", default="")
        command.add_argument("--invalidate-signatures", action="store_true")
        command.set_defaults(pdf_mode=mode)

    diagnose = sub.add_parser("diagnose", help="diagnostica arquivo")
    diagnose.add_argument("file")
    return parser


def run_cli(args: argparse.Namespace) -> int:
    if args.check:
        return 0 if check_installation() else 1
    if not args.command:
        main_menu()
        return 0
    try:
        if args.command == "diagnose":
            diagnose_file(Path(args.file))
            return 0
        files = [Path(item).expanduser().resolve() for item in args.files]
        output_folder = (args.output or files[0].parent).resolve()
        if args.command == "direct":
            for path in files:
                direct_convert(path, output_folder)
        elif args.command in {"auto-ocr", "force-ocr"}:
            mode = "auto" if args.command == "auto-ocr" else "force"
            for path in files:
                ocr_pdf_to_text(path, output_folder, mode, args.pages, args.format)
        elif args.command in {"searchable", "force-pdf", "redo-pdf"}:
            for path in files:
                searchable_pdf(path, output_folder, args.pdf_mode, args.pages, args.invalidate_signatures)
        return 0
    except KeyboardInterrupt:
        console.print("[yellow]Operação cancelada.[/yellow]")
        return 130
    except Exception as exc:
        handle_error(exc)
        return 1


def main() -> int:
    parser = cli_parser()
    args = parser.parse_args()
    return run_cli(args)


if __name__ == "__main__":
    raise SystemExit(main())
