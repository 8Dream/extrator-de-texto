"""O download do ffmpeg estatico (static-ffmpeg, ~72 MB) e uma execucao visivel na
Central de execucoes: etapa "Baixando ffmpeg" e progresso em bytes quando o
tamanho e conhecido. Aqui o pacote e a rede sao simulados."""
from __future__ import annotations

import sys
import types

import pytest

import transcricao_core as tc


class _Resposta:
    def __init__(self, tamanho, cabecalho=True, blocos=4):
        self.headers = {"content-length": str(tamanho)} if cabecalho else {}
        self._tamanho, self._blocos = tamanho, blocos

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def raise_for_status(self):
        pass

    def iter_content(self, _n):
        for _ in range(self._blocos):
            yield b"x" * (self._tamanho // self._blocos)


@pytest.fixture
def falso_ffmpeg(monkeypatch, tmp_path):
    import requests

    pasta = tmp_path / "bin"
    run = types.ModuleType("static_ffmpeg.run")
    run.get_platform_dir = lambda: str(pasta)

    def original(*a):
        raise AssertionError("o original nao deve rodar")

    run.download_file = original
    pacote = types.ModuleType("static_ffmpeg")
    pacote.run = run
    estado = {"chamadas": 0}

    def add_paths():
        estado["chamadas"] += 1
        run.download_file("http://exemplo/ffmpeg.zip", str(tmp_path / "ffmpeg.zip"))
        pasta.mkdir(exist_ok=True)
        (pasta / "installed.crumb").write_text("ok")

    pacote.add_paths = add_paths
    monkeypatch.setitem(sys.modules, "static_ffmpeg", pacote)
    monkeypatch.setitem(sys.modules, "static_ffmpeg.run", run)
    monkeypatch.setattr(tc, "_ffmpeg_paths_added", False)
    monkeypatch.setattr(tc, "_ffmpeg_falha_em", None)
    estado.update(requests=requests, pasta=pasta, run=run, original=original, tmp=tmp_path, pacote=pacote)
    return estado


def _execucoes_ffmpeg(registro):
    return [e for e in registro.snapshot()["execucoes"] if e["rotulo"] == "Baixando ffmpeg"]


def test_download_vira_execucao_com_progresso_em_bytes(falso_ffmpeg, monkeypatch, _registro_isolado):
    monkeypatch.setattr(falso_ffmpeg["requests"], "get", lambda *a, **k: _Resposta(2 * 1048576))
    tc._garantir_ffmpeg_no_path()
    (ex,) = _execucoes_ffmpeg(_registro_isolado)
    assert ex["estado"] == "concluida" and ex["ferramenta"] == "instalacao"
    assert ex["determinada"] is True and ex["progresso"] == 100.0
    assert ex["progresso_texto"] == "2,0 MB de 2,0 MB"
    detalhe = _registro_isolado.detalhe(ex["id"])
    assert any("Baixando" in l["x"] for l in detalhe["linhas"])
    assert falso_ffmpeg["run"].download_file is falso_ffmpeg["original"]  # o original foi restaurado
    assert (falso_ffmpeg["tmp"] / "ffmpeg.zip").exists()


def test_sem_tamanho_so_mostra_a_etapa(falso_ffmpeg, monkeypatch, _registro_isolado):
    monkeypatch.setattr(falso_ffmpeg["requests"], "get", lambda *a, **k: _Resposta(1048576, cabecalho=False))
    tc._garantir_ffmpeg_no_path()
    (ex,) = _execucoes_ffmpeg(_registro_isolado)
    assert ex["estado"] == "concluida" and ex["progresso"] in (None, 100.0)


def test_ja_baixado_nao_cria_execucao(falso_ffmpeg, _registro_isolado):
    falso_ffmpeg["pasta"].mkdir()
    (falso_ffmpeg["pasta"] / "installed.crumb").write_text("ok")
    chamadas = []
    falso_ffmpeg["pacote"].add_paths = lambda: chamadas.append(1)
    tc._garantir_ffmpeg_no_path()
    assert chamadas == [1] and _execucoes_ffmpeg(_registro_isolado) == []


def test_falha_aparece_como_falha_e_so_tenta_de_novo_depois_do_intervalo(falso_ffmpeg, monkeypatch, _registro_isolado):
    def sem_internet(*a, **k):
        raise OSError("sem internet")

    monkeypatch.setattr(falso_ffmpeg["requests"], "get", sem_internet)
    tc._garantir_ffmpeg_no_path()
    (ex,) = _execucoes_ffmpeg(_registro_isolado)
    assert ex["estado"] == "falhou" and "sem internet" in ex["erro"]
    assert not (falso_ffmpeg["tmp"] / "ffmpeg.zip").exists()
    tc._garantir_ffmpeg_no_path()  # dentro do intervalo: nao tenta de novo
    assert falso_ffmpeg["chamadas"] == 1
    monkeypatch.setattr(tc, "_ffmpeg_falha_em", tc.time.monotonic() - tc.FFMPEG_NOVA_TENTATIVA_S - 1)
    monkeypatch.setattr(falso_ffmpeg["requests"], "get", lambda *a, **k: _Resposta(1048576))
    tc._garantir_ffmpeg_no_path()
    assert falso_ffmpeg["chamadas"] == 2 and tc._ffmpeg_paths_added is True


def test_download_pode_ser_cancelado(falso_ffmpeg, monkeypatch, _registro_isolado):
    class Cancelavel(_Resposta):
        def iter_content(self, n):
            for bloco in super().iter_content(n):
                _registro_isolado.cancelar(_execucoes_ffmpeg(_registro_isolado)[0]["id"])
                yield bloco

    monkeypatch.setattr(falso_ffmpeg["requests"], "get", lambda *a, **k: Cancelavel(1048576))
    tc._garantir_ffmpeg_no_path()
    (ex,) = _execucoes_ffmpeg(_registro_isolado)
    assert ex["estado"] == "cancelada"
