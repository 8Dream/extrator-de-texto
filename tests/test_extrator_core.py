"""Testes automatizados do Extrator de Texto.

Cobrem as funcoes puras e os pontos onde ja houve bugs reais (deteccao de
assinatura invertida, listagem de idiomas do Tesseract, limpeza de cache).
Nao dependem de Tesseract/OCRmyPDF instalados: os testes que precisariam
disso usam PyMuPDF (dependencia normal do projeto) para gerar PDFs
sinteticos e validar apenas a logica que decide o que fazer com cada
pagina, sem executar OCR de fato.

Executar com: venv\\Scripts\\python.exe -m pytest
"""
from __future__ import annotations

import os
import queue
import sys
import threading
import time
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

import extrator_core as app


# ---------------------------------------------------------------------------
# Funcoes utilitarias puras
# ---------------------------------------------------------------------------


def test_normalize_text_strips_and_collapses_blank_lines():
    text = "linha \t\n\n\n\n\noutra  "
    assert app.normalize_text(text) == "linha\n\n\noutra"


def test_normalize_text_removes_null_bytes():
    assert app.normalize_text("a\x00b") == "ab"


def test_human_size_progression():
    assert app.human_size(500) == "500.0 B"
    assert app.human_size(2048) == "2.0 KB"
    assert app.human_size(2 * 1024 * 1024) == "2.0 MB"


def test_human_duration_without_hours():
    assert app.human_duration(65) == "01:05"


def test_human_duration_with_hours():
    assert app.human_duration(3665) == "01:01:05"


def test_human_duration_never_negative():
    assert app.human_duration(-10) == "00:00"


def test_unique_path_appends_counter_when_exists(tmp_path):
    target = tmp_path / "arquivo.txt"
    target.write_text("x", encoding="utf-8")
    result = app.unique_path(target)
    assert result == tmp_path / "arquivo (1).txt"


def test_unique_path_returns_same_when_free(tmp_path):
    target = tmp_path / "novo.txt"
    assert app.unique_path(target) == target


# ---------------------------------------------------------------------------
# parse_page_range / page_spec_for_ocrmypdf
# ---------------------------------------------------------------------------


def test_parse_page_range_empty_means_all_pages():
    assert app.parse_page_range("", 5) == [0, 1, 2, 3, 4]
    assert app.parse_page_range("todas", 5) == [0, 1, 2, 3, 4]


def test_parse_page_range_mixed_spec():
    assert app.parse_page_range("1-3,5", 5) == [0, 1, 2, 4]


def test_parse_page_range_reversed_interval_is_normalized():
    assert app.parse_page_range("5-3", 5) == [2, 3, 4]


def test_parse_page_range_ignores_out_of_bounds_pages():
    assert app.parse_page_range("1,999", 5) == [0]


def test_parse_page_range_invalid_letter_raises_clear_error():
    with pytest.raises(ValueError):
        app.parse_page_range("1-x", 10)
    with pytest.raises(ValueError):
        app.parse_page_range("abc", 10)


def test_parse_page_range_only_out_of_range_raises():
    with pytest.raises(ValueError):
        app.parse_page_range("999", 5)


def test_page_spec_for_ocrmypdf_all_returns_none():
    assert app.page_spec_for_ocrmypdf("") is None
    assert app.page_spec_for_ocrmypdf("todas") is None


def test_page_spec_for_ocrmypdf_translates_fim_to_end():
    assert app.page_spec_for_ocrmypdf("10-fim") == "10-end"


# ---------------------------------------------------------------------------
# write_joined_output
# ---------------------------------------------------------------------------


def test_write_joined_output_md_marks_ocr_pages(tmp_path):
    chunks = [(1, "texto nativo", "abc"), (2, "OCR", "def")]
    outputs = app.write_joined_output("base", "origem.pdf", chunks, tmp_path, "md", "auto")
    content = outputs[0].read_text(encoding="utf-8")
    assert "Paginas processadas por OCR: 2" in content
    assert "## Pagina 1 (texto nativo)" in content
    assert "## Pagina 2 (OCR)" in content


def test_write_joined_output_omits_ocr_line_when_all_native(tmp_path):
    chunks = [(1, "texto nativo", "abc")]
    outputs = app.write_joined_output("base", "origem.pdf", chunks, tmp_path, "md", "auto")
    content = outputs[0].read_text(encoding="utf-8")
    assert "Paginas processadas por OCR" not in content


def test_write_joined_output_txt_format(tmp_path):
    chunks = [(1, "OCR", "hello")]
    outputs = app.write_joined_output("base", "origem.png", chunks, tmp_path, "txt", "OCR de imagem")
    assert outputs[0].suffix == ".txt"
    assert "hello" in outputs[0].read_text(encoding="utf-8")


def test_write_joined_output_both_formats(tmp_path):
    chunks = [(1, "OCR", "hello")]
    outputs = app.write_joined_output("base", "origem.png", chunks, tmp_path, "both", "OCR de imagem")
    assert {p.suffix for p in outputs} == {".md", ".txt"}


# ---------------------------------------------------------------------------
# Deteccao de assinatura digital (bug corrigido: get_sigflags() == -1 sem
# assinatura era interpretado como bool(-1) == True)
# ---------------------------------------------------------------------------


def _make_native_pdf(path: Path, image_covers_page: bool = False) -> None:
    import fitz

    doc = fitz.open()
    page = doc.new_page()
    long_text = (
        "Texto nativo de teste com bastante conteudo para superar o limiar "
        "configurado de caracteres considerados nativos pelo programa."
    )
    page.insert_text((50, 100), long_text)
    if image_covers_page:
        stamp_doc = fitz.open()
        stamp_page = stamp_doc.new_page(width=600, height=800)
        img_bytes = stamp_page.get_pixmap(dpi=72).tobytes("png")
        stamp_doc.close()
        page.insert_image(page.rect, stream=img_bytes)
    doc.save(path)
    doc.close()


def test_get_pdf_info_signed_is_false_for_unsigned_pdf(tmp_path):
    pytest.importorskip("fitz")
    pdf_path = tmp_path / "sem_assinatura.pdf"
    _make_native_pdf(pdf_path)
    info = app.get_pdf_info(pdf_path)
    assert info["signed"] is False


# ---------------------------------------------------------------------------
# Heuristica de pagina digitalizada com carimbo de texto nativo
# ---------------------------------------------------------------------------


def test_scanned_image_coverage_full_page_image_is_high(tmp_path):
    fitz = pytest.importorskip("fitz")
    pdf_path = tmp_path / "carimbo.pdf"
    _make_native_pdf(pdf_path, image_covers_page=True)
    doc = fitz.open(pdf_path)
    try:
        coverage = app._scanned_image_coverage(doc.load_page(0))
    finally:
        doc.close()
    assert coverage >= app.SCANNED_IMAGE_COVERAGE_THRESHOLD


def test_scanned_image_coverage_pure_text_page_is_low(tmp_path):
    fitz = pytest.importorskip("fitz")
    pdf_path = tmp_path / "sem_imagem.pdf"
    _make_native_pdf(pdf_path, image_covers_page=False)
    doc = fitz.open(pdf_path)
    try:
        coverage = app._scanned_image_coverage(doc.load_page(0))
    finally:
        doc.close()
    assert coverage < app.SCANNED_IMAGE_COVERAGE_THRESHOLD


def test_process_single_page_forces_ocr_when_stamp_over_scanned_image(tmp_path, monkeypatch):
    """Reproduz o cenario do relatorio original: uma pagina digitalizada
    carrega um carimbo de texto nativo (ex.: "assinado digitalmente") que
    sozinho ultrapassa o limiar de caracteres. Sem a heuristica de cobertura
    de imagem, essa pagina seria erroneamente tratada como "texto nativo" e
    o conteudo digitalizado nunca passaria pelo OCR.
    """
    pytest.importorskip("fitz")
    pdf_path = tmp_path / "digitalizada_com_carimbo.pdf"
    _make_native_pdf(pdf_path, image_covers_page=True)

    calls = []
    monkeypatch.setattr(
        app, "ocr_image_text", lambda image, language, psm: calls.append(1) and "TEXTO_OCR" or "TEXTO_OCR"
    )

    page_number, source, text = app._process_single_page(
        (str(pdf_path), 0, "auto", 80, 100, "por+eng", 3)
    )
    assert source == "OCR"
    assert calls, "OCR deveria ter sido executado mesmo com texto nativo acima do limiar"


def test_process_single_page_keeps_native_text_without_image(tmp_path, monkeypatch):
    pytest.importorskip("fitz")
    pdf_path = tmp_path / "nativo_puro.pdf"
    _make_native_pdf(pdf_path, image_covers_page=False)

    def _fail_if_called(*_args, **_kwargs):
        raise AssertionError("OCR nao deveria ser executado em pagina nativa sem imagem dominante")

    monkeypatch.setattr(app, "ocr_image_text", _fail_if_called)

    page_number, source, text = app._process_single_page(
        (str(pdf_path), 0, "auto", 80, 100, "por+eng", 3)
    )
    assert source == "texto nativo"
    assert text


# ---------------------------------------------------------------------------
# cleanup_data
# ---------------------------------------------------------------------------


def test_cleanup_data_removes_only_old_entries(tmp_path, monkeypatch):
    log_dir = tmp_path / "logs"
    work_dir = tmp_path / "work"
    log_dir.mkdir()
    work_dir.mkdir()
    monkeypatch.setattr(app, "LOG_DIR", log_dir)
    monkeypatch.setattr(app, "WORK_DIR", work_dir)

    old_log = log_dir / "old.log"
    new_log = log_dir / "new.log"
    old_log.write_text("x", encoding="utf-8")
    new_log.write_text("x", encoding="utf-8")
    old_job = work_dir / "old_job"
    old_job.mkdir()
    (old_job / "page_000001.json").write_text("{}", encoding="utf-8")

    old_time = time.time() - 10 * 86400
    os.utime(old_log, (old_time, old_time))
    os.utime(old_job, (old_time, old_time))

    removed_logs, removed_jobs = app.cleanup_data(retention_days=7)

    assert removed_logs == 1
    assert removed_jobs == 1
    assert not old_log.exists()
    assert new_log.exists()
    assert not old_job.exists()


def test_cleanup_data_retention_zero_removes_everything(tmp_path, monkeypatch):
    log_dir = tmp_path / "logs"
    work_dir = tmp_path / "work"
    log_dir.mkdir()
    work_dir.mkdir()
    monkeypatch.setattr(app, "LOG_DIR", log_dir)
    monkeypatch.setattr(app, "WORK_DIR", work_dir)
    (log_dir / "recent.log").write_text("x", encoding="utf-8")

    removed_logs, _ = app.cleanup_data(retention_days=0)
    assert removed_logs == 1


# ---------------------------------------------------------------------------
# Listagem de idiomas do Tesseract (bug corrigido: UnicodeDecodeError quando
# o caminho do TESSDATA_PREFIX contem caracteres acentuados)
# ---------------------------------------------------------------------------


def test_list_tesseract_languages_handles_non_utf8_output(monkeypatch):
    class FakeResult:
        stdout = 'List of available languages in "C:\\Ferrament\xe1rio\\dados\\tessdata/" (2):\neng\npor\n'.encode(
            "latin-1"
        )

    monkeypatch.setattr(app.subprocess, "run", lambda *a, **k: FakeResult())
    languages = app.list_tesseract_languages(Path("tesseract.exe"))
    assert languages == ["eng", "por"]


def test_list_tesseract_languages_returns_empty_on_failure(monkeypatch):
    def _raise(*_a, **_k):
        raise FileNotFoundError("tesseract nao encontrado")

    monkeypatch.setattr(app.subprocess, "run", _raise)
    assert app.list_tesseract_languages(Path("tesseract.exe")) == []


# ---------------------------------------------------------------------------
# Lote: nao reprocessar arquivos ja gerados por execucoes anteriores
# ---------------------------------------------------------------------------


def test_output_name_suffixes_cover_all_generated_kinds():
    for suffix in ("_OCR", "_PESQUISAVEL", "_OCR_FORCADO", "_OCR_REFEITO"):
        assert suffix in app.OUTPUT_NAME_SUFFIXES


def test_batch_glob_skips_previously_generated_files(tmp_path):
    (tmp_path / "processo.pdf").write_bytes(b"%PDF-1.4")
    (tmp_path / "processo_PESQUISAVEL.pdf").write_bytes(b"%PDF-1.4")
    (tmp_path / "processo_OCR_FORCADO.pdf").write_bytes(b"%PDF-1.4")

    files = sorted(
        path
        for path in tmp_path.glob("*")
        if path.is_file()
        and path.suffix.lower() == ".pdf"
        and not path.stem.endswith(app.OUTPUT_NAME_SUFFIXES)
    )
    assert [p.name for p in files] == ["processo.pdf"]


# ---------------------------------------------------------------------------
# Progresso estruturado e cancelamento cooperativo (usados pela interface
# grafica; o terminal nunca chama set_progress_queue, entao seu comportamento
# nao muda)
# ---------------------------------------------------------------------------


def test_emit_progress_without_queue_is_a_noop():
    app.set_progress_queue(None)
    app.emit_progress(type="status", text="sem fila registrada, nao deve lancar erro")


def test_emit_progress_puts_event_on_registered_queue():
    q = queue.Queue()
    app.set_progress_queue(q)
    try:
        app.emit_progress(type="status", text="ola")
        event = q.get_nowait()
        assert event == {"type": "status", "text": "ola"}
    finally:
        app.set_progress_queue(None)


def test_emit_progress_is_per_thread():
    """A GUI roda o trabalho em uma thread separada; a fila registrada ali
    nao deve vazar para outras threads (por exemplo, a thread principal do
    Tk, que nao deve receber eventos de progresso)."""
    main_queue = queue.Queue()
    worker_queue = queue.Queue()
    app.set_progress_queue(main_queue)

    def _worker():
        app.set_progress_queue(worker_queue)
        app.emit_progress(type="status", text="da thread worker")

    thread = threading.Thread(target=_worker)
    thread.start()
    thread.join()

    app.emit_progress(type="status", text="da thread principal")

    assert worker_queue.get_nowait()["text"] == "da thread worker"
    assert main_queue.get_nowait()["text"] == "da thread principal"
    app.set_progress_queue(None)


def test_ocr_pdf_to_text_raises_when_pre_cancelled(tmp_path):
    fitz = pytest.importorskip("fitz")
    pdf_path = tmp_path / "cancelar.pdf"
    _make_native_pdf(pdf_path, image_covers_page=True)  # forca passagem pelo OCR

    cancel_event = threading.Event()
    cancel_event.set()
    with pytest.raises(app.OperationCancelled):
        app.ocr_pdf_to_text(pdf_path, tmp_path, "auto", "", "md", cancel_event=cancel_event)
    # cancelado antes de qualquer pagina ser escrita: nenhuma saida deve existir
    assert not list(tmp_path.glob("*_OCR.md"))


def test_ocr_images_raises_when_pre_cancelled(tmp_path, monkeypatch):
    from PIL import Image

    img_path = tmp_path / "imagem.png"
    Image.new("RGB", (50, 50), color="white").save(img_path)

    def _fail_if_called(*_a, **_k):
        raise AssertionError("OCR nao deveria executar apos cancelamento")

    monkeypatch.setattr(app, "ocr_image_text", _fail_if_called)

    cancel_event = threading.Event()
    cancel_event.set()
    with pytest.raises(app.OperationCancelled):
        app.ocr_images([img_path], tmp_path, "md", cancel_event=cancel_event)


# ---------------------------------------------------------------------------
# Painel/linha de comando seguros contra markup do Rich (nomes de arquivo com
# colchetes, comuns em numeros de processo, ou saida de subprocessos com
# repr de lista Python como "['tesseract.EXE', ...]")
# ---------------------------------------------------------------------------


def test_command_panel_survives_brackets_in_filename():
    panel = app._command_panel(["python", "-m", "markitdown", "Processo [123-456].pdf"])
    with app.console.capture() as capture:
        app.console.print(panel)
    assert "Processo [123-456].pdf" in capture.get()


def test_command_panel_survives_python_list_repr_style_text():
    panel = app._command_panel(["tesseract.exe", "-l", "por+eng", "['a', 'b']"])
    with app.console.capture() as capture:
        app.console.print(panel)
    assert "['a', 'b']" in capture.get()


# ---------------------------------------------------------------------------
# installation_rows / diagnose_rows (usados pela interface grafica)
# ---------------------------------------------------------------------------


def test_installation_rows_returns_python_row():
    rows = app.installation_rows()
    names = [row[0] for row in rows]
    assert "Python" in names


def test_diagnose_rows_for_pdf(tmp_path):
    pytest.importorskip("fitz")
    pdf_path = tmp_path / "diag.pdf"
    _make_native_pdf(pdf_path)
    rows = dict(app.diagnose_rows(pdf_path))
    assert rows["Extensao"] == ".pdf"
    assert "Assinatura digital" in rows
    assert rows["Assinatura digital"] == "Nao detectada"
