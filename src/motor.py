#!/usr/bin/env python3
"""
Extrator de Texto - motor sem janela, para a interface nativa (Rust).

A interface inicia este programa como processo-filho oculto:

    venv\\Scripts\\python.exe -X utf8 -u src\\motor.py

e conversa com ele por stdin/stdout, uma linha JSON (UTF-8) por mensagem.
O protocolo completo, com exemplos, esta em docs\\PROTOCOLO-MOTOR.md. Resumo:

    -> {"id": 12, "m": "start_convert", "a": [ {...} ]}
    <- {"id": 12, "ok": true,  "r": <resultado da Api>}
    <- {"id": 12, "ok": false, "e": "mensagem em portugues"}

Nao ha mensagens espontaneas: a interface consulta `drain_events`, `exec_listar`
etc. em intervalos curtos.

Regras deste arquivo:
  * stdout e SO do protocolo. Ao iniciar, o descritor 1 original e separado para
    o protocolo e os descritores 1 e 2 passam a apontar para
    dados\\logs\\motor-saida.out (print, avisos de bibliotecas, filhos do OCR...).
  * Cada requisicao roda em thread propria (verificacoes e conversoes longas nao
    bloqueiam as outras).
  * Se o stdin fechar (o processo pai morreu) ou chegar `encerrar`, tudo e
    cancelado e o processo sai. No Windows ele entra num Job Object que mata os
    processos-filho (tesseract, ffmpeg, workers do OCR) quando o motor sai: nada
    fica orfao.
  * Nenhuma janela, caixa de dialogo ou console.

Tudo que cria algo fica dentro de main(): o OCR usa multiprocessing e, no
Windows, cada processo-filho reimporta este script.
"""
from __future__ import annotations

import semjanela

# Antes de qualquer outro import: nenhum subprocesso (tesseract, gs, ffmpeg...)
# pode abrir janela de console. Fica fora do main() de proposito: os processos-
# filho do OCR (multiprocessing, spawn) reimportam este arquivo.
semjanela.instalar()

import json  # noqa: E402
import logging  # noqa: E402
import os  # noqa: E402
import sys  # noqa: E402
import threading  # noqa: E402
import time  # noqa: E402
from pathlib import Path  # noqa: E402

# Metodos da Api expostos (mesmos nomes e argumentos da Api atual).
METODOS_API = (
    "bootstrap", "drain_events", "is_busy", "request_stop_all", "open_path", "open_data_folder",
    "start_convert", "cancel_job", "scan_batch", "start_batch",
    "transcricao_state", "transcricao_deps", "start_transcricao", "toggle_pause_transcricao",
    "cancel_transcricao", "download_model", "diagnose", "save_settings", "restore_settings",
    "check_installation", "start_check_installation", "install_missing", "clear_data",
    "exec_listar", "exec_detalhe", "exec_cancelar", "exec_repetir", "exec_resumo", "exec_limpar_concluidas",
    "log_arquivos", "log_ler", "log_buscar", "log_registrar", "log_abrir_pasta",
)
# Eram da janela antiga: a interface nativa usa dialogos proprios.
METODOS_SO_NATIVOS = ("pick_files", "pick_folder", "attach_window", "notify_close_blocked")
MSG_SO_NATIVO = "use o seletor nativo da interface"

# Flags de recursos escondidos ate ficarem prontos (v0.1.2: interlocutores e motor Spark).
CAPACIDADES = {"interlocutores": False, "motor_spark": False, "nativo": True}

LIMITE_ENCERRAR_S = 3.0  # teto (nunca espera fixa): so para nao travar se algo nao cancelar
LIMITE_SAIDA_BYTES = 2_000_000

_proto = None  # arquivo binario do protocolo (descritor 1 original)
_lock_saida = threading.Lock()
_encerrando = threading.Event()
_api = None
_job = None  # mantem vivo o handle do Job Object


# ----------------------------------------------------------------- infraestrutura


def _preparar_stdio() -> None:
    """Separa o stdout original para o protocolo e manda 1 e 2 para um arquivo."""
    global _proto
    for fluxo in (sys.stdout, sys.stderr):
        try:
            fluxo.flush()
        except Exception:
            pass
    _proto = os.fdopen(os.dup(1), "wb", buffering=0)
    pasta = Path(__file__).resolve().parent.parent / "dados" / "logs"
    try:
        pasta.mkdir(parents=True, exist_ok=True)
        alvo = pasta / "motor-saida.out"
        modo = os.O_WRONLY | os.O_CREAT | (os.O_TRUNC if alvo.exists() and alvo.stat().st_size > LIMITE_SAIDA_BYTES
                                           else os.O_APPEND)
        fd = os.open(str(alvo), modo | getattr(os, "O_BINARY", 0))
    except OSError:
        fd = os.open(os.devnull, os.O_WRONLY)
    os.dup2(fd, 1)
    os.dup2(fd, 2)
    os.close(fd)


def _job_object_mata_filhos() -> None:
    """Windows: o motor entra num Job Object que encerra todos os processos-filho
    quando o motor termina (por qualquer motivo, inclusive ser morto)."""
    global _job
    if os.name != "nt":
        return
    try:
        import ctypes
        from ctypes import wintypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)

        class _Basico(ctypes.Structure):
            _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                        ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                        ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                        ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                        ("SchedulingClass", wintypes.DWORD)]

        class _IO(ctypes.Structure):
            _fields_ = [(n, ctypes.c_uint64) for n in (
                "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

        class _Estendido(ctypes.Structure):
            _fields_ = [("Basic", _Basico), ("Io", _IO), ("ProcessMemoryLimit", ctypes.c_size_t),
                        ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t),
                        ("PeakJobMemoryUsed", ctypes.c_size_t)]

        k32.CreateJobObjectW.restype = wintypes.HANDLE
        k32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
        k32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD]
        k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        k32.GetCurrentProcess.restype = wintypes.HANDLE
        job = k32.CreateJobObjectW(None, None)
        if not job:
            return
        info = _Estendido()
        info.Basic.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not k32.SetInformationJobObject(job, 9, ctypes.byref(info), ctypes.sizeof(info)):
            return
        if k32.AssignProcessToJobObject(job, k32.GetCurrentProcess()):
            _job = job
    except Exception:
        logging.debug("Job Object indisponivel", exc_info=True)


class _EntradaPipeWindows:
    """Leitor de linhas do stdin SEM leitura bloqueante.

    No Windows, uma leitura sincrona pendente no pipe do stdin trava qualquer outra
    thread que, ao carregar uma DLL nativa (numpy, por exemplo), consulte aquele
    handle: o `import` ficava parado para sempre enquanto o motor esperava a
    proxima requisicao. Por isso o pipe e consultado com PeekNamedPipe (em laco de
    5 ms) e so se le quando ha dados; pipe fechado pelo pai = fim da entrada."""

    def __init__(self) -> None:
        import ctypes
        import msvcrt
        from ctypes import wintypes

        self._ct = ctypes
        self._dword = wintypes.DWORD
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.PeekNamedPipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD, wintypes.LPVOID,
                                      ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
        k32.GetFileType.argtypes = [wintypes.HANDLE]
        self._k32 = k32
        self._handle = msvcrt.get_osfhandle(0)
        self._buf = b""

    def e_pipe(self) -> bool:
        return self._k32.GetFileType(self._handle) == 3  # FILE_TYPE_PIPE

    def readline(self) -> bytes:
        while b"\n" not in self._buf:
            disponivel = self._dword(0)
            if not self._k32.PeekNamedPipe(self._handle, None, 0, None, self._ct.byref(disponivel), None):
                resto, self._buf = self._buf, b""  # pipe quebrado/fechado
                return resto
            if disponivel.value:
                dados = os.read(0, disponivel.value)
                if not dados:
                    resto, self._buf = self._buf, b""
                    return resto
                self._buf += dados
            else:
                time.sleep(0.005)
        linha, _, self._buf = self._buf.partition(b"\n")
        return linha + b"\n"


def _abrir_entrada():
    if os.name == "nt":
        try:
            entrada = _EntradaPipeWindows()
            if entrada.e_pipe():
                return entrada
        except Exception:
            logging.debug("Leitura por PeekNamedPipe indisponivel", exc_info=True)
    return sys.stdin.buffer


def _escrever(quadro: dict) -> None:
    """Uma linha JSON no protocolo; a trava impede misturar respostas de threads."""
    try:
        dados = json.dumps(quadro, ensure_ascii=False, default=str, separators=(",", ":"))
    except Exception as exc:  # noqa: BLE001 - nunca derruba o motor por causa de um resultado
        dados = json.dumps({"id": quadro.get("id"), "ok": False, "e": f"Resultado não serializável: {exc}"},
                           ensure_ascii=False)
    corpo = (dados.replace("\r", "").replace("\n", " ") + "\n").encode("utf-8")
    try:
        with _lock_saida:
            _proto.write(corpo)
    except (OSError, ValueError):  # o pai fechou o pipe
        _sair_sem_pai()


def _ok(rid, resultado) -> None:
    _escrever({"id": rid, "ok": True, "r": resultado})


def _erro(rid, mensagem: str) -> None:
    _escrever({"id": rid, "ok": False, "e": mensagem})


# ------------------------------------------------------------------ ciclo de vida


def _cancelar_tudo() -> None:
    """Pede cancelamento de tudo que roda (nao espera)."""
    import execucoes

    api = _api
    for passo in (api.request_stop_all, api.cancel_job, api.cancel_transcricao):
        try:
            passo()
        except Exception:
            logging.exception("Falha ao cancelar")
    try:
        for ex in execucoes.REGISTRO.snapshot()["execucoes"]:
            if ex.get("cancelavel"):
                execucoes.REGISTRO.cancelar(ex["id"])
    except Exception:
        logging.exception("Falha ao cancelar execucoes")


def _esperar_inativo() -> None:
    """Sai assim que nao houver execucao ativa; o teto so evita travar o fechamento."""
    import execucoes

    limite = time.monotonic() + LIMITE_ENCERRAR_S
    while time.monotonic() < limite:
        try:
            if execucoes.REGISTRO.contar_ativas() == 0:
                return
        except Exception:
            return
        time.sleep(0.02)


def _sair(codigo: int = 0) -> None:
    try:
        logging.info("Motor encerrando (codigo %s)", codigo)
        logging.shutdown()
    except Exception:
        pass
    os._exit(codigo)


def _encerrar_ordenado() -> None:
    if _encerrando.is_set():
        return
    _encerrando.set()
    try:
        _cancelar_tudo()
        _esperar_inativo()
    finally:
        _sair(0)


def _sair_sem_pai() -> None:
    """stdin/stdout do pai se foram: cancela e sai (nunca fica orfao)."""
    _encerrar_ordenado()  # termina o processo


# --------------------------------------------------------------------- requisicoes


def _tabela_metodos() -> dict:
    import extrator_core as core

    def ping():
        return {"versao": core.APP_VERSION, "python": sys.version.split()[0], "pid": os.getpid()}

    def capacidades():
        return dict(CAPACIDADES)

    tabela = {"ping": ping, "capacidades": capacidades}
    for nome in METODOS_API:
        tabela[nome] = getattr(_api, nome)
    return tabela


def _assinatura_aceita(funcao, args: list) -> str | None:
    """Mensagem de erro se os argumentos nao servem para a funcao (None se servem)."""
    import inspect

    try:
        assinatura = inspect.signature(funcao)
    except (ValueError, TypeError):
        return None
    try:
        assinatura.bind(*args)
    except TypeError as exc:
        return str(exc)
    return None


def _executar(rid, nome: str, funcao, args: list) -> None:
    try:
        _ok(rid, funcao(*args))
    except Exception as exc:  # noqa: BLE001 - fronteira do protocolo
        logging.exception("Erro em %s", nome)
        _erro(rid, str(exc) or exc.__class__.__name__)


def _tratar(linha: bytes, tabela: dict) -> None:
    texto = linha.decode("utf-8-sig", errors="replace").strip()
    if not texto:
        return
    try:
        msg = json.loads(texto)
    except ValueError as exc:
        _erro(None, f"JSON inválido: {exc}")
        return
    if not isinstance(msg, dict):
        _erro(None, "A requisição deve ser um objeto JSON.")
        return
    rid = msg.get("id")
    nome = msg.get("m")
    args = msg.get("a")
    if args is None:
        args = []
    if not isinstance(nome, str) or not nome:
        _erro(rid, "Requisição sem o campo \"m\" (nome do método).")
        return
    if not isinstance(args, list):
        _erro(rid, "O campo \"a\" deve ser uma lista de argumentos.")
        return
    if nome == "encerrar":
        _ok(rid, {"ok": True})
        threading.Thread(target=_encerrar_ordenado, daemon=True).start()
        return
    if nome in METODOS_SO_NATIVOS:
        _erro(rid, MSG_SO_NATIVO)
        return
    funcao = tabela.get(nome)
    if funcao is None:
        _erro(rid, f"Método inexistente: {nome}")
        return
    problema = _assinatura_aceita(funcao, args)
    if problema:
        _erro(rid, f"Argumentos inválidos para {nome}: {problema}")
        return
    threading.Thread(target=_executar, args=(rid, nome, funcao, args), daemon=True, name=f"req-{nome}").start()


def main() -> None:
    global _api
    _preparar_stdio()
    _job_object_mata_filhos()
    try:
        import extrator_core as core  # noqa: F401 - configura log e pastas
        from extrator_api import Api

        _api = Api()
        tabela = _tabela_metodos()
    except Exception as exc:  # noqa: BLE001
        logging.exception("Falha ao iniciar o motor")
        _erro(None, f"Falha ao iniciar o motor: {exc}")
        _sair(1)
        return
    logging.info("Motor iniciado (pid %s)", os.getpid())

    entrada = _abrir_entrada()
    while True:
        try:
            linha = entrada.readline()
        except (OSError, ValueError):
            linha = b""
        if not linha:  # stdin fechado: o pai acabou
            logging.info("stdin fechado: encerrando")
            _encerrar_ordenado()
            return
        try:
            _tratar(linha, tabela)
        except Exception as exc:  # noqa: BLE001 - uma linha ruim nao derruba o motor
            logging.exception("Falha ao tratar a requisicao")
            _erro(None, f"Erro interno: {exc}")


if __name__ == "__main__":
    main()
