"""Testes da ponte Python <-> interface HTML (extrator_api.Api).

O motor pesado (OCR, Whisper, pip) e substituido por funcoes falsas: os testes
verificam validacoes, eventos emitidos para a interface, cancelamento e a
instalacao automatica, sem processar documentos de verdade.

Executar com: venv\\Scripts\\python.exe -m pytest tests/test_api.py
"""
from __future__ import annotations

import re
import shutil
import subprocess
import sys
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

import extrator_core as core  # noqa: E402
import extrator_api as fa  # noqa: E402
import transcricao_core as transcricao  # noqa: E402


@pytest.fixture
def api(monkeypatch):
    monkeypatch.setattr(core, "CONFIG", dict(core.DEFAULT_CONFIG))
    monkeypatch.setattr(core, "save_config", lambda cfg: None)
    monkeypatch.setattr(core, "set_progress_queue", lambda q: None)
    monkeypatch.setattr(fa, "_signed", lambda path: "assinado" in path.name)
    return fa.Api()


def wait_job(api, thread_attr="_job_thread", timeout=10):
    thread = getattr(api, thread_attr)
    assert thread is not None, "nenhum trabalho foi iniciado"
    thread.join(timeout)
    assert not thread.is_alive(), "trabalho nao terminou a tempo"


def types(api) -> list[str]:
    """Tipos dos eventos pendentes, sem as linhas de log (que so documentam)."""
    return [event["type"] for event in api.drain_events() if event["type"] != "line"]


# ------------------------------------------------------------------ inicio


def test_bootstrap_traz_tudo_que_a_interface_precisa(api):
    boot = api.bootstrap()
    assert boot["ok"] is True
    assert boot["version"] == core.APP_VERSION
    assert {m["key"] for m in boot["convert_modes"]} >= {"auto", "force", "direct", "images"}
    assert boot["extensions"]["pdf"] == [".pdf"]
    assert ".mp3" in boot["extensions"]["media"]
    assert boot["transcricao"]["default_model"] == transcricao.MODELO_PADRAO
    assert boot["config"]["dpi"] == core.DEFAULT_CONFIG["dpi"]


def test_versao_vem_do_version_txt():
    texto = (ROOT / "VERSION.txt").read_text(encoding="utf-8")
    assert f"Versao {core.APP_VERSION}" in texto


def test_excecao_vira_erro_para_a_interface(api, monkeypatch):
    monkeypatch.setattr(core, "diagnose_rows", lambda p: (_ for _ in ()).throw(RuntimeError("quebrou")))
    result = api.diagnose("x.pdf")
    assert result == {"ok": False, "error": "quebrou"}


# ---------------------------------------------------------------- conversao


def test_converter_sem_arquivos_ou_modo_invalido(api):
    assert api.start_convert({"mode": "auto", "files": []})["ok"] is False
    assert api.start_convert({"mode": "nao_existe", "files": ["a.pdf"]})["ok"] is False


def test_converter_rejeita_arquivos_incompativeis_com_o_modo(api):
    result = api.start_convert({"mode": "auto", "files": ["a.pdf", "foto.png"]})
    assert result["ok"] is False
    assert "foto.png" in result["error"]
    ok_images = api.start_convert({"mode": "images", "files": ["a.pdf"]})
    assert ok_images["ok"] is False


def test_converter_exige_pasta_de_saida_quando_nao_e_a_de_origem(api):
    result = api.start_convert({"mode": "direct", "files": ["a.docx"], "same_folder": False})
    assert result["ok"] is False
    assert "saída" in result["error"]


def test_pdf_assinado_pede_confirmacao_e_depois_invalida(api, monkeypatch):
    chamadas = []
    monkeypatch.setattr(
        core, "searchable_pdf",
        lambda path, folder, mode, pages, invalidate, **kw: chamadas.append((path.name, invalidate)) or path,
    )
    params = {"mode": "searchable_skip", "files": ["contrato_assinado.pdf"], "same_folder": True}

    primeiro = api.start_convert(params)
    assert primeiro["status"] == "needs_confirm"
    assert primeiro["signed"] == ["contrato_assinado.pdf"]
    assert chamadas == []

    segundo = api.start_convert({**params, "confirm_signed": True})
    assert segundo["status"] == "started"
    wait_job(api)
    assert chamadas == [("contrato_assinado.pdf", True)]


def test_conversao_emite_eventos_e_conclui(api, monkeypatch):
    feitos = []
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda path, folder, mode, pages, fmt, **kw: feitos.append(path.name))
    result = api.start_convert({"mode": "auto", "files": ["a.pdf", "b.pdf"], "format": "md"})
    assert result["status"] == "started"
    wait_job(api)
    assert feitos == ["a.pdf", "b.pdf"]
    eventos = types(api)
    assert eventos.count("file_start") == 2
    assert eventos[-1] == "done"


def test_falha_em_um_arquivo_nao_derruba_os_demais(api, monkeypatch):
    def falso(path, *args, **kwargs):
        if path.name == "ruim.pdf":
            raise RuntimeError("PDF corrompido")

    monkeypatch.setattr(core, "ocr_pdf_to_text", falso)
    api.start_convert({"mode": "auto", "files": ["ruim.pdf", "bom.pdf"]})
    wait_job(api)
    eventos = api.drain_events()
    erro = next(e for e in eventos if e["type"] == "file_error")
    assert erro["name"] == "ruim.pdf" and "corrompido" in erro["text"]
    assert [e["type"] for e in eventos][-1] == "done"


def test_cancelar_gera_evento_cancelled(api, monkeypatch):
    liberado = threading.Event()

    def lento(path, *args, **kwargs):
        liberado.wait(5)
        raise core.OperationCancelled("cancelado")

    monkeypatch.setattr(core, "ocr_pdf_to_text", lento)
    api.start_convert({"mode": "auto", "files": ["a.pdf"]})
    assert api.cancel_job()["ok"] is True
    assert api._cancel.is_set()
    liberado.set()
    wait_job(api)
    assert "cancelled" in types(api)


def test_nao_inicia_segunda_operacao_enquanto_uma_roda(api, monkeypatch):
    liberado = threading.Event()
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda *a, **k: liberado.wait(5))
    assert api.start_convert({"mode": "auto", "files": ["a.pdf"]})["status"] == "started"
    segunda = api.start_convert({"mode": "auto", "files": ["b.pdf"]})
    assert segunda["ok"] is False
    liberado.set()
    wait_job(api)


# --------------------------------------------------------------------- lote


def _pasta_com_arquivos(tmp_path: Path) -> Path:
    for nome in ("a.pdf", "b.PDF", "c_OCR.pdf", "d.docx", "e.txt"):
        (tmp_path / nome).write_bytes(b"x")
    (tmp_path / "sub").mkdir()
    (tmp_path / "sub" / "f.pdf").write_bytes(b"x")
    return tmp_path


def test_scan_do_lote_ignora_saidas_e_respeita_subpastas(api, tmp_path):
    pasta = _pasta_com_arquivos(tmp_path)
    base = {"input_folder": str(pasta), "mode": "auto_md"}
    assert api.scan_batch({**base, "recursive": False})["count"] == 2  # a.pdf, b.PDF (c_OCR e saida)
    assert api.scan_batch({**base, "recursive": True})["count"] == 3
    assert api.scan_batch({**base, "mode": "direct"})["count"] == 4


def test_scan_do_lote_pasta_invalida(api):
    assert api.scan_batch({"input_folder": "", "mode": "auto_md"})["ok"] is False
    assert api.scan_batch({"input_folder": "Z:\\nao\\existe", "mode": "auto_md"})["ok"] is False


def test_lote_roda_cada_arquivo_e_conclui(api, monkeypatch, tmp_path):
    pasta = _pasta_com_arquivos(tmp_path)
    feitos = []
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda path, folder, mode, pages, fmt, **kw: feitos.append(path.name))
    result = api.start_batch({"input_folder": str(pasta), "mode": "auto_md"})
    assert result["status"] == "started" and result["count"] == 2
    wait_job(api)
    assert sorted(feitos) == ["a.pdf", "b.PDF"]
    assert types(api)[-1] == "done"


def test_lote_vazio_e_modo_invalido(api, tmp_path):
    assert api.start_batch({"input_folder": str(tmp_path), "mode": "auto_md"})["ok"] is False
    assert api.start_batch({"input_folder": str(tmp_path), "mode": "xyz"})["ok"] is False


# ----------------------------------------------------------- configuracoes


def test_salvar_configuracoes_valida_e_limita_valores(api):
    result = api.save_settings({
        "language": " por ", "dpi": "9999", "psm": "-4", "native_text_threshold": "80",
        "verbosity": "invalido", "cpu_mode": "maximo", "pdf_output_type": "pdfa",
        "rotate_pages": True, "deskew": False, "resume": True, "auto_open_output": False,
    })
    config = result["config"]
    assert config["language"] == "por"
    assert config["dpi"] == 600 and config["psm"] == 0
    assert config["verbosity"] == core.DEFAULT_CONFIG["verbosity"]  # valor invalido e ignorado
    assert config["cpu_mode"] == "maximo" and config["pdf_output_type"] == "pdfa"
    assert config["deskew"] is False


def test_salvar_configuracoes_recusa_idioma_vazio_e_numero_invalido(api):
    base = {"language": "por", "dpi": 220, "psm": 3, "native_text_threshold": 80}
    assert api.save_settings({**base, "language": "  "})["ok"] is False
    assert api.save_settings({**base, "dpi": "abc"})["ok"] is False


def test_restaurar_padroes(api):
    core.CONFIG["dpi"] = 111
    assert api.restore_settings()["config"]["dpi"] == core.DEFAULT_CONFIG["dpi"]


# --------------------------------------------------------------- transcricao


def test_transcricao_roda_fila_e_emite_progresso(api, monkeypatch, tmp_path):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {"ffmpeg": True})

    def falso(arquivo, progresso_cb, **kw):
        progresso_cb({"pct": 50, "processado_s": 5, "total_s": 10, "restante_s": 5, "elapsed_s": 2, "eta_s": 2})
        return {"output_path": tmp_path / (arquivo.stem + ".txt")}

    monkeypatch.setattr(transcricao, "transcrever_com_progresso", falso)
    result = api.start_transcricao({"items": [{"id": "t1", "path": str(tmp_path / "a.mp3")}], "beams": 99})
    assert result["status"] == "started"
    wait_job(api, "_tr_thread")
    eventos = [e for e in api.drain_events() if e["type"] != "line"]
    assert [e["type"] for e in eventos] == ["transc_item_start", "transc_progress", "transc_item_done", "transc_queue_done"]
    assert eventos[1]["id"] == "t1" and eventos[1]["pct"] == 50
    assert eventos[2]["output_path"].endswith("a.txt")


def test_transcricao_recusa_sem_dependencias_ou_sem_itens(api, monkeypatch):
    assert api.start_transcricao({"items": []})["ok"] is False
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {"ffmpeg": False, "faster-whisper": True})
    result = api.start_transcricao({"items": [{"id": "t1", "path": "a.mp3"}]})
    assert result["ok"] is False and "ffmpeg" in result["error"]


def test_transcricao_erro_em_um_item_continua_a_fila(api, monkeypatch):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {})

    def falso(arquivo, **kw):
        if arquivo.name == "ruim.mp3":
            raise RuntimeError("audio ilegivel")
        return {"output_path": Path("ok.txt")}

    monkeypatch.setattr(transcricao, "transcrever_com_progresso", falso)
    api.start_transcricao({"items": [{"id": "1", "path": "ruim.mp3"}, {"id": "2", "path": "bom.mp3"}]})
    wait_job(api, "_tr_thread")
    assert types(api) == [
        "transc_item_start", "transc_item_error", "transc_item_start", "transc_item_done", "transc_queue_done",
    ]


def test_pausar_e_cancelar_transcricao(api, monkeypatch):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {})
    liberado = threading.Event()
    monkeypatch.setattr(transcricao, "transcrever_com_progresso", lambda **kw: liberado.wait(5) or {"output_path": Path("x")})
    api.start_transcricao({"items": [{"id": "1", "path": "a.mp3"}]})
    assert api.toggle_pause_transcricao()["paused"] is True
    assert api._tr_pause.is_set()
    assert api.toggle_pause_transcricao()["paused"] is False
    api.cancel_transcricao()
    assert api._tr_cancel.is_set()
    liberado.set()
    wait_job(api, "_tr_thread")


def test_baixar_modelo_valida_nome_e_avisa_quando_termina(api, monkeypatch):
    baixados = []
    monkeypatch.setattr(transcricao, "baixar_modelo", lambda nome, **kw: baixados.append(nome))
    assert api.download_model("modelo-que-nao-existe")["ok"] is False
    assert api.download_model("tiny")["status"] == "started"
    api._download_thread.join(5)
    assert baixados == ["tiny"]
    assert api.drain_events() == [{"type": "model_download", "name": "tiny", "status": "done"}]


# ------------------------------------------------- instalacao automatica


def test_pip_requirements_so_cobrem_pacotes_pip_de_verdade():
    for nome in ("Python", "Tesseract", "Idiomas OCR", "Ghostscript (PDF/A)"):
        assert nome not in core.PIP_REQUIREMENTS
    assert "MarkItDown" in core.PIP_REQUIREMENTS
    assert "faster-whisper" in transcricao.PIP_REQUIREMENTS
    assert "ffmpeg (transcrição)" in transcricao.PIP_REQUIREMENTS


def test_verificar_instalacao_lista_o_que_falta(api, monkeypatch):
    monkeypatch.setattr(core, "installation_rows", lambda: [("Rich", False, "ausente"), ("Python", True, "3.13")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [("Modelo de transcrição", False, "nenhum")])
    result = api.check_installation()
    assert result["missing_pip"] == ["Rich"]
    assert result["missing_model"] is True
    # modelo ausente e AVISO (baixado no 1o uso): conta como ok=True
    assert [r["ok"] for r in result["rows"]] == [False, True, True]
    assert [r["estado"] for r in result["rows"]] == ["falha", "ok", "aviso"]
    assert result["resumo"] == "1 de 3 OK, 1 aviso, 1 falha"


def test_tudo_instalado_nao_instala_nada(api, monkeypatch):
    monkeypatch.setattr(core, "installation_rows", lambda: [("Rich", True, "ok")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [])
    assert api.install_missing()["status"] == "nothing"


def test_instala_dependencia_pip_ausente(api, monkeypatch):
    monkeypatch.setattr(core, "installation_rows", lambda: [("Rich", False, "ausente"), ("Python", True, "3.13")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [])
    chamadas = []

    def fake_run(cmd, **kwargs):
        chamadas.append(cmd)
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

    monkeypatch.setattr(fa.subprocess, "run", fake_run)
    assert api.install_missing()["status"] == "started"
    api._install_thread.join(5)
    assert len(chamadas) == 1
    assert chamadas[0][:3] == [sys.executable, "-m", "pip"]
    assert "rich" in chamadas[0][-1].lower()
    assert types(api)[-1] == "install_fix_done"


def test_modelo_ausente_dispara_download(api, monkeypatch):
    monkeypatch.setattr(core, "installation_rows", lambda: [("Python", True, "3.13")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [("Modelo de transcrição", False, "nenhum")])
    baixados = []
    monkeypatch.setattr(transcricao, "baixar_modelo", lambda nome, **kw: baixados.append(nome))
    api.install_missing()
    api._install_thread.join(5)
    assert baixados == [transcricao.MODELO_PADRAO]


def test_falha_no_pip_e_registrada_no_log(api, monkeypatch):
    monkeypatch.setattr(core, "installation_rows", lambda: [("Rich", False, "ausente")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [])

    def falha(cmd, **kwargs):
        raise subprocess.CalledProcessError(1, cmd, stderr="erro simulado de instalacao")

    monkeypatch.setattr(fa.subprocess, "run", falha)
    api.install_missing()
    api._install_thread.join(5)
    linhas = [e.get("text", "") for e in api.drain_events()]
    assert any("Falha ao instalar Rich" in linha for linha in linhas)


def test_api_nao_expoe_estado_interno_ao_javascript():
    """Todo atributo publico da Api vira metodo do motor; estado interno comeca com '_'."""
    instancia = fa.Api()
    publicos = [n for n in vars(instancia) if not n.startswith("_")]
    assert publicos == []
