"""
Extrator de Texto - leitura de logs para a tela "Log".

Os logs ficam em dados/logs/*.log (um por sessao, mais o instalacao.log do
instalador) e podem ficar grandes. Nada aqui le o arquivo inteiro para a
memoria: a leitura e sempre por trechos em bytes (final do arquivo, "desde um
deslocamento", "antes de um deslocamento", intervalo, ao redor de um ponto) ou
em fluxo (busca), e cada trecho so devolve linhas completas.

Formato das linhas da sessao: "AAAA-MM-DD HH:MM:SS,mmm | NIVEL | mensagem".
Linhas sem esse formato (por exemplo, o rastreamento de uma excecao) herdam o
nivel da linha anterior.
"""
from __future__ import annotations

import re
from collections import deque
from datetime import datetime
from pathlib import Path

LINHA_RE = re.compile(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})(?:[,.]\d+)? \| ([A-Z]+) \| (.*)$")
NIVEIS = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
LIMITE_NIVEL = {"tudo": 0, "avisos": 30, "erros": 40}
TAMANHO_TRECHO = 256 * 1024
MAX_TRECHO = 2 * 1024 * 1024
MAX_LINHA_BYTES = 64 * 1024
EXTENSOES = (".log", ".txt")


def nome_valido(pasta: Path, nome: str) -> Path:
    """Resolve `nome` dentro de `pasta`; recusa qualquer caminho fora dela."""
    if not nome or Path(nome).name != nome or Path(nome).suffix.lower() not in EXTENSOES:
        raise ValueError("Nome de registro inválido.")
    caminho = (pasta / nome).resolve()
    if caminho.parent != pasta.resolve():
        raise ValueError("Nome de registro inválido.")
    if not caminho.is_file():
        raise FileNotFoundError("Esse arquivo de registro não existe mais.")
    return caminho


def listar_logs(pasta: Path, atual: str = "") -> list[dict]:
    itens = []
    if pasta.exists():
        for arq in pasta.iterdir():
            if arq.is_file() and arq.suffix.lower() in EXTENSOES:
                try:
                    st = arq.stat()
                except OSError:
                    continue
                quando = datetime.fromtimestamp(st.st_mtime)
                itens.append({
                    "nome": arq.name, "tamanho": st.st_size, "modificado": st.st_mtime,
                    "data": quando.strftime("%Y-%m-%d"), "hora": quando.strftime("%H:%M"),
                    "atual": arq.name == atual,
                    "tipo": "instalacao" if arq.stem.lower().startswith("instalacao") else "sessao",
                })
    itens.sort(key=lambda i: i["modificado"], reverse=True)
    return itens


def parse_linhas(bloco: bytes, base: int, nivel_anterior: str = "INFO") -> list[dict]:
    """Quebra `bloco` (que comeca no byte `base` do arquivo) em linhas {o,t,nv,x}."""
    linhas = []
    nivel = nivel_anterior if nivel_anterior in NIVEIS else "INFO"
    deslocamento = base
    partes = bloco.split(b"\n")
    if partes and partes[-1] == b"":
        partes.pop()  # o bloco termina em quebra de linha
    for bruta in partes:
        texto = bruta.decode("utf-8", errors="replace").rstrip("\r")
        casou = LINHA_RE.match(texto)
        if casou:
            quando, nivel_lido, msg = casou.groups()
            nivel = nivel_lido if nivel_lido in NIVEIS else "INFO"
            linhas.append({"o": deslocamento, "t": quando, "nv": nivel, "x": msg})
        else:
            linhas.append({"o": deslocamento, "t": "", "nv": nivel, "x": texto})
        deslocamento += len(bruta) + 1
    return linhas


def _ler_bytes(caminho: Path, ini: int, fim: int) -> bytes:
    with open(caminho, "rb") as f:
        f.seek(ini)
        return f.read(max(0, fim - ini))


def _resultado(caminho: Path, ini: int, fim: int, linhas: list[dict], **extra) -> dict:
    return {"ok": True, "nome": caminho.name, "tamanho": caminho.stat().st_size, "ini": ini, "fim": fim,
            "linhas": linhas, "truncado_inicio": ini > 0, **extra}


def _alinhar_inicio(bloco: bytes, ini: int) -> tuple[bytes, int]:
    """Descarta a linha cortada no comeco de um trecho que nao parte do byte 0."""
    if ini == 0:
        return bloco, 0
    nl = bloco.find(b"\n")
    if nl < 0:
        return b"", ini + len(bloco)
    return bloco[nl + 1:], ini + nl + 1


def _so_linhas_completas(bloco: bytes, ao_vivo: bool) -> bytes:
    """Em arquivo ainda aberto, a ultima linha pode estar pela metade: fica para a proxima leitura."""
    if ao_vivo:
        ultimo = bloco.rfind(b"\n")
        return bloco[: ultimo + 1] if ultimo >= 0 else b""
    return bloco if bloco.endswith(b"\n") or not bloco else bloco + b"\n"


def ler_cauda(caminho: Path, max_bytes: int = TAMANHO_TRECHO, ao_vivo: bool = False) -> dict:
    max_bytes = max(1024, min(int(max_bytes), MAX_TRECHO))
    tamanho = caminho.stat().st_size
    ini = max(0, tamanho - max_bytes)
    bloco = _ler_bytes(caminho, ini, tamanho)
    bloco, ini = _alinhar_inicio(bloco, ini)
    util = _so_linhas_completas(bloco, ao_vivo)
    fim = ini + len(util) if ao_vivo else tamanho
    return _resultado(caminho, ini, fim, parse_linhas(util, ini))


def ler_desde(caminho: Path, offset: int, max_bytes: int = TAMANHO_TRECHO, nivel_anterior: str = "INFO",
              ao_vivo: bool = True) -> dict:
    """Linhas novas depois de `offset`. Se o arquivo encolheu (limpeza), recomeca pela cauda."""
    max_bytes = max(1024, min(int(max_bytes), MAX_TRECHO))
    tamanho = caminho.stat().st_size
    if offset > tamanho:
        r = ler_cauda(caminho, max_bytes, ao_vivo)
        r["reiniciado"] = True
        return r
    fim_leitura = min(tamanho, offset + max_bytes)
    bloco = _ler_bytes(caminho, offset, fim_leitura)
    if fim_leitura < tamanho:  # trecho cheio: so termina em quebra de linha
        ultimo = bloco.rfind(b"\n")
        bloco = bloco[: ultimo + 1] if ultimo >= 0 else bloco
        util = bloco
    else:
        util = _so_linhas_completas(bloco, ao_vivo)
    fim = offset + len(util)
    return _resultado(caminho, offset, fim, parse_linhas(util, offset, nivel_anterior), reiniciado=False,
                      mais=fim < tamanho)


def ler_antes(caminho: Path, offset: int, max_bytes: int = TAMANHO_TRECHO) -> dict:
    """Trecho imediatamente anterior a `offset` (rolagem para linhas mais antigas)."""
    max_bytes = max(1024, min(int(max_bytes), MAX_TRECHO))
    fim = max(0, min(int(offset), caminho.stat().st_size))
    ini = max(0, fim - max_bytes)
    bloco = _ler_bytes(caminho, ini, fim)
    bloco, ini = _alinhar_inicio(bloco, ini)
    return _resultado(caminho, ini, fim, parse_linhas(bloco, ini))


def ler_intervalo(caminho: Path, ini: int, fim: int | None, max_bytes: int = MAX_TRECHO) -> dict:
    """Linhas entre dois deslocamentos (o trecho do log de uma execucao)."""
    tamanho = caminho.stat().st_size
    ini = max(0, min(int(ini or 0), tamanho))
    fim = tamanho if fim is None else max(ini, min(int(fim), tamanho))
    truncado = fim - ini > max_bytes
    if truncado:
        ini = fim - max_bytes
    bloco = _ler_bytes(caminho, ini, fim)
    bloco, ini = _alinhar_inicio(bloco, ini) if truncado else (bloco, ini)
    util = _so_linhas_completas(bloco, False)
    return _resultado(caminho, ini, fim, parse_linhas(util, ini), recortado=truncado)


def ler_ao_redor(caminho: Path, offset: int, antes: int = 48 * 1024, depois: int = 48 * 1024) -> dict:
    """Janela em volta de um ponto do arquivo (usada por "Ver no log")."""
    tamanho = caminho.stat().st_size
    offset = max(0, min(int(offset), tamanho))
    ini = max(0, offset - antes)
    fim = min(tamanho, offset + depois)
    bloco = _ler_bytes(caminho, ini, fim)
    bloco, ini = _alinhar_inicio(bloco, ini)
    ultimo = bloco.rfind(b"\n")
    if fim < tamanho and ultimo >= 0:
        bloco = bloco[: ultimo + 1]
    util = _so_linhas_completas(bloco, False)
    return _resultado(caminho, ini, ini + len(util), parse_linhas(util, ini), marca=offset)


# ---------------------------------------------------------------- filtros


def passa_filtro(linha: dict, nivel: str = "tudo", texto: str = "") -> bool:
    if NIVEIS.get(linha.get("nv", "INFO"), 20) < LIMITE_NIVEL.get(nivel, 0):
        return False
    if texto and texto.lower() not in (linha.get("t", "") + " " + linha.get("x", "")).lower():
        return False
    return True


def filtrar_linhas(linhas: list[dict], nivel: str = "tudo", texto: str = "") -> list[dict]:
    return [l for l in linhas if passa_filtro(l, nivel, texto)]


def buscar(caminho: Path, nivel: str = "tudo", texto: str = "", limite: int = 2000) -> dict:
    """Varre o arquivo em fluxo (sem carregar tudo) e devolve as ultimas `limite` linhas que casam."""
    limite = max(1, min(int(limite), 20000))
    achadas: deque = deque(maxlen=limite)
    total = 0
    nivel_atual = "INFO"
    deslocamento = 0
    with open(caminho, "rb") as f:
        while True:
            bruta = f.readline(MAX_LINHA_BYTES)
            if not bruta:
                break
            tamanho = len(bruta)
            texto_linha = bruta.decode("utf-8", errors="replace").rstrip("\r\n")
            casou = LINHA_RE.match(texto_linha)
            if casou:
                quando, nv, msg = casou.groups()
                nivel_atual = nv if nv in NIVEIS else "INFO"
                linha = {"o": deslocamento, "t": quando, "nv": nivel_atual, "x": msg}
            else:
                linha = {"o": deslocamento, "t": "", "nv": nivel_atual, "x": texto_linha}
            if passa_filtro(linha, nivel, texto):
                total += 1
                achadas.append(linha)
            deslocamento += tamanho
    return _resultado(caminho, 0, deslocamento, list(achadas), total_achadas=total, cortada=total > limite)
