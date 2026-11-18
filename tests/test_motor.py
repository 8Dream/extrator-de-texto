"""Testes do motor (src/motor.py): o processo que a interface nativa em Rust usa.

Cada teste inicia o motor de verdade, como subprocesso, falando o protocolo de
docs/PROTOCOLO-MOTOR.md (uma linha JSON por mensagem em stdin/stdout). O motor
roda numa COPIA temporaria de src/: assim a pasta dados/ (logs, historico,
configuracao) nasce no temporario e o repositorio nao e sujo.

Sem esperas fixas: toda espera e por evento (condicao + teto so para dar erro claro).
"""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
TETO_S = 120  # so evita travar a suite se algo quebrar


class Motor:
    def __init__(self, pasta: Path) -> None:
        src = pasta / "src"
        src.mkdir(parents=True)
        for arquivo in (ROOT / "src").glob("*.py"):
            shutil.copy2(arquivo, src / arquivo.name)
        shutil.copy2(ROOT / "VERSION.txt", pasta / "VERSION.txt")
        self.pasta = pasta
        self.proc = subprocess.Popen(
            [sys.executable, "-X", "utf8", "-u", str(src / "motor.py")],
            cwd=str(pasta), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        self.linhas: list[bytes] = []   # tudo que saiu em stdout, na ordem
        self.ordem: list = []           # ids na ordem de chegada
        self._respostas: dict = {}
        self._cond = threading.Condition()
        self._proximo = 0
        self._fim = threading.Event()
        threading.Thread(target=self._ler, daemon=True).start()

    def _ler(self) -> None:
        for bruta in self.proc.stdout:
            with self._cond:
                self.linhas.append(bruta)
                try:
                    quadro = json.loads(bruta.decode("utf-8"))
                except ValueError:
                    continue
                self.ordem.append(quadro.get("id"))
                self._respostas.setdefault(quadro.get("id"), []).append(quadro)
                self._cond.notify_all()
        with self._cond:
            self._fim.set()
            self._cond.notify_all()

    def enviar_bruto(self, texto: str) -> None:
        self.proc.stdin.write(texto.encode("utf-8"))
        self.proc.stdin.flush()

    def enviar(self, metodo: str, *args, rid=None) -> int:
        self._proximo += 1
        rid = rid if rid is not None else self._proximo
        self.enviar_bruto(json.dumps({"id": rid, "m": metodo, "a": list(args)}, ensure_ascii=False) + "\n")
        return rid

    def esperar(self, rid, teto: float = TETO_S) -> dict:
        fim = time.monotonic() + teto
        with self._cond:
            while rid not in self._respostas:
                if self._fim.is_set():
                    pytest.fail(f"o motor saiu sem responder {rid}")
                restante = fim - time.monotonic()
                if restante <= 0:
                    pytest.fail(f"sem resposta para {rid}")
                self._cond.wait(min(restante, 0.5))
            return self._respostas[rid][0]

    def chamar(self, metodo: str, *args) -> dict:
        return self.esperar(self.enviar(metodo, *args))

    def r(self, metodo: str, *args):
        quadro = self.chamar(metodo, *args)
        assert quadro["ok"] is True, quadro
        return quadro["r"]


@pytest.fixture
def motor(tmp_path):
    m = Motor(tmp_path / "app")
    yield m
    if m.proc.poll() is None:
        try:
            m.proc.stdin.close()
        except OSError:
            pass
        try:
            m.proc.wait(30)
        except subprocess.TimeoutExpired:
            m.proc.kill()
    # stdout e so do protocolo: toda linha e um objeto JSON com "id" e "ok"
    for bruta in m.linhas:
        quadro = json.loads(bruta.decode("utf-8"))
        assert isinstance(quadro, dict) and "id" in quadro and isinstance(quadro["ok"], bool), bruta
        assert not bruta.endswith(b"\r\n")


def test_ping(motor):
    r = motor.r("ping")
    assert set(r) == {"versao", "python", "pid"}
    # no Windows o python.exe do venv e um redirecionador: o pid real e o do filho dele
    assert isinstance(r["pid"], int) and r["pid"] > 0
    esperado = (ROOT / "VERSION.txt").read_text(encoding="utf-8").split("Versao")[-1].strip()
    assert r["versao"] == esperado


def test_capacidades(motor):
    assert motor.r("capacidades") == {"interlocutores": False, "motor_spark": False, "nativo": True}


def test_metodo_inexistente(motor):
    quadro = motor.chamar("nao_existe")
    assert quadro["ok"] is False and "inexistente" in quadro["e"]
    assert motor.r("ping")["pid"] > 0  # continua vivo


def test_nomes_privados_nao_sao_metodos(motor):
    for nome in ("_emit", "__init__", "_reg", "drain_events_x"):
        assert motor.chamar(nome)["ok"] is False


def test_json_invalido_nao_derruba_o_motor(motor):
    motor.enviar_bruto("{isso nao e json\n")
    motor.enviar_bruto("[1, 2]\n")
    motor.enviar_bruto("\n")  # linha em branco e ignorada
    motor.enviar_bruto('{"id": 7}\n')  # sem "m"
    erro = motor.esperar(None)
    assert erro["ok"] is False and "JSON inválido" in erro["e"]
    assert motor.esperar(7)["ok"] is False
    assert motor.r("ping")["pid"] > 0
    assert motor.ordem.count(None) == 2  # JSON quebrado e lista; a linha em branco nao responde


def test_argumentos_invalidos_viram_erro(motor):
    quadro = motor.chamar("exec_detalhe")  # falta o id
    assert quadro["ok"] is False and "exec_detalhe" in quadro["e"]
    assert motor.chamar("ping", 1, 2)["ok"] is False
    assert motor.chamar("is_busy")["ok"] is True  # continua respondendo


def test_metodos_que_dependem_da_janela_respondem_erro_sem_abrir_nada(motor):
    for nome in ("pick_files", "pick_folder", "attach_window", "notify_close_blocked"):
        quadro = motor.chamar(nome)
        assert quadro == {"id": quadro["id"], "ok": False, "e": "use o seletor nativo da interface"}


def test_bootstrap_e_metodos_basicos(motor):
    boot = motor.r("bootstrap")
    assert boot["ok"] and boot["convert_modes"] and boot["batch_modes"]
    assert motor.r("is_busy") == {"ocr": False, "transcricao": False}
    assert motor.r("drain_events") == []
    assert motor.r("exec_listar")["ok"] is True
    assert motor.r("log_arquivos")["ok"] is True


def test_concorrencia_chamada_curta_nao_espera_a_longa(motor):
    longa = motor.enviar("check_installation")
    curta = motor.enviar("ping")
    assert motor.esperar(curta)["ok"] is True
    assert motor.esperar(longa)["ok"] is True
    assert motor.ordem.index(curta) < motor.ordem.index(longa)


def test_duas_chamadas_ao_mesmo_tempo_recebem_cada_uma_a_sua_resposta(motor):
    a = motor.enviar("check_installation")
    b = motor.enviar("check_installation")
    ra, rb = motor.esperar(a), motor.esperar(b)
    assert ra["ok"] and rb["ok"]
    assert ra["r"]["total"] == rb["r"]["total"] > 0


def test_check_installation_tem_estados(motor):
    r = motor.r("check_installation")
    assert r["ok"] is True
    estados = {"ok", "aviso", "falha"}
    for linha in r["rows"]:
        assert set(linha) == {"name", "ok", "estado", "detail"}
        assert linha["estado"] in estados
        assert linha["ok"] is (linha["estado"] != "falha")
    # contagens e texto coerentes
    assert r["n_ok"] + r["n_aviso"] + r["n_falha"] == r["total"] == len(r["rows"])
    assert r["resumo"].startswith(f"{r['n_ok']} de {r['total']} OK")
    # sem modelo baixado (pasta dados nova): AVISO, nunca falha
    modelo = next(l for l in r["rows"] if l["name"] == "Modelo de transcrição")
    assert modelo["estado"] == "aviso" and modelo["ok"] is True and "1º uso" in modelo["detail"]
    assert r["missing_model"] is True and "Modelo de transcrição" not in r["missing_pip"]
    # nada foi gravado como ERRO no log por causa do modelo
    for log in (motor.pasta / "dados" / "logs").glob("extrator_*.log"):
        assert not [l for l in log.read_text(encoding="utf-8").splitlines() if "| ERROR |" in l and "odelo" in l]


def _esperar_evento(motor, tipos: set, teto: float = TETO_S) -> list:
    """Consulta drain_events em laco curto ate aparecer um evento de `tipos`."""
    fim = time.monotonic() + teto
    vistos: list = []
    while time.monotonic() < fim:
        vistos += motor.r("drain_events")
        if any(e.get("type") in tipos for e in vistos):
            return vistos
        time.sleep(0.05)
    pytest.fail(f"nenhum evento {tipos}; vistos: {vistos}")


def test_conversao_direta_de_ponta_a_ponta(motor, tmp_path):
    entrada = tmp_path / "doc.html"
    entrada.write_text("<html><body><h1>Titulo de teste</h1><p>Primeiro paragrafo.</p></body></html>",
                       encoding="utf-8")
    inicio = motor.r("start_convert", {"mode": "direct", "files": [str(entrada)], "same_folder": True})
    assert inicio == {"ok": True, "status": "started"}
    eventos = _esperar_evento(motor, {"done", "error", "cancelled"})
    tipos = [e["type"] for e in eventos]
    assert "file_start" in tipos and tipos[-1] == "done", eventos

    saida = next(tmp_path.glob("doc*.md"))
    assert "Titulo de teste" in saida.read_text(encoding="utf-8")

    lista = motor.r("exec_listar")
    ex = next(e for e in lista["execucoes"] if e["ferramenta"] == "converter")
    assert ex["estado"] == "concluida"
    detalhe = motor.r("exec_detalhe", ex["id"], 0)
    assert detalhe["ok"] and detalhe["linhas"] and detalhe["execucao"]["id"] == ex["id"]
    assert motor.r("is_busy")["ocr"] is False


def test_encerrar_responde_e_sai(motor):
    quadro = motor.chamar("encerrar")
    assert quadro["ok"] is True
    assert motor.proc.wait(15) == 0


def test_encerrar_cancela_o_que_esta_rodando_e_sai(motor):
    pasta = motor.pasta / "lote"
    pasta.mkdir()
    for i in range(30):
        (pasta / f"d{i}.html").write_text(f"<p>texto {i}</p>", encoding="utf-8")
    assert motor.r("start_batch", {"mode": "direct", "input_folder": str(pasta)})["ok"] is True
    assert motor.chamar("encerrar")["ok"] is True
    assert motor.proc.wait(15) == 0


def test_fechar_o_stdin_encerra_o_motor(motor):
    assert motor.r("ping")
    motor.proc.stdin.close()
    assert motor.proc.wait(15) == 0


def test_saida_de_diagnostico_nao_vai_para_stdout(motor):
    """Qualquer print de biblioteca cai em dados/logs/motor-saida.out, nao no protocolo."""
    assert motor.r("ping")
    motor.proc.stdin.close()
    motor.proc.wait(15)
    for bruta in motor.linhas:
        json.loads(bruta.decode("utf-8"))
