"""Motor FALSO do Extrator de Texto, so para desenvolver e testar a interface Rust.

Fala o mesmo protocolo do src\\motor.py (JSON por linha em stdin/stdout):
    {"id": 12, "m": "start_convert", "a": [ {...} ]}
    {"id": 12, "ok": true, "r": <json>}   ou   {"id": 12, "ok": false, "e": "mensagem"}
Os formatos de cada resposta e evento vem de src\\extrator_api.py e src\\web\\dev\\mock.js.

Variaveis de ambiente de teste:
    FALSO_CAPS=interlocutores,motor_spark   liga as flags de capacidades
    FALSO_MORRER_APOS=N                     o motor sai (sem aviso) depois de N requisicoes
    FALSO_LENTO=1                           respostas de start_* demoram mais (barras visiveis por mais tempo)
"""
import json
import os
import sys
import threading
import time
import uuid
from collections import deque

LOCK = threading.Lock()
SAIDA = sys.stdout
EVENTOS = deque()
T0 = time.time()
LENTO = 2.5 if os.environ.get("FALSO_LENTO") else 1.0
CAPS = set(filter(None, os.environ.get("FALSO_CAPS", "").split(",")))
MORRER_APOS = int(os.environ.get("FALSO_MORRER_APOS", "0") or 0)
N_REQ = 0
RAIZ = os.getcwd()
LOG_NOME = time.strftime("extrator_%Y%m%d_%H%M%S.log")
LOG = []  # {"o","t","nv","x"}
LOG_TAM = [0]
CANCEL = threading.Event()
PAUSA = threading.Event()
TR_CANCEL = threading.Event()
EXECS = {}
ORDEM = []

CONFIG = {"language": "por+eng", "dpi": 220, "psm": 3, "native_text_threshold": 80, "verbosity": "detalhado",
          "cpu_mode": "normal", "rotate_pages": True, "deskew": True, "pdf_output_type": "pdf", "resume": True,
          "auto_open_output": False, "motor": "local"}
MODELOS = {
    "tiny": {"tamanho_mb": 76, "precisao": "Baixa", "velocidade": "Muito rapida", "descricao": "Para experimentacao rapida"},
    "base": {"tamanho_mb": 142, "precisao": "Razoavel", "velocidade": "Rapida", "descricao": "Equilibrio basico"},
    "small": {"tamanho_mb": 460, "precisao": "Boa", "velocidade": "Moderada", "descricao": "Melhor equilibrio (padrao)"},
    "medium": {"tamanho_mb": 1500, "precisao": "Muito boa", "velocidade": "Lenta", "descricao": "Alta precisao para PT-BR"},
}
INSTALADOS = ["small"]

MODOS = [
    ["auto", "OCR automático", "Mantém o texto nativo e aplica OCR só onde falta. Gera Markdown ou TXT.", "pdf", True, True, False],
    ["force", "OCR forçado", "Reconhece todas as páginas, mesmo as que já têm texto. Mais lento.", "pdf", True, True, False],
    ["direct", "Conversão direta", "Converte PDF, Word, Excel, PowerPoint, HTML e outros para Markdown, sem OCR.", "direct", False, False, False],
    ["searchable_skip", "PDF pesquisável", "Mantém a aparência do PDF e adiciona uma camada de texto pesquisável.", "pdf", True, False, False],
    ["searchable_skip_md", "PDF pesquisável + Markdown", "Gera o PDF pesquisável e também o Markdown dele.", "pdf", True, False, False],
    ["searchable_force", "PDF pesquisável (OCR forçado)", "Refaz o OCR de todas as páginas ao gerar o PDF pesquisável.", "pdf", True, False, False],
    ["searchable_redo", "Refazer OCR existente", "Descarta o OCR antigo do PDF e faz de novo.", "pdf", True, False, False],
    ["images", "OCR de imagens", "Reconhece texto em PNG, JPG, TIFF, BMP e WEBP, podendo juntar tudo em um arquivo.", "images", False, True, True],
]
MODOS_J = [dict(zip(["key", "label", "description", "accepts", "pages", "format", "combine"], m)) for m in MODOS]
LOTE = [
    {"key": "auto_md", "label": "OCR automático para Markdown", "description": "Usa o texto nativo e faz OCR só nas páginas sem texto."},
    {"key": "force_md", "label": "OCR forçado para Markdown", "description": "Reconhece todas as páginas. Mais lento."},
    {"key": "direct", "label": "Conversão direta", "description": "Sem OCR. Aceita PDF, Word, Excel, PowerPoint, HTML e mais."},
    {"key": "searchable", "label": "PDF pesquisável", "description": "Adiciona camada de texto mantendo a aparência."},
    {"key": "searchable_md", "label": "PDF pesquisável + Markdown", "description": "Gera o PDF pesquisável e o Markdown dele."},
]


def emitir(**ev):
    with LOCK:
        EVENTOS.append(ev)


def log(nv, texto):
    with LOCK:
        o = LOG_TAM[0]
        LOG.append({"o": o, "t": time.strftime("%Y-%m-%d %H:%M:%S"), "nv": nv, "x": texto})
        LOG_TAM[0] = o + 80
        return o


# ------------------------------------------------------------------ execucoes

def criar_exec(ferramenta, nome, rotulo, entrada, variante="", repetivel=True, instantanea=False):
    ex = {"id": uuid.uuid4().hex[:8], "ferramenta": ferramenta, "ferramenta_nome": nome, "rotulo": rotulo, "entrada": entrada,
          "variante": variante, "estado": "fila", "estado_texto": "Na fila", "etapa": "", "determinada": False,
          "progresso": None, "progresso_texto": "", "mensagem": "", "erro": "", "avisos": 0, "cancelavel": True,
          "cancelando": False, "repetivel": False, "_repetivel": repetivel, "log_arquivo": LOG_NOME,
          "log_ini": LOG_TAM[0], "log_fim": None, "criada_em": time.time(), "inicio_em": None, "fim_em": None,
          "linhas": [], "origem": "", "_cancel": threading.Event(), "_t0": None, "_total": 0, "_feito": 0, "_inst": instantanea}
    with LOCK:
        EXECS[ex["id"]] = ex
        ORDEM.insert(0, ex["id"])
    return ex


def exec_linha(ex, texto, nivel="info"):
    ex["linhas"].append([time.time(), nivel, texto])
    log({"erro": "ERROR", "aviso": "WARNING"}.get(nivel, "INFO"), "[exec %s] %s" % (ex["id"], texto))


def exec_iniciar(ex):
    ex["estado"], ex["estado_texto"], ex["inicio_em"], ex["_t0"] = "rodando", "Rodando", time.time(), time.time()


def exec_fim(ex, estado, texto, msg="", erro=""):
    ex["estado"], ex["estado_texto"] = estado, texto
    ex["mensagem"], ex["erro"], ex["fim_em"], ex["cancelavel"] = msg, erro, time.time(), False
    ex["repetivel"] = ex["_repetivel"]
    ex["log_fim"] = LOG_TAM[0]
    if estado == "concluida" and ex["_total"]:
        ex["determinada"], ex["progresso"] = True, 100.0


def exec_dict(ex):
    agora = time.time()
    d = {k: v for k, v in ex.items() if not k.startswith("_") and k != "linhas"}
    d["linhas"] = len(ex["linhas"])
    t0 = ex["_t0"]
    d["decorrido_s"] = None if t0 is None else round(((ex["fim_em"] or agora) - t0), 1)
    d["eta_s"] = d["eta_min_s"] = d["eta_max_s"] = None
    d["eta_fonte"] = d["eta_texto"] = ""
    if ex["estado"] == "rodando":
        if ex["_total"]:
            frac = ex["_feito"] / ex["_total"]
            d["determinada"], d["progresso"] = True, round(frac * 100, 1)
            d["progresso_texto"] = "%d de %d %s" % (ex["_feito"], ex["_total"], ex.get("_un", "itens"))
            dec = d["decorrido_s"] or 0
            if dec < 3 or frac <= 0:
                d["eta_texto"] = "estimando…"
            else:
                rest = dec * (1 - frac) / frac
                d["eta_s"], d["eta_fonte"] = rest, "andamento"
                d["eta_min_s"], d["eta_max_s"] = rest * 0.8, rest * 1.3
                d["eta_texto"] = "cerca de %d min" % max(1, round(rest / 60)) if rest >= 45 else "menos de 1 min"
        else:
            d["determinada"], d["progresso"] = False, None
            d["eta_texto"] = "sem estimativa"
    elif ex["estado"] == "concluida" and ex["_total"]:
        d["determinada"], d["progresso"] = True, 100.0
    return d


def dormir(ex, segundos, cancel):
    """Espera `segundos` ou ate cancelar (evento real), devolve True se cancelado."""
    return cancel.wait(segundos) or ex["_cancel"].is_set()


# ------------------------------------------------------------------ trabalhos

def trabalho(params, arquivos, rotulo, ferramenta, nome_fer, acao):
    ex = criar_exec(ferramenta, nome_fer, rotulo, ", ".join(os.path.basename(a) for a in arquivos[:3]), params.get("mode", ""))
    ex["_cancel"] = CANCEL
    CANCEL.clear()

    def rodar():
        exec_iniciar(ex)
        ex["_total"], ex["_un"] = len(arquivos), "arquivo(s)"
        exec_linha(ex, "Iniciando: %s (%d arquivo(s))" % (rotulo, len(arquivos)))
        emitir(type="line", text="Iniciando: %s" % rotulo)
        falhas = 0
        try:
            for i, caminho in enumerate(arquivos, 1):
                if CANCEL.is_set():
                    raise InterruptedError
                nome = os.path.basename(caminho)
                ex["etapa"] = "Arquivo %d de %d: %s" % (i, len(arquivos), nome)
                emitir(type="file_start", index=i, total=len(arquivos), name=nome)
                emitir(type="start", file=nome, total=4)
                for p in range(1, 5):
                    if CANCEL.wait(0.45 * LENTO):
                        raise InterruptedError
                    emitir(type="page", current=p, total=4, source="OCR" if p % 2 == 0 else "texto")
                if "erro" in nome.lower():
                    falhas += 1
                    exec_linha(ex, "FALHA em %s: PDF corrompido (simulado)" % nome, "erro")
                    emitir(type="file_error", name=nome, text="PDF corrompido (simulado)")
                ex["_feito"] = i
            if falhas >= len(arquivos):
                exec_fim(ex, "falhou", "Falhou", erro="Todos os arquivos falharam.")
            elif falhas:
                ex["avisos"] = falhas
                exec_fim(ex, "concluida", "Concluída", msg="%d de %d processado(s)." % (len(arquivos) - falhas, len(arquivos)))
            else:
                exec_fim(ex, "concluida", "Concluída", msg="%d arquivo(s) processado(s)." % len(arquivos))
            emitir(type="done")
        except InterruptedError:
            exec_fim(ex, "cancelada", "Cancelada", msg="Cancelada pelo usuário.")
            emitir(type="cancelled")

    ex["_acao"] = (acao, params)
    ex["cancelavel"] = True
    threading.Thread(target=rodar, daemon=True).start()
    return ex


def cancelar_exec(ex):
    ex["cancelando"] = True
    ex["_cancel"].set()


def tr_trabalho(params):
    itens = params["items"]
    TR_CANCEL.clear()
    PAUSA.clear()
    execs = []
    for it in itens:
        ex = criar_exec("transcricao", "Transcrição", "Transcrever " + os.path.basename(it["path"]), it["path"], params.get("model", "small"))
        execs.append(ex)

    def rodar():
        total = len(itens)
        for idx, (it, ex) in enumerate(zip(itens, execs), 1):
            if TR_CANCEL.is_set() or ex["_cancel"].is_set():
                exec_fim(ex, "cancelada", "Cancelada", msg="Removida da fila antes de começar.")
                emitir(type="transc_item_cancelled", id=it["id"])
                continue
            exec_iniciar(ex)
            nome = os.path.basename(it["path"])
            emitir(type="transc_item_start", id=it["id"], index=idx, total=total, name=nome)
            dur = 600.0
            feito = 0.0
            ex["_total"], ex["_un"] = dur, "segundo(s)"
            ex["etapa"] = "Arquivo %d de %d: transcrevendo" % (idx, total)
            t_ini = time.time()
            cancelou = False
            while feito < dur:
                if TR_CANCEL.is_set() or ex["_cancel"].is_set():
                    cancelou = True
                    break
                if not PAUSA.is_set():
                    feito += 60
                ex["_feito"] = feito
                dec = time.time() - t_ini
                emitir(type="transc_progress", id=it["id"], index=idx, total=total, pct=100 * feito / dur,
                       processado_s=feito, total_s=dur, restante_s=dur - feito, elapsed_s=dec * 4, eta_s=(dur - feito) / 4)
                if TR_CANCEL.wait(0.4 * LENTO):
                    cancelou = True
                    break
            if cancelou:
                exec_fim(ex, "cancelada", "Cancelada", msg="Transcrição cancelada pelo usuário.")
                emitir(type="transc_item_cancelled", id=it["id"])
                if TR_CANCEL.is_set():
                    break
                continue
            if "erro" in nome.lower():
                exec_fim(ex, "falhou", "Falhou", erro="Áudio ilegível (simulado).")
                emitir(type="transc_item_error", id=it["id"], text="Áudio ilegível (simulado).")
                continue
            saida = os.path.splitext(it["path"])[0] + "." + params.get("format", "txt")
            ex["_feito"] = dur
            exec_fim(ex, "concluida", "Concluída", msg="Salvo em " + saida)
            extra = {}
            if (params.get("interlocutores") or {}).get("ativo"):
                extra["interlocutores"] = {"n": 2, "rotulos": ["Interlocutor 1", "Interlocutor 2"], "tempo_s": [410.0, 190.0], "trocas": 37}
            emitir(type="transc_item_done", id=it["id"], output_path=saida, **extra)
        for ex in execs:
            if ex["estado"] in ("fila", "rodando"):
                exec_fim(ex, "cancelada", "Cancelada", msg="Fila cancelada antes de chegar a vez deste arquivo.")
        emitir(type="transc_queue_done")

    threading.Thread(target=rodar, daemon=True).start()


COMPONENTES = [("Python", "ok", "3.14.7"), ("MarkItDown", "ok", "0.1.6"), ("PyMuPDF", "ok", "instalado"),
               ("pytesseract", "ok", "instalado"), ("Pillow", "ok", "instalado"), ("OCRmyPDF", "ok", "17.8.1"),
               ("Rich", "ok", "instalado"), ("Tesseract", "ok", r"C:\Tesseract-OCR\tesseract.exe"),
               ("Idiomas OCR", "ok", "eng, osd, por"), ("Ghostscript", "aviso", "não encontrado (só PDF/A precisa)"),
               ("ffmpeg", "ok", "7.1"), ("faster-whisper", "ok", "1.2"), ("Modelo de transcricao", "ok", "small")]


def verificar():
    ex = criar_exec("verificacao", "Verificação", "Verificar instalação", "Componentes do programa")
    exec_iniciar(ex)
    ex["_total"], ex["_un"] = len(COMPONENTES), "componente(s)"
    emitir(type="check_start")
    linhas = []
    for i, (nome, estado, det) in enumerate(COMPONENTES, 1):
        time.sleep(0.12)
        ex["_feito"], ex["etapa"] = i, "Verificado: " + nome
        r = {"name": nome, "ok": estado == "ok", "estado": estado, "detail": det}
        linhas.append(r)
        emitir(type="check_row", index=i, **r)
    exec_fim(ex, "concluida", "Concluída", msg="%d de %d OK." % (sum(1 for r in linhas if r["ok"]), len(linhas)))
    emitir(type="check_done", ok=True, rows=linhas, missing_pip=[], missing_model=False)


# ------------------------------------------------------------------ metodos

def m_bootstrap():
    return {"ok": True, "app_name": "Extrator de Texto", "version": "0.1.1", "convert_modes": MODOS_J, "batch_modes": LOTE,
            "config": dict(CONFIG), "defaults": dict(CONFIG),
            "extensions": {"pdf": [".pdf"], "direct": [".csv", ".docx", ".htm", ".html", ".json", ".pdf", ".pptx", ".txt", ".xlsx", ".xml"],
                           "images": [".bmp", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"],
                           "media": [".aac", ".avi", ".flac", ".m4a", ".mkv", ".mov", ".mp3", ".mp4", ".ogg", ".wav", ".webm", ".wma"]},
            "data_dir": os.path.join(RAIZ, "dados"), "log_file": os.path.join(RAIZ, "dados", "logs", LOG_NOME),
            "log_dir": os.path.join(RAIZ, "dados", "logs"), "transcricao": m_transcricao_state_dado(),
            "busy": {"ocr": False, "transcricao": False}}


def m_transcricao_state_dado():
    return {"default_model": "small", "installed": list(INSTALADOS),
            "models": {k: dict(v, instalado=k in INSTALADOS) for k, v in MODOS_M().items()},
            "languages": ["pt", "en", "es", "fr", "de", "ja", "ko", "auto"], "formats": ["txt", "md", "srt", "json"]}


def MODOS_M():
    return MODELOS


def m_start_convert(p):
    if not p.get("files"):
        return {"ok": False, "error": "Selecione ao menos um arquivo."}
    if p["mode"].startswith("searchable") and not p.get("confirm_signed") and any("assinado" in f.lower() for f in p["files"]):
        return {"ok": True, "status": "needs_confirm", "signed": [os.path.basename(f) for f in p["files"] if "assinado" in f.lower()]}
    rot = next(m[1] for m in MODOS if m[0] == p["mode"])
    trabalho(p, p["files"], rot, "converter", "Converter", "start_convert")
    return {"ok": True, "status": "started"}


def m_scan_batch(p):
    if not p.get("input_folder"):
        return {"ok": False, "error": "Escolha uma pasta de entrada válida."}
    time.sleep(0.6)
    saida = p.get("output_folder") or os.path.join(p["input_folder"], "Extrator_saida")
    sig = ["Contrato (assinado).pdf"] if p.get("mode", "").startswith("searchable") else []
    return {"ok": True, "count": 6, "signed": sig, "output_folder": saida, "sample": ["a.pdf", "b.pdf", "c.pdf"]}


def m_start_batch(p):
    rot = next(m["label"] for m in LOTE if m["key"] == p["mode"])
    arqs = [os.path.join(p["input_folder"], "doc%d.pdf" % i) for i in range(1, 7)]
    ex = trabalho(p, arqs, rot, "lote", "Lote", "start_batch")
    ex["entrada"] = "%s (%d arquivo(s))" % (p["input_folder"], len(arqs))
    return {"ok": True, "status": "started", "count": len(arqs)}


def m_start_transcricao(p):
    if not p.get("items"):
        return {"ok": False, "error": "Adicione ao menos um arquivo pendente na fila."}
    tr_trabalho(p)
    return {"ok": True, "status": "started"}


def m_toggle_pause():
    if PAUSA.is_set():
        PAUSA.clear()
        return {"ok": True, "paused": False}
    PAUSA.set()
    return {"ok": True, "paused": True}


def m_download_model(nome):
    if nome not in MODELOS:
        return {"ok": False, "error": "Modelo desconhecido."}
    ex = criar_exec("modelo", "Modelo", "Baixar modelo " + nome, nome, nome)

    def rodar():
        exec_iniciar(ex)
        ex["etapa"] = "Baixando o modelo %s (sem progresso detalhado)" % nome
        time.sleep(1.5 * LENTO)
        INSTALADOS.append(nome)
        exec_fim(ex, "concluida", "Concluída", msg="Modelo pronto.")
        emitir(type="model_download", name=nome, status="done")

    threading.Thread(target=rodar, daemon=True).start()
    return {"ok": True, "status": "started"}


def m_diagnose(path):
    ex = criar_exec("diagnostico", "Diagnóstico", "Diagnosticar arquivo", os.path.basename(path))
    exec_iniciar(ex)
    time.sleep(1.2)
    rows = [["Arquivo", os.path.basename(path)], ["Páginas", "42"], ["Texto nativo", "38 de 42 páginas"],
            ["Imagem de página inteira", "4 páginas"], ["Assinatura digital", "Não detectada"], ["Protegido por senha", "Não"]]
    exec_fim(ex, "concluida", "Concluída", msg="%d item(ns) analisado(s)." % len(rows))
    return {"ok": True, "rows": [{"item": a, "result": b} for a, b in rows]}


def m_save_settings(v):
    if not str(v.get("language", "")).strip():
        return {"ok": False, "error": "Informe o idioma do OCR (por exemplo, por+eng)."}
    for k in ("dpi", "psm", "native_text_threshold"):
        v[k] = int(v[k])
    CONFIG.update({k: v[k] for k in v})
    return {"ok": True, "config": dict(CONFIG)}


def m_exec_listar():
    with LOCK:
        lista = [exec_dict(EXECS[i]) for i in ORDEM]
    return {"ok": True, "versao": 1, "rodando": sum(1 for e in lista if e["estado"] == "rodando"),
            "na_fila": sum(1 for e in lista if e["estado"] == "fila"), "total": len(lista),
            "ferramentas": {"converter": "Converter", "lote": "Lote", "transcricao": "Transcrição", "verificacao": "Verificação",
                            "diagnostico": "Diagnóstico", "modelo": "Modelo"}, "estados": {}, "execucoes": lista}


def m_exec_detalhe(id, desde=0):
    ex = EXECS.get(str(id))
    if ex is None:
        return {"ok": False, "error": "Execução não encontrada (talvez tenha sido limpa)."}
    linhas = ex["linhas"][int(desde or 0):]
    return {"ok": True, "execucao": exec_dict(ex), "proximo": len(ex["linhas"]), "ativo": ex["estado"] in ("fila", "rodando"),
            "linhas": [{"t": time.strftime("%H:%M:%S", time.localtime(t)), "n": n, "x": x} for t, n, x in linhas]}


def m_exec_repetir(id):
    ex = EXECS.get(str(id))
    if not ex or not ex.get("_acao"):
        if ex and ex["ferramenta"] == "verificacao":
            threading.Thread(target=verificar, daemon=True).start()
            return {"ok": True, "status": "started", "acao": "start_check_installation"}
        return {"ok": False, "error": "Esta execução não pode ser repetida."}
    acao, params = ex["_acao"]
    r = globals()["m_" + acao](dict(params))
    return dict(r, acao=acao)


def pedaco(inicio, fim, **extra):
    linhas = [l for l in LOG if inicio <= l["o"] < fim]
    return {"ok": True, "nome": LOG_NOME, "tamanho": LOG_TAM[0], "ini": linhas[0]["o"] if linhas else inicio,
            "fim": (linhas[-1]["o"] + 80) if linhas else inicio, "linhas": linhas, "truncado_inicio": False, "mais": False,
            "reiniciado": False, "ao_vivo": True, **extra}


def m_log_ler(p):
    modo = p.get("modo", "cauda")
    with LOCK:
        if modo == "desde":
            return pedaco(int(p.get("offset") or 0), 1 << 60)
        if modo == "antes":
            fim = int(p.get("offset") or 0)
            return pedaco(max(0, fim - 80 * 200), fim)
        if modo == "intervalo":
            f = p.get("fim")
            return pedaco(int(p.get("ini") or 0), (1 << 60) if f is None else int(f))
        if modo == "ao_redor":
            o = int(p.get("offset") or 0)
            return pedaco(max(0, o - 80 * 100), o + 80 * 100)
        return pedaco(max(0, LOG_TAM[0] - 80 * 400), 1 << 60)


def m_log_buscar(p):
    nivel, texto = p.get("nivel", "tudo"), (p.get("texto") or "").lower()
    peso = {"tudo": 0, "avisos": 30, "erros": 40}.get(nivel, 0)
    pes = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40}
    with LOCK:
        achadas = [l for l in LOG if pes.get(l["nv"], 20) >= peso and texto in l["x"].lower()]
    return {"ok": True, "nome": LOG_NOME, "tamanho": LOG_TAM[0], "ini": 0, "fim": LOG_TAM[0], "linhas": achadas[-2000:],
            "total_achadas": len(achadas), "cortada": len(achadas) > 2000, "ao_vivo": True}


def m_log_registrar(nivel, texto):
    o = log({"ERROR": "ERROR", "WARNING": "WARNING"}.get(str(nivel).upper(), "INFO"), "Interface: " + str(texto))
    return {"ok": True, "nome": LOG_NOME, "offset": o}


def m_exec_limpar():
    with LOCK:
        rem = [i for i in ORDEM if EXECS[i]["estado"] not in ("fila", "rodando")]
        for i in rem:
            ORDEM.remove(i)
            del EXECS[i]
    return {"ok": True, "removidas": len(rem)}


def m_exec_cancelar(id):
    ex = EXECS.get(str(id))
    if not ex or ex["estado"] not in ("fila", "rodando"):
        return {"ok": False, "error": "Esta execução não pode ser cancelada agora."}
    cancelar_exec(ex)
    if ex["ferramenta"] == "transcricao":
        pass
    return {"ok": True}


METODOS = {
    "ping": lambda: "pong",
    "capacidades": lambda: {"interlocutores": "interlocutores" in CAPS, "motor_spark": "motor_spark" in CAPS, "nativo": True},
    "bootstrap": m_bootstrap,
    "drain_events": lambda: _drenar(),
    "is_busy": lambda: {"ocr": False, "transcricao": False},
    "request_stop_all": lambda: (CANCEL.set(), TR_CANCEL.set()) and None,
    "open_path": lambda p: (log("INFO", "open_path " + p), {"ok": True})[1],
    "open_data_folder": lambda: {"ok": True},
    "start_convert": m_start_convert,
    "cancel_job": lambda: (CANCEL.set(), emitir(type="line", text="Cancelamento solicitado."), {"ok": True})[2],
    "scan_batch": m_scan_batch,
    "start_batch": m_start_batch,
    "transcricao_state": lambda: dict(ok=True, **m_transcricao_state_dado()),
    "transcricao_deps": lambda: {"ok": True, "deps": [{"name": "ffmpeg", "ok": True}, {"name": "faster-whisper", "ok": True}, {"name": "soundfile", "ok": True}]},
    "start_transcricao": m_start_transcricao,
    "toggle_pause_transcricao": m_toggle_pause,
    "cancel_transcricao": lambda: (TR_CANCEL.set(), PAUSA.clear(), {"ok": True})[2],
    "download_model": m_download_model,
    "diagnose": m_diagnose,
    "save_settings": m_save_settings,
    "restore_settings": lambda: (CONFIG.update({"language": "por+eng", "dpi": 220, "psm": 3, "native_text_threshold": 80, "verbosity": "detalhado",
                                                "cpu_mode": "normal", "rotate_pages": True, "deskew": True, "pdf_output_type": "pdf",
                                                "resume": True, "auto_open_output": False, "motor": "local"}), {"ok": True, "config": dict(CONFIG)})[1],
    "check_installation": lambda: {"ok": True},
    "start_check_installation": lambda: (threading.Thread(target=verificar, daemon=True).start(), {"ok": True, "status": "started"})[1],
    "install_missing": lambda: (threading.Timer(1.0, lambda: emitir(type="install_fix_done")).start(), {"ok": True, "status": "started"})[1],
    "clear_data": lambda: {"ok": True, "logs": 3, "jobs": 1},
    "exec_listar": m_exec_listar,
    "exec_detalhe": m_exec_detalhe,
    "exec_cancelar": m_exec_cancelar,
    "exec_repetir": m_exec_repetir,
    "exec_resumo": lambda id: {"ok": True, "texto": "Execução " + str(id)},
    "exec_limpar_concluidas": m_exec_limpar,
    "log_arquivos": lambda: {"ok": True, "atual": LOG_NOME, "pasta": os.path.join(RAIZ, "dados", "logs"), "arquivos": [
        {"nome": LOG_NOME, "tamanho": LOG_TAM[0], "modificado": 0, "data": time.strftime("%Y-%m-%d"), "hora": time.strftime("%H:%M"), "atual": True, "tipo": "sessao"},
        {"nome": "instalacao.log", "tamanho": 9000, "modificado": 0, "data": "2026-11-15", "hora": "18:30", "atual": False, "tipo": "instalador"}]},
    "log_ler": m_log_ler,
    "log_buscar": m_log_buscar,
    "log_registrar": m_log_registrar,
    "log_abrir_pasta": lambda: {"ok": True},
    "spark_testar": lambda: {"ok": True, "latencia_ms": 42, "servico": "transcricao-gpu"},
    "encerrar": lambda: (threading.Timer(0.05, lambda: os._exit(0)).start(), {"ok": True})[1],
}


def _drenar():
    with LOCK:
        out = list(EVENTOS)
        EVENTOS.clear()
    return out


def tratar(req):
    global N_REQ
    N_REQ += 1
    if MORRER_APOS and N_REQ > MORRER_APOS:
        os._exit(3)
    rid, nome, args = req.get("id"), req.get("m"), req.get("a") or []
    try:
        f = METODOS.get(nome)
        if f is None:
            resp = {"id": rid, "ok": False, "e": "Método desconhecido: %s" % nome}
        else:
            resp = {"id": rid, "ok": True, "r": f(*args)}
    except Exception as exc:  # noqa: BLE001
        resp = {"id": rid, "ok": False, "e": "%s: %s" % (exc.__class__.__name__, exc)}
    with LOCK:
        SAIDA.write(json.dumps(resp, ensure_ascii=False) + "\n")
        SAIDA.flush()


def semear():
    """FALSO_SEMEAR=1: historico de exemplo (para capturas de tela da tela Inicio)."""
    agora = time.time()

    def pronto(ferr, nome, rotulo, entrada, atras_s, dur_s, estado="concluida", texto="Concluída", msg="", erro=""):
        ex = criar_exec(ferr, nome, rotulo, entrada)
        ex["criada_em"] = agora - atras_s - dur_s
        ex["inicio_em"] = agora - atras_s - dur_s
        ex["_t0"] = ex["inicio_em"]
        exec_fim(ex, estado, texto, msg=msg, erro=erro)
        ex["fim_em"] = agora - atras_s
        return ex

    pronto("verificacao", "Verificação", "Verificar instalação", "Componentes do programa", 5000, 9)
    pronto("converter", "Converter", "OCR automático", r"C:\Processos\Petição inicial - Silva.pdf", 4200, 42, msg="1 arquivo(s) processado(s).")
    pronto("lote", "Lote", "Lote: OCR automático", r"C:\Contratos (24 arquivo(s))", 2600, 11, "falhou", "Falhou", erro="A pasta não pôde ser lida.")
    pronto("converter", "Converter", "Conversão direta", r"C:\Financeiro\Planilha de custos.xlsx", 900, 6, msg="1 arquivo(s) processado(s).")
    ex = criar_exec("transcricao", "Transcrição", "Transcrever Entrevista-04.mp3", r"C:\Áudios\Entrevista-04.mp3")
    ex["criada_em"] = agora - 140
    exec_iniciar(ex)
    ex["_t0"] = agora - 130
    ex["_total"], ex["_feito"], ex["_un"] = 100, 38, "%"
    ex["etapa"] = "Transcrevendo"
    ex2 = criar_exec("transcricao", "Transcrição", "Transcrever Aula 12.m4a", r"C:\Áudios\Aula 12.m4a")
    ex2["criada_em"] = agora - 139


def main():
    log("INFO", "Inicio Extrator de Texto (motor falso)")
    if os.environ.get("FALSO_SEMEAR"):
        semear()
    log("WARNING", "Tesseract sem o idioma por (simulado)")
    for linha in sys.stdin:
        linha = linha.strip()
        if not linha:
            continue
        try:
            req = json.loads(linha)
        except ValueError:
            continue
        threading.Thread(target=tratar, args=(req,), daemon=True).start()
    os._exit(0)


if __name__ == "__main__":
    main()
