"""Configuracao comum dos testes.

O registro central de execucoes grava o historico em dados/. Para que a suite
nunca suje a pasta de dados real, cada teste usa um Registro isolado numa
pasta temporaria (a Api le `execucoes.REGISTRO` ao ser criada).
"""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

import execucoes  # noqa: E402


@pytest.fixture(autouse=True)
def _registro_isolado(tmp_path, monkeypatch):
    registro = execucoes.Registro(tmp_path / "dados_execucoes")
    monkeypatch.setattr(execucoes, "REGISTRO", registro)
    return registro
