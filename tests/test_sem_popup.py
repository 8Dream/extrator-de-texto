"""Testes anti pop-up: nada pode abrir janela fora da janela principal nem
interromper o usuario com caixa de dialogo; subprocessos nao podem piscar
console; a verificacao de instalacao precisa dar retorno visivel.

Executar com: venv\\Scripts\\python.exe -m pytest tests/test_sem_popup.py
"""
from __future__ import annotations

import ast
import re
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

import extrator_core as core  # noqa: E402
import extrator_api as fa  # noqa: E402
import semjanela  # noqa: E402
import transcricao_core as transcricao  # noqa: E402


# ------------------------------------------------------------------ nada de janela

UI_SRC = ROOT / "ui" / "src"
PROIBIDOS_PY = ("tkinter", "messagebox", "MessageBox", "create_confirmation_dialog", "create_alert",
                "create_file_dialog", "import webview", "pywebview")


def test_codigo_python_sem_caixa_de_dialogo_nem_webview():
    for nome in SRC.glob("*.py"):
        texto = nome.read_text(encoding="utf-8")
        for proibido in PROIBIDOS_PY:
            if nome.name == "semjanela.py" and proibido in ("MessageBox", "messagebox"):
                continue
            if nome.name == "extrator_core.py" and proibido == "tkinter":
                continue  # so o modo terminal (CLI) usa seletores tkinter; o motor e a Api nunca os chamam
            assert proibido not in texto, f"{nome.name}: {proibido!r}"


def test_nao_ha_pagina_web_antiga():
    assert not (SRC / "web").exists()
    assert not (SRC / "extrator.py").exists()


def test_interface_nativa_sem_message_box():
    if not UI_SRC.is_dir():
        pytest.skip("interface Rust ausente")
    for arquivo in UI_SRC.rglob("*.rs"):
        texto = arquivo.read_text(encoding="utf-8", errors="replace")
        assert "MessageBox" not in texto and "MessageDialog" not in texto, arquivo.name


def test_motor_instala_o_patch_antes_dos_outros_imports():
    linhas = (SRC / "motor.py").read_text(encoding="utf-8").splitlines()
    imports = [i for i, l in enumerate(linhas) if re.match(r"(import|from) ", l) and "__future__" not in l]
    assert linhas[imports[0]].strip() == "import semjanela"
    chamada = next(i for i, l in enumerate(linhas) if l.strip() == "semjanela.instalar()")
    assert chamada < imports[1]


def test_todo_subprocess_do_codigo_pede_sem_janela():
    """Alem do patch global, cada chamada explicita ja pede CREATE_NO_WINDOW."""
    problemas = []
    for nome in SRC.glob("*.py"):
        if nome.name == "semjanela.py":
            continue
        arvore = ast.parse(nome.read_text(encoding="utf-8"))
        for no in ast.walk(arvore):
            if not isinstance(no, ast.Call):
                continue
            texto = ast.unparse(no.func)
            if texto in {"subprocess.run", "subprocess.Popen", "subprocess.check_output", "subprocess.call"}:
                if not any(k.arg == "creationflags" for k in no.keywords):
                    problemas.append(f"{nome.name}:{no.lineno} {texto}")
    # unicas excecoes: abrir pasta em macOS/Linux (nunca roda no Windows)
    problemas = [p for p in problemas if not p.startswith("extrator_core.py:3")]
    assert not problemas, problemas


# ----------------------------------------------------- patch CREATE_NO_WINDOW


@pytest.fixture
def popen_gravador():
    """Troca o __init__ do Popen por um gravador e restaura no fim."""
    semjanela.desinstalar()
    original = subprocess.Popen.__init__
    chamadas = []

    def gravador(self, *args, **kwargs):
        chamadas.append(kwargs)

    subprocess.Popen.__init__ = gravador
    yield chamadas
    subprocess.Popen.__init__ = original
    semjanela.desinstalar()


def test_patch_acrescenta_create_no_window(popen_gravador):
    assert semjanela.instalar(force=True)
    subprocess.Popen(["x"])
    assert popen_gravador[-1]["creationflags"] & semjanela._CREATE_NO_WINDOW


def test_patch_preserva_outros_flags(popen_gravador):
    semjanela.instalar(force=True)
    extra = 0x00000200  # CREATE_NEW_PROCESS_GROUP
    subprocess.Popen(["x"], creationflags=extra)
    flags = popen_gravador[-1]["creationflags"]
    assert flags & extra and flags & semjanela._CREATE_NO_WINDOW


@pytest.mark.parametrize("flag", [0x00000010, 0x00000008, 0x08000000])
def test_patch_respeita_pedido_explicito_de_console(popen_gravador, flag):
    semjanela.instalar(force=True)
    subprocess.Popen(["x"], creationflags=flag)
    assert popen_gravador[-1]["creationflags"] == flag


def test_patch_e_idempotente(popen_gravador):
    semjanela.instalar(force=True)
    primeiro = subprocess.Popen.__init__
    semjanela.instalar(force=True)
    assert subprocess.Popen.__init__ is primeiro
    subprocess.Popen(["x"])
    assert len(popen_gravador) == 1


@pytest.mark.skipif(not hasattr(subprocess, "STARTUPINFO"), reason="so Windows")
def test_patch_esconde_a_janela_com_startupinfo(popen_gravador):
    semjanela.instalar(force=True)
    subprocess.Popen(["x"])
    info = popen_gravador[-1]["startupinfo"]
    assert info.dwFlags & subprocess.STARTF_USESHOWWINDOW and info.wShowWindow == subprocess.SW_HIDE


@pytest.mark.skipif(sys.platform != "win32", reason="so Windows")
def test_patch_real_roda_processo_sem_quebrar():
    semjanela.instalar()
    try:
        out = subprocess.run([sys.executable, "-c", "print('ok')"], capture_output=True, text=True)
        assert out.stdout.strip() == "ok"
    finally:
        semjanela.desinstalar()


# ------------------------------------------- verificacao de instalacao (API)


@pytest.fixture
def api(monkeypatch):
    monkeypatch.setattr(core, "CONFIG", dict(core.DEFAULT_CONFIG))
    return fa.Api()


def test_verificacao_roda_em_thread_e_emite_linhas_e_resultado(api, monkeypatch):
    monkeypatch.setattr(core, "iter_installation_rows", lambda: iter([("Python", True, "3.14"), ("Rich", False, "ausente")]))
    monkeypatch.setattr(transcricao, "iter_installation_rows", lambda: iter([("Modelo de transcrição", False, "nenhum")]))
    assert api.start_check_installation() == {"ok": True, "status": "started"}
    api._check_thread.join(5)
    assert not api._check_thread.is_alive()
    eventos = api.drain_events()
    assert [e["type"] for e in eventos] == ["check_start", "check_row", "check_row", "check_row", "check_done"]
    assert [e["name"] for e in eventos if e["type"] == "check_row"] == ["Python", "Rich", "Modelo de transcrição"]
    final = eventos[-1]
    assert [r["ok"] for r in final["rows"]] == [True, False, True]  # o modelo ausente e aviso
    assert [r["estado"] for r in final["rows"]] == ["ok", "falha", "aviso"]
    assert final["rows"][0] == {"name": "Python", "ok": True, "estado": "ok", "detail": "3.14"}
    assert final["missing_pip"] == ["Rich"] and final["missing_model"] is True


def test_verificacao_nao_inicia_duas_vezes(api, monkeypatch):
    import threading

    solto = threading.Event()

    def lento():
        solto.wait(5)
        return iter([])

    monkeypatch.setattr(core, "iter_installation_rows", lento)
    monkeypatch.setattr(transcricao, "iter_installation_rows", lambda: iter([]))
    assert api.start_check_installation()["ok"] is True
    assert api.start_check_installation()["ok"] is False
    solto.set()
    api._check_thread.join(5)


def test_erro_na_verificacao_vira_evento_para_a_tela(api, monkeypatch):
    def quebra():
        raise RuntimeError("tesseract travou")
        yield  # pragma: no cover

    monkeypatch.setattr(core, "iter_installation_rows", quebra)
    api.start_check_installation()
    api._check_thread.join(5)
    ultimo = api.drain_events()[-1]
    assert ultimo["type"] == "check_error" and "tesseract travou" in ultimo["text"]


def test_verificacao_real_devolve_os_componentes_esperados(api):
    """Roda a verificacao de verdade (sem mocks) e confere os nomes pedidos."""
    api.start_check_installation()
    api._check_thread.join(120)
    assert not api._check_thread.is_alive()
    final = api.drain_events()[-1]
    assert final["type"] == "check_done", final
    nomes = {r["name"] for r in final["rows"]}
    esperados = {"Python", "MarkItDown", "PyMuPDF", "pytesseract", "Pillow", "OCRmyPDF", "Rich",
                 "Tesseract", "Idiomas OCR", "Ghostscript (PDF/A)"}
    assert esperados <= nomes
    assert "pywebview" not in nomes
    assert all(set(r) == {"name", "ok", "estado", "detail"} for r in final["rows"])


def test_motor_e_api_nao_importam_tkinter():
    for nome in ("motor.py", "extrator_api.py", "transcricao_core.py", "execucoes.py"):
        assert "tkinter" not in (SRC / nome).read_text(encoding="utf-8"), nome
