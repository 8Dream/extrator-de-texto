"""
Extrator de Texto - registro central de execucoes.

Toda tarefa do programa (converter, lote, OCR, transcricao, verificacao,
diagnostico, limpeza, instalacao, configuracoes...) passa por aqui. Cada
execucao tem id, ferramenta, entrada, estado (fila, rodando, concluida,
falhou, cancelada, interrompida), etapa atual, progresso, estimativa de tempo
(ETA) e as proprias linhas de log.

Regras de honestidade da barra e da estimativa:

* Barra DETERMINADA so quando o total e conhecido (paginas, arquivos, segundos
  de audio, componentes). Sem total, o progresso e `None` e a interface mostra
  o indicador indeterminado com a etapa. Se o total passa a ser conhecido
  depois, a execucao migra sozinha de indeterminada para determinada.
* O ETA vem da taxa observada (media movel exponencial) e, quando existir, do
  historico de execucoes anteriores da mesma ferramenta (segundos por pagina,
  por MB, por segundo de audio), guardado em dados/historico_execucoes.json.
  Ate haver dados suficientes o texto e "estimando..."; tarefas instantaneas
  nao mostram ETA; o valor nunca e negativo.

O modulo nao importa extrator_core (evita ciclo): quem o configura e o proprio
extrator_core, por `REGISTRO.configurar(pasta, ...)`. Todo o estado e protegido
por um RLock; o relogio e injetavel para testes (sem dormir).
"""
from __future__ import annotations

import contextlib
import json
import logging
import os
import re
import threading
import time
import uuid
from collections import deque
from datetime import datetime
from pathlib import Path
from typing import Callable

ESTADOS = ("fila", "rodando", "concluida", "falhou", "cancelada", "interrompida")
ATIVOS = ("fila", "rodando")
ESTADO_TEXTO = {
    "fila": "Na fila", "rodando": "Rodando", "concluida": "Concluída",
    "falhou": "Falhou", "cancelada": "Cancelada", "interrompida": "Interrompida",
}
FERRAMENTAS = {
    "converter": "Converter", "lote": "Lote", "transcricao": "Transcrição",
    "modelo": "Modelo de transcrição", "verificacao": "Verificação",
    "diagnostico": "Diagnóstico", "limpeza": "Limpeza", "instalacao": "Instalação",
    "configuracoes": "Configurações",
}

ARQUIVO_HISTORICO = "historico_execucoes.json"
MAX_EXECUCOES_MEMORIA = 200
MAX_EXECUCOES_DISCO = 50
MAX_LINHAS_LOG = 400
LINHAS_LOG_DISCO = 60
MAX_CARACTERES_LINHA = 400
MAX_DURACOES = 300
MAX_BYTES_HISTORICO = 1_500_000
LIMIAR_INSTANTANEA_S = 3.0
_PLURAL = {"página": "páginas", "arquivo": "arquivos", "imagem": "imagens",
           "componente": "componentes", "segundo": "segundos", "MB": "MB"}

_log = logging.getLogger("extrator.execucoes")
_ATUAL = threading.local()


# ---------------------------------------------------------------- formatacao


def formatar_eta(segundos: float) -> str:
    """'cerca de 2 min', 'menos de 5 s'... Aproximacao, nunca falsa precisao."""
    s = max(0.0, float(segundos))
    if s < 5:
        return "menos de 5 s"
    if s < 60:
        return f"cerca de {int(round(s / 5.0) * 5) or 5} s"
    if s < 3600:
        m = max(1, int(round(s / 60.0)))
        if m >= 10:
            m = int(round(m / 5.0) * 5)
        if m >= 60:
            return "cerca de 1 h"
        return f"cerca de {m} min"
    horas = int(s // 3600)
    minutos = int(round((s % 3600) / 60.0 / 5.0) * 5)
    if minutos >= 60:
        horas, minutos = horas + 1, 0
    return f"cerca de {horas} h" + (f" {minutos} min" if minutos else "")


def formatar_duracao(segundos: float | None) -> str:
    if segundos is None:
        return "-"
    s = max(0, int(round(segundos)))
    h, resto = divmod(s, 3600)
    m, sec = divmod(resto, 60)
    if h:
        return f"{h} h {m:02d} min {sec:02d} s"
    if m:
        return f"{m} min {sec:02d} s"
    return f"{sec} s"


def _hms(segundos: float) -> str:
    s = max(0, int(round(segundos)))
    return f"{s // 3600:02d}:{s // 60 % 60:02d}:{s % 60:02d}"


def _mb(n_bytes: float) -> str:
    return f"{n_bytes / 1048576:.1f}".replace(".", ",") + " MB"


def _data(epoch: float | None) -> str:
    return datetime.fromtimestamp(epoch).strftime("%Y-%m-%d %H:%M:%S") if epoch else ""


def _plural(unidade: str, n: float) -> str:
    return unidade if int(n) == 1 else _PLURAL.get(unidade, unidade)


def _cortar(texto: str, limite: int = MAX_CARACTERES_LINHA) -> str:
    texto = str(texto).replace("\r", " ").replace("\n", " ")
    return texto if len(texto) <= limite else texto[: limite - 1] + "…"


# ----------------------------------------------------------------- execucao


class Execucao:
    """Uma tarefa. Todos os metodos publicos sao thread-safe."""

    def __init__(self, reg: "Registro", ferramenta: str, rotulo: str, entrada: str = "", *,
                 variante: str = "", cancelavel: bool = False, instantanea: bool = False,
                 repetir: dict | None = None, id: str | None = None) -> None:
        self._reg = reg
        self.id = id or uuid.uuid4().hex[:12]
        self.ferramenta = ferramenta
        self.rotulo = rotulo
        self.entrada = _cortar(entrada, 600)
        self.variante = variante
        self.estado = "fila"
        self.criada_em = reg._wall()
        self.inicio_em: float | None = None
        self.fim_em: float | None = None
        self.duracao_s: float | None = None
        self._t0: float | None = None
        self.etapa = ""
        # nivel geral (conjunto de itens: arquivos, componentes) e nivel do item
        # (paginas, segundos de audio...). So ha porcentagem com total conhecido.
        self.geral_total: float | None = None
        self.geral_atual = 0.0
        self.geral_unidade = ""
        self.geral_formato = "contagem"
        self.item_total: float | None = None
        self.item_atual = 0.0
        self.item_unidade = ""
        self.item_formato = "contagem"
        self.dimensao: tuple[float, str] | None = None  # tamanho conhecido, sem progresso
        self.mensagem = ""
        self.erro = ""
        self.avisos = 0
        self.cancelavel = bool(cancelavel)
        self.instantanea = bool(instantanea)
        self.repetir = repetir
        self.cancelamento_pedido = False
        self._cancel_cb: Callable[[], None] | None = None
        self.linhas: deque = deque(maxlen=MAX_LINHAS_LOG)
        self.linhas_total = 0
        self.log_arquivo = ""
        self.log_ini: int | None = None
        self.log_fim: int | None = None
        self._base: tuple | None = None
        self._ema: float | None = None
        self._amostras = 0
        self._amostra_t = 0.0
        self._amostra_u = 0.0
        self._final_frac: float | None = None
        self._texto_prog = ""  # progresso de uma execucao carregada do disco

    # ------------------------------------------------------------ propriedades

    @property
    def ativo(self) -> bool:
        return self.estado in ATIVOS

    @property
    def nome_ferramenta(self) -> str:
        return FERRAMENTAS.get(self.ferramenta, self.ferramenta)

    # ------------------------------------------------------------ ciclo de vida

    def iniciar(self) -> None:
        reg = self._reg
        with reg._lock:
            if self.estado != "fila":
                return
            self.estado = "rodando"
            self._t0 = reg._clock()
            self.inicio_em = reg._wall()
            self._amostra_t = self._t0
            self.log_arquivo, self.log_ini = reg._posicao_log()
        _log.info("[exec %s] %s iniciada%s", self.id, self.rotulo, f": {self.entrada}" if self.entrada else "")
        reg._mudou()
        reg._persistir()

    def definir_cancelamento(self, callback: Callable[[], None]) -> None:
        with self._reg._lock:
            self._cancel_cb = callback
            self.cancelavel = True

    def pedir_cancelamento(self) -> bool:
        """Pede o cancelamento cooperativo. Devolve False se nao for possivel."""
        reg = self._reg
        with reg._lock:
            if not (self.ativo and self.cancelavel and self._cancel_cb and not self.cancelamento_pedido):
                return False
            self.cancelamento_pedido = True
            callback = self._cancel_cb
            if self.estado == "rodando":
                self.etapa = "Cancelando... aguardando a etapa atual terminar"
        reg._mudou()
        with contextlib.suppress(Exception):
            callback()
        return True

    def concluir(self, mensagem: str = "", avisos: int = 0) -> bool:
        return self._finalizar("concluida", mensagem=mensagem, avisos=avisos)

    def falhar(self, erro: str) -> bool:
        return self._finalizar("falhou", erro=erro)

    def cancelar(self, mensagem: str = "Cancelada.") -> bool:
        return self._finalizar("cancelada", mensagem=mensagem)

    def _finalizar(self, estado: str, mensagem: str = "", erro: str = "", avisos: int = 0) -> bool:
        reg = self._reg
        with reg._lock:
            if not self.ativo:
                return False
            agora = reg._clock()
            self.estado = estado
            self.fim_em = reg._wall()
            self.duracao_s = max(0.0, agora - self._t0) if self._t0 is not None else 0.0
            if estado == "concluida":
                self._completar_progresso()
            self._final_frac = self._fracao()
            self.mensagem = _cortar(mensagem, 600)
            self.erro = _cortar(erro, 1000)
            self.avisos = int(avisos)
            self.cancelamento_pedido = False
            dim = self._dimensao_hist()
            if self._t0 is None:
                self.inicio_em = None
            nivel = logging.ERROR if estado == "falhou" else logging.INFO
            texto = f"{ESTADO_TEXTO[estado].lower()} em {formatar_duracao(self.duracao_s)}"
            if erro:
                texto += f": {self.erro}"
            elif mensagem:
                texto += f": {self.mensagem}"
            self._anexar("erro" if estado == "falhou" else "info", texto)
            if estado == "concluida" and self._t0 is not None:
                reg._registrar_duracao(self, dim)
        _log.log(nivel, "[exec %s] %s %s", self.id, self.rotulo, texto)
        with reg._lock:
            _arq, self.log_fim = reg._posicao_log()
        reg._mudou()
        reg._persistir()
        return True

    # ---------------------------------------------------------------- andamento

    def definir_etapa(self, texto: str, registrar: bool = False) -> None:
        texto = _cortar(texto, 300)
        with self._reg._lock:
            if not self.ativo:
                return
            mudou = texto != self.etapa
            self.etapa = texto
        if mudou:
            if registrar:
                _log.info("[exec %s] etapa: %s", self.id, texto)
            self._reg._mudou()

    def definir_geral(self, total: float | None, unidade: str = "arquivo", formato: str = "contagem") -> None:
        """Total do conjunto de itens (ex.: 5 arquivos). Passa a ter barra determinada."""
        with self._reg._lock:
            self.geral_total = float(total) if total and total > 0 else None
            self.geral_unidade, self.geral_formato = unidade, formato
            self._amostrar()
        self._reg._mudou()

    def avancar_geral(self, atual: float) -> None:
        with self._reg._lock:
            if self.geral_total and atual > self.geral_total:
                self.geral_total = float(atual)  # contagem previa errada: cresce, nunca passa de 100%
            self.geral_atual = max(0.0, float(atual))
            self._amostrar()
        self._reg._mudou()

    def novo_item(self, indice: int, nome: str = "") -> None:
        """Comeca o item `indice` (1-based) do conjunto; zera o progresso do item."""
        with self._reg._lock:
            self.geral_atual = max(0.0, float(indice - 1))
            self.item_total, self.item_atual, self.item_unidade = None, 0.0, ""
            self.item_formato = "contagem"
            self.dimensao = None
            self._amostrar()
        self._reg._mudou()

    def definir_total(self, total: float | None, unidade: str = "", formato: str = "contagem") -> None:
        """Total do item atual (paginas, segundos de audio). Migra para barra determinada."""
        with self._reg._lock:
            novo = float(total) if total and total > 0 else None
            if novo != self.item_total or unidade != self.item_unidade:
                self.item_total, self.item_unidade, self.item_formato = novo, unidade, formato
                self._amostrar()
        self._reg._mudou()

    def avancar(self, atual: float, base: bool = False) -> None:
        """Progresso do item. base=True: unidades ja prontas (cache), fora do calculo de ritmo."""
        with self._reg._lock:
            if self.item_total and atual > self.item_total:
                self.item_total = float(atual)
            self.item_atual = max(0.0, float(atual))
            if base:
                self._base = None
            self._amostrar()
        self._reg._mudou()

    def definir_dimensao(self, n: float, unidade: str) -> None:
        """Tamanho conhecido SEM progresso real (so alimenta o ETA historico)."""
        with self._reg._lock:
            self.dimensao = (float(n), unidade) if n and n > 0 else None

    def log(self, texto: str, nivel: str = "info") -> None:
        with self._reg._lock:
            self._anexar(nivel, texto)

    def _anexar(self, nivel: str, texto: str) -> None:
        self.linhas.append((self._reg._wall(), nivel, _cortar(texto)))
        self.linhas_total += 1

    # ------------------------------------------------------------- calculos

    def _efetivo(self) -> tuple[float, float | None, str, str]:
        if self.geral_total:
            fracao_item = 0.0
            if self.item_total:
                fracao_item = min(1.0, max(0.0, self.item_atual / self.item_total))
            return (min(self.geral_total, self.geral_atual + fracao_item), self.geral_total,
                    self.geral_unidade, self.geral_formato)
        if self.item_total:
            return min(self.item_atual, self.item_total), self.item_total, self.item_unidade, self.item_formato
        return 0.0, None, "", "contagem"

    def _fracao(self) -> float | None:
        atual, total, _u, _f = self._efetivo()
        if not total:
            return None
        return max(0.0, min(1.0, atual / total))

    def _completar_progresso(self) -> None:
        if self.geral_total:
            self.geral_atual = self.geral_total
            if self.item_total:
                self.item_atual = self.item_total
        elif self.item_total:
            self.item_atual = self.item_total

    def _dimensao_hist(self) -> tuple[float, str] | None:
        if self.geral_total:
            return self.geral_total, self.geral_unidade
        if self.item_total:
            return self.item_total, self.item_unidade
        return self.dimensao

    def _amostrar(self) -> None:
        """Atualiza a media movel exponencial do ritmo (unidades por segundo)."""
        if self._t0 is None or self.estado != "rodando":
            return
        agora = self._reg._clock()
        atual, total, unidade, _f = self._efetivo()
        base = (total, unidade)
        if base != self._base:
            self._base, self._ema, self._amostras = base, None, 0
            self._amostra_t, self._amostra_u = agora, atual
            return
        dt, du = agora - self._amostra_t, atual - self._amostra_u
        if du < 0:
            self._amostra_t, self._amostra_u = agora, atual
            return
        if dt >= 0.2 and du > 0:
            inst = du / dt
            self._ema = inst if self._ema is None else 0.3 * inst + 0.7 * self._ema
            self._amostras += 1
            self._amostra_t, self._amostra_u = agora, atual

    def _eta(self, agora: float) -> dict:
        r = {"eta_s": None, "eta_min_s": None, "eta_max_s": None, "eta_texto": "", "eta_fonte": ""}
        if self.estado != "rodando" or self.instantanea or self._t0 is None:
            return r
        reg = self._reg
        decorrido = max(0.0, agora - self._t0)
        atual, total, unidade, _f = self._efetivo()
        live = None
        if total and self._ema and self._ema > 0 and self._amostras >= 2 and decorrido >= 1.5:
            live = max(0.0, (total - atual) / self._ema)
            parado = agora - self._amostra_t  # tempo sem avancar: o atraso ja ocorrido entra na conta
            if parado > 1.0 / self._ema:
                live += parado - 1.0 / self._ema
        hist = None
        ultrapassou = False
        tem_hist = False
        dim = self._dimensao_hist()
        if dim:
            modelo = reg._modelo_tempo(self.ferramenta, self.variante, dim[1], dim[0])
            if modelo:
                fixo, por_unidade = modelo  # segundos = fixo + por_unidade * tamanho
                tem_hist = True
                if total and dim[1] == unidade:
                    hist = max(0.0, (total - atual) * por_unidade)
                else:
                    restante = fixo + dim[0] * por_unidade - decorrido
                    hist, ultrapassou = max(0.0, restante), restante <= 0
        else:
            tipica = reg._duracao_tipica(self.ferramenta, self.variante)
            if tipica:
                tem_hist = True
                restante = tipica - decorrido
                hist, ultrapassou = max(0.0, restante), restante <= 0
        if ultrapassou and live is None:
            hist = None
        eta = fonte = None
        faixa = (0.8, 1.35)
        if live is not None and hist is not None:
            peso = self._fracao() or 0.5
            eta, fonte = peso * live + (1 - peso) * hist, "misto"
        elif live is not None:
            eta, fonte = live, "andamento"
            faixa = (0.7, 1.5) if self._amostras < 5 else (0.85, 1.25)
        elif hist is not None:
            eta, fonte, faixa = hist, "historico", (0.7, 1.6)
        if eta is None:
            if ultrapassou:
                r["eta_texto"] = "demorando mais que o habitual"
            elif not total and not tem_hist:
                r["eta_texto"] = "sem estimativa" if decorrido >= LIMIAR_INSTANTANEA_S else "estimando…"
            else:
                r["eta_texto"] = "estimando…"
            return r
        eta = max(0.0, eta)
        if decorrido + eta < LIMIAR_INSTANTANEA_S:
            return r  # tarefa instantanea: sem ETA
        r.update(eta_s=round(eta, 1), eta_min_s=round(eta * faixa[0], 1), eta_max_s=round(eta * faixa[1], 1),
                 eta_texto=formatar_eta(eta), eta_fonte=fonte)
        return r

    def progresso_texto(self) -> str:
        atual, total, unidade, formato = self._efetivo()
        if not total:
            return self._texto_prog
        if formato == "duracao":
            return f"{_hms(atual)} de {_hms(total)}"
        if formato == "bytes":
            return f"{_mb(atual)} de {_mb(total)}"
        return f"{int(atual)} de {int(total)} {_plural(unidade, total)}".strip()

    # ---------------------------------------------------------------- saidas

    def para_dict(self) -> dict:
        reg = self._reg
        with reg._lock:
            agora = reg._clock()
            if self.estado == "rodando" and self._t0 is not None:
                decorrido = max(0.0, agora - self._t0)
            else:
                decorrido = self.duracao_s
            if self.ativo:
                fracao = self._fracao()
            else:
                fracao = self._final_frac
            d = {
                "id": self.id, "ferramenta": self.ferramenta, "ferramenta_nome": self.nome_ferramenta,
                "rotulo": self.rotulo, "entrada": self.entrada, "variante": self.variante,
                "estado": self.estado, "estado_texto": ESTADO_TEXTO[self.estado], "etapa": self.etapa,
                "determinada": fracao is not None,
                "progresso": None if fracao is None else round(fracao * 100, 1),
                "progresso_texto": self.progresso_texto() if self.ativo or self.estado == "concluida" else "",
                "decorrido_s": None if decorrido is None else round(decorrido, 1),
                "criada_em": self.criada_em, "inicio_em": self.inicio_em, "fim_em": self.fim_em,
                "mensagem": self.mensagem, "erro": self.erro, "avisos": self.avisos,
                "cancelavel": bool(self.ativo and self.cancelavel and not self.cancelamento_pedido),
                "cancelando": bool(self.ativo and self.cancelamento_pedido),
                "repetivel": bool(self.repetir) and not self.ativo,
                "log_arquivo": self.log_arquivo, "log_ini": self.log_ini, "log_fim": self.log_fim,
                "linhas": self.linhas_total,
            }
            d.update(self._eta(agora))
            return d

    def para_disco(self) -> dict:
        with self._reg._lock:
            fracao = self._final_frac if not self.ativo else self._fracao()
            return {
                "id": self.id, "ferramenta": self.ferramenta, "rotulo": self.rotulo, "entrada": self.entrada,
                "variante": self.variante, "estado": self.estado, "etapa": self.etapa,
                "fracao": fracao, "progresso_texto": self.progresso_texto(),
                "criada_em": self.criada_em, "inicio_em": self.inicio_em, "fim_em": self.fim_em,
                "duracao_s": self.duracao_s, "mensagem": self.mensagem, "erro": self.erro,
                "avisos": self.avisos, "repetir": self.repetir, "instantanea": self.instantanea,
                "log_arquivo": self.log_arquivo, "log_ini": self.log_ini, "log_fim": self.log_fim,
                "linhas": [[t, n, x] for t, n, x in list(self.linhas)[-LINHAS_LOG_DISCO:]],
            }

    @classmethod
    def de_disco(cls, reg: "Registro", d: dict) -> "Execucao":
        ex = cls(reg, str(d.get("ferramenta", "")), str(d.get("rotulo", "")), str(d.get("entrada", "")),
                 variante=str(d.get("variante", "")), repetir=d.get("repetir") if isinstance(d.get("repetir"), dict) else None,
                 instantanea=bool(d.get("instantanea")), id=str(d.get("id") or "") or None)
        estado = d.get("estado")
        ex.estado = estado if estado in ESTADOS else "interrompida"
        ex.etapa = str(d.get("etapa", ""))
        for campo in ("criada_em", "inicio_em", "fim_em", "duracao_s"):
            valor = d.get(campo)
            setattr(ex, campo, float(valor) if isinstance(valor, (int, float)) else None)
        if ex.criada_em is None:
            ex.criada_em = 0.0
        ex.mensagem, ex.erro = str(d.get("mensagem", "")), str(d.get("erro", ""))
        ex.avisos = int(d.get("avisos") or 0)
        fr = d.get("fracao")
        ex._final_frac = float(fr) if isinstance(fr, (int, float)) else None
        ex._texto_prog = str(d.get("progresso_texto", ""))
        ex.log_arquivo = str(d.get("log_arquivo", ""))
        for campo in ("log_ini", "log_fim"):
            valor = d.get(campo)
            setattr(ex, campo, int(valor) if isinstance(valor, (int, float)) else None)
        for item in d.get("linhas") or []:
            if isinstance(item, list) and len(item) == 3:
                ex.linhas.append((float(item[0] or 0), str(item[1]), _cortar(item[2])))
                ex.linhas_total += 1
        if ex.ativo:  # estava rodando quando o programa fechou
            ex.estado = "interrompida"
            ex.mensagem = "O programa foi fechado antes de a execução terminar."
            if ex.fim_em is None:
                ex.fim_em = ex.inicio_em or ex.criada_em
        return ex

    def resumo_texto(self) -> str:
        d = self.para_dict()
        partes = [
            f"Execução: {d['ferramenta_nome']} - {d['rotulo']}",
            f"Estado: {d['estado_texto']}",
        ]
        if d["entrada"]:
            partes.append(f"Entrada: {d['entrada']}")
        if d["inicio_em"]:
            partes.append(f"Início: {_data(d['inicio_em'])}")
        if d["fim_em"]:
            partes.append(f"Fim: {_data(d['fim_em'])}")
        if d["decorrido_s"] is not None:
            partes.append(f"Duração: {formatar_duracao(d['decorrido_s'])}")
        if d["progresso_texto"]:
            partes.append(f"Progresso: {d['progresso_texto']}")
        if d["etapa"]:
            partes.append(f"Etapa: {d['etapa']}")
        if d["mensagem"]:
            partes.append(f"Resultado: {d['mensagem']}")
        if d["erro"]:
            partes.append(f"Erro: {d['erro']}")
        if d["log_arquivo"]:
            partes.append(f"Log: {d['log_arquivo']} (bytes {d['log_ini']} a {d['log_fim'] or 'atual'})")
        partes.append(f"ID: {d['id']}")
        with self._reg._lock:
            ultimas = list(self.linhas)[-15:]
        if ultimas:
            partes.append("Últimas linhas:")
            partes.extend(f"  [{datetime.fromtimestamp(t).strftime('%H:%M:%S')}] {x}" for t, _n, x in ultimas)
        return "\n".join(partes)


# ----------------------------------------------------------------- registro


class Registro:
    def __init__(self, pasta: Path | str | None = None, *, relogio: Callable[[], float] = time.monotonic,
                 relogio_parede: Callable[[], float] = time.time,
                 posicao_log: Callable[[], tuple[str, int | None]] | None = None) -> None:
        self._lock = threading.RLock()
        self._io_lock = threading.Lock()
        self._clock = relogio
        self._wall = relogio_parede
        self._posicao_log_fn = posicao_log
        self._execs: dict[str, Execucao] = {}
        self._duracoes: list[dict] = []
        self._versao = 0
        self._pasta: Path | None = None
        if pasta is not None:
            self.configurar(pasta, posicao_log)

    # --------------------------------------------------------------- configuracao

    def configurar(self, pasta: Path | str | None, posicao_log: Callable[[], tuple[str, int | None]] | None = None) -> None:
        with self._lock:
            self._pasta = Path(pasta) if pasta else None
            if posicao_log is not None:
                self._posicao_log_fn = posicao_log
            self._execs.clear()
            self._duracoes = []
        self._carregar()

    @property
    def arquivo(self) -> Path | None:
        return self._pasta / ARQUIVO_HISTORICO if self._pasta else None

    def _posicao_log(self) -> tuple[str, int | None]:
        if self._posicao_log_fn is None:
            return "", None
        try:
            return self._posicao_log_fn()
        except Exception:
            return "", None

    def _mudou(self) -> None:
        with self._lock:
            self._versao += 1

    @property
    def versao(self) -> int:
        return self._versao

    # ----------------------------------------------------------------- criacao

    def criar(self, ferramenta: str, rotulo: str, entrada: str = "", **opcoes) -> Execucao:
        ex = Execucao(self, ferramenta, rotulo, entrada, **opcoes)
        with self._lock:
            self._execs[ex.id] = ex
            self._aparar_memoria()
        self._mudou()
        return ex

    @contextlib.contextmanager
    def executar(self, ex: Execucao):
        """Marca `ex` como a execucao da thread atual (o motor reporta por
        `execucoes.etapa()`, `total()`...) e garante um estado final."""
        anterior = getattr(_ATUAL, "ex", None)
        _ATUAL.ex = ex
        try:
            ex.iniciar()
            yield ex
        except BaseException as exc:
            if ex.ativo:
                ex.falhar(str(exc) or exc.__class__.__name__)
            raise
        else:
            if ex.ativo:
                ex.concluir()
        finally:
            _ATUAL.ex = anterior

    @contextlib.contextmanager
    def tarefa(self, ferramenta: str, rotulo: str, entrada: str = "", **opcoes):
        """Tarefa sincrona: cria, executa e finaliza ao sair do bloco."""
        ex = self.criar(ferramenta, rotulo, entrada, **opcoes)
        with self.executar(ex):
            yield ex

    def _aparar_memoria(self) -> None:
        finais = sorted((e for e in self._execs.values() if not e.ativo), key=lambda e: e.criada_em)
        for ex in finais[: max(0, len(finais) - MAX_EXECUCOES_MEMORIA)]:
            self._execs.pop(ex.id, None)

    # ------------------------------------------------------------------ consulta

    def obter(self, id: str) -> Execucao | None:
        with self._lock:
            return self._execs.get(str(id))

    def snapshot(self) -> dict:
        with self._lock:
            execs = sorted(self._execs.values(), key=lambda e: e.criada_em, reverse=True)
            lista = [e.para_dict() for e in execs]
            rodando = sum(1 for e in execs if e.estado == "rodando")
            fila = sum(1 for e in execs if e.estado == "fila")
            return {"versao": self._versao, "rodando": rodando, "na_fila": fila, "total": len(lista),
                    "execucoes": lista, "ferramentas": dict(FERRAMENTAS), "estados": dict(ESTADO_TEXTO)}

    def detalhe(self, id: str, desde: int = 0) -> dict | None:
        with self._lock:
            ex = self._execs.get(str(id))
            if ex is None:
                return None
            primeiro = ex.linhas_total - len(ex.linhas)
            desde = max(int(desde or 0), primeiro)
            linhas = list(ex.linhas)[desde - primeiro:]
            return {
                "execucao": ex.para_dict(), "proximo": ex.linhas_total, "ativo": ex.ativo,
                "linhas": [{"t": datetime.fromtimestamp(t).strftime("%H:%M:%S"), "n": n, "x": x} for t, n, x in linhas],
            }

    def contar_ativas(self) -> int:
        with self._lock:
            return sum(1 for e in self._execs.values() if e.ativo)

    def cancelar(self, id: str) -> bool:
        ex = self.obter(id)
        return bool(ex and ex.pedir_cancelamento())

    def limpar_concluidas(self) -> int:
        with self._lock:
            remover = [i for i, e in self._execs.items() if not e.ativo]
            for i in remover:
                del self._execs[i]
        if remover:
            self._mudou()
            self._persistir()
        return len(remover)

    # ------------------------------------------------------------------ historico

    def _registrar_duracao(self, ex: Execucao, dim: tuple[float, str] | None) -> None:
        if ex.duracao_s is None:
            return
        registro = {"f": ex.ferramenta, "v": ex.variante, "u": dim[1] if dim else "",
                    "n": round(dim[0], 3) if dim else 0, "s": round(ex.duracao_s, 3), "t": round(self._wall())}
        self._duracoes.append(registro)
        del self._duracoes[: max(0, len(self._duracoes) - MAX_DURACOES)]

    def _recentes(self, ferramenta: str, variante: str, unidade: str) -> list[dict]:
        with self._lock:
            return [d for d in self._duracoes if d["f"] == ferramenta and d["v"] == variante and d["u"] == unidade][-15:]

    def _modelo_tempo(self, ferramenta: str, variante: str, unidade: str, n: float) -> tuple[float, float] | None:
        """(segundos fixos, segundos por unidade) aprendidos das execucoes anteriores.

        Com tamanhos bem variados no historico, ajusta uma reta (custo fixo de
        iniciar + custo por unidade): sem isso, um arquivo minusculo que levou 3 s
        so para abrir o processo faria um arquivo grande parecer levar horas. Com
        tamanhos parecidos, usa a razao simples, mas so para tamanhos na mesma
        ordem de grandeza do que ja foi observado; fora disso, nao estima."""
        if not unidade or not n:
            return None
        registros = [d for d in self._recentes(ferramenta, variante, unidade) if d["n"] > 0 and d["s"] > 0]
        if not registros:
            return None
        ns = [d["n"] for d in registros]
        if len(registros) >= 3 and max(ns) >= 1.5 * min(ns):
            media_n = sum(ns) / len(ns)
            media_s = sum(d["s"] for d in registros) / len(registros)
            var = sum((d["n"] - media_n) ** 2 for d in registros)
            if var > 0:
                b = sum((d["n"] - media_n) * (d["s"] - media_s) for d in registros) / var
                if b > 0:
                    return max(0.0, media_s - b * media_n), b
        if not (min(ns) / 3.0 <= n <= max(ns) * 3.0):
            return None
        return 0.0, sum(d["s"] for d in registros) / sum(ns)

    def _duracao_tipica(self, ferramenta: str, variante: str) -> float | None:
        registros = sorted(d["s"] for d in self._recentes(ferramenta, variante, "") if d["s"] >= 0)
        if not registros:
            return None
        meio = len(registros) // 2
        return registros[meio] if len(registros) % 2 else (registros[meio - 1] + registros[meio]) / 2

    def duracoes(self) -> list[dict]:
        with self._lock:
            return list(self._duracoes)

    def _montar_dados(self) -> dict:
        with self._lock:
            execs = sorted(self._execs.values(), key=lambda e: e.criada_em, reverse=True)[:MAX_EXECUCOES_DISCO]
            return {"versao": 1, "salvo_em": self._wall(), "execucoes": [e.para_disco() for e in execs],
                    "duracoes": list(self._duracoes)}

    def _persistir(self) -> None:
        caminho = self.arquivo
        if caminho is None:
            return
        try:
            dados = self._montar_dados()
            texto = json.dumps(dados, ensure_ascii=False, separators=(",", ":"))
            while len(texto.encode("utf-8")) > MAX_BYTES_HISTORICO and (dados["execucoes"] or dados["duracoes"]):
                if any(e["linhas"] for e in dados["execucoes"]):
                    for e in dados["execucoes"]:
                        e["linhas"] = e["linhas"][(len(e["linhas"]) + 1) // 2:]  # descarta ao menos uma
                elif len(dados["execucoes"]) > 1:
                    dados["execucoes"].pop()
                else:
                    dados["duracoes"] = dados["duracoes"][len(dados["duracoes"]) // 2:]
                texto = json.dumps(dados, ensure_ascii=False, separators=(",", ":"))
            with self._io_lock:
                caminho.parent.mkdir(parents=True, exist_ok=True)
                tmp = caminho.with_name(f"{caminho.name}.{os.getpid()}.{threading.get_ident()}.tmp")
                tmp.write_text(texto, encoding="utf-8")
                for tentativa in range(8):  # o Windows recusa o replace se um antivirus estiver lendo o destino
                    try:
                        os.replace(tmp, caminho)
                        break
                    except PermissionError:
                        if tentativa == 7:
                            raise
                        time.sleep(0.03)
        except Exception:  # historico e conveniencia: nunca derruba a tarefa
            _log.exception("Falha ao gravar o historico de execucoes")

    def _carregar(self) -> None:
        caminho = self.arquivo
        if caminho is None or not caminho.exists():
            return
        try:
            dados = json.loads(caminho.read_text(encoding="utf-8"))
            if not isinstance(dados, dict):
                raise ValueError("formato inesperado")
        except Exception:
            _log.warning("Historico de execucoes ilegivel; sera recriado.", exc_info=True)
            with contextlib.suppress(Exception):
                os.replace(caminho, caminho.with_name(caminho.name + ".corrompido"))
            return
        with self._lock:
            for item in dados.get("execucoes") or []:
                if isinstance(item, dict):
                    with contextlib.suppress(Exception):
                        ex = Execucao.de_disco(self, item)
                        self._execs[ex.id] = ex
            for item in dados.get("duracoes") or []:
                if isinstance(item, dict) and {"f", "v", "u", "n", "s"} <= set(item):
                    with contextlib.suppress(Exception):
                        self._duracoes.append({"f": str(item["f"]), "v": str(item["v"]), "u": str(item["u"]),
                                               "n": float(item["n"]), "s": float(item["s"]), "t": item.get("t", 0)})
            del self._duracoes[: max(0, len(self._duracoes) - MAX_DURACOES)]
            self._versao += 1


REGISTRO = Registro()


# ------------------------------------------------ atalhos da thread atual
# O motor (extrator_core, transcricao_core) chama estas funcoes sem precisar
# conhecer a execucao: sem execucao na thread, nao fazem nada.


def atual() -> Execucao | None:
    return getattr(_ATUAL, "ex", None)


def etapa(texto: str) -> None:
    ex = atual()
    if ex:
        ex.definir_etapa(texto)


def total(n: float | None, unidade: str = "", formato: str = "contagem") -> None:
    ex = atual()
    if ex:
        ex.definir_total(n, unidade, formato)


def avancar(valor: float, base: bool = False) -> None:
    ex = atual()
    if ex:
        ex.avancar(valor, base=base)


def dimensao(n: float, unidade: str) -> None:
    ex = atual()
    if ex:
        ex.definir_dimensao(n, unidade)


def log(texto: str, nivel: str = "info") -> None:
    ex = atual()
    if ex:
        ex.log(texto, nivel)


def evento_progresso(ev: dict, ex: Execucao | None = None) -> None:
    """Traduz os eventos de progresso do motor (core.emit_progress). `ex` explicito
    serve a threads auxiliares, que nao herdam a execucao da thread do trabalho."""
    ex = ex or atual()
    if ex is None:
        return
    tipo = ev.get("type")
    if tipo == "start":
        ex.definir_total(ev.get("total"), "página")
        ex.definir_etapa(f"Lendo {ev.get('file', 'arquivo')}")
    elif tipo == "page":
        ex.definir_total(ev.get("total"), "página")
        ex.avancar(ev.get("current") or 0)
        origem = f" ({ev['source']})" if ev.get("source") else ""
        ex.definir_etapa(f"Página {ev.get('current')} de {ev.get('total')}{origem}")
    elif tipo == "image":
        ex.definir_total(ev.get("total"), "imagem")
        ex.avancar(ev.get("current") or 0)
        ex.definir_etapa(f"Imagem {ev.get('current')} de {ev.get('total')}: {ev.get('name', '')}")
    elif tipo == "status":
        ex.definir_etapa(str(ev.get("text", "")))
    elif tipo == "line":
        ex.log(str(ev.get("text", "")))
