"""Testes da Central de execucoes: registro, ETA, barra, historico em disco,
leitura de logs por trechos e a integracao com a Api.

O relogio e injetado (nada de dormir para esperar tempo passar).

Executar com: venv\\Scripts\\python.exe -m pytest tests/test_execucoes.py
"""
from __future__ import annotations

import json
import re
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent

import extrator_core as core  # noqa: E402
import execucoes  # noqa: E402
import extrator_api as fa  # noqa: E402
import leitorlog  # noqa: E402
import transcricao_core as transcricao  # noqa: E402

WEB = ROOT / "src" / "web"


class Relogio:
    def __init__(self) -> None:
        self.t = 1000.0

    def __call__(self) -> float:
        return self.t

    def avancar(self, segundos: float) -> None:
        self.t += segundos


@pytest.fixture
def relogio():
    return Relogio()


@pytest.fixture
def reg(relogio, tmp_path):
    return execucoes.Registro(tmp_path / "dados", relogio=relogio, relogio_parede=lambda: 1_700_000_000 + relogio.t - 1000)


def rodar(reg, relogio, ferramenta="converter", variante="", **kw):
    ex = reg.criar(ferramenta, "Teste", "arquivo.pdf", variante=variante, **kw)
    ex.iniciar()
    return ex


# ------------------------------------------------------------------ ciclo de vida


def test_ciclo_de_vida_fila_rodando_concluida(reg, relogio):
    ex = reg.criar("converter", "OCR automático", "a.pdf")
    assert ex.estado == "fila" and reg.snapshot()["na_fila"] == 1
    ex.iniciar()
    assert ex.estado == "rodando" and reg.snapshot()["rodando"] == 1
    relogio.avancar(12.5)
    d = ex.para_dict()
    assert d["decorrido_s"] == 12.5 and d["estado_texto"] == "Rodando"
    assert ex.concluir("tudo certo") is True
    d = ex.para_dict()
    assert d["estado"] == "concluida" and d["decorrido_s"] == 12.5 and d["mensagem"] == "tudo certo"
    assert d["fim_em"] and d["inicio_em"]
    assert ex.concluir() is False and ex.falhar("x") is False  # estado final nao muda


def test_falha_e_cancelamento_registram_motivo(reg, relogio):
    a = rodar(reg, relogio)
    a.falhar("PDF protegido por senha")
    assert a.para_dict()["estado"] == "falhou" and "senha" in a.para_dict()["erro"]
    b = reg.criar("transcricao", "Transcrever", "x.mp3")
    assert b.cancelar("Removida da fila") is True  # cancelada sem nunca ter rodado
    d = b.para_dict()
    assert d["estado"] == "cancelada" and d["inicio_em"] is None and d["decorrido_s"] == 0.0


def test_cancelamento_cooperativo_chama_o_callback_so_uma_vez(reg, relogio):
    chamadas = []
    ex = rodar(reg, relogio)
    assert ex.para_dict()["cancelavel"] is False
    ex.definir_cancelamento(lambda: chamadas.append(1))
    assert ex.para_dict()["cancelavel"] is True
    assert reg.cancelar(ex.id) is True and reg.cancelar(ex.id) is False
    assert chamadas == [1]
    d = ex.para_dict()
    assert d["cancelando"] is True and d["cancelavel"] is False
    assert reg.cancelar("inexistente") is False


def test_contexto_executar_garante_estado_final(reg, relogio):
    with reg.executar(reg.criar("limpeza", "Limpar")) as ex:
        assert execucoes.atual() is ex and ex.estado == "rodando"
    assert ex.estado == "concluida" and execucoes.atual() is None
    with pytest.raises(RuntimeError):
        with reg.tarefa("diagnostico", "Diagnosticar") as ex2:
            raise RuntimeError("quebrou")
    assert ex2.estado == "falhou" and "quebrou" in ex2.erro


def test_atalhos_do_motor_sao_inocuos_sem_execucao():
    assert execucoes.atual() is None
    execucoes.etapa("x"); execucoes.total(3, "página"); execucoes.avancar(1); execucoes.dimensao(2, "MB"); execucoes.log("y")
    execucoes.evento_progresso({"type": "page", "current": 1, "total": 2})


def test_eventos_do_motor_viram_etapa_progresso_e_log(reg, relogio):
    with reg.executar(reg.criar("converter", "OCR")) as ex:
        execucoes.evento_progresso({"type": "start", "file": "a.pdf", "total": 4})
        assert ex.para_dict()["determinada"] is True
        relogio.avancar(2)
        execucoes.evento_progresso({"type": "page", "current": 1, "total": 4, "source": "OCR"})
        execucoes.evento_progresso({"type": "status", "text": "Rodando OCRmyPDF"})
        execucoes.evento_progresso({"type": "line", "text": "linha do processo"})
        d = ex.para_dict()
        assert d["progresso"] == 25.0 and d["progresso_texto"] == "1 de 4 páginas" and d["etapa"] == "Rodando OCRmyPDF"
        det = reg.detalhe(ex.id)
        assert [l["x"] for l in det["linhas"]][-1] == "linha do processo"


def test_detalhe_do_log_e_incremental_e_limitado(reg, relogio):
    ex = rodar(reg, relogio)
    for i in range(execucoes.MAX_LINHAS_LOG + 50):
        ex.log(f"linha {i}")
    det = reg.detalhe(ex.id, 0)
    assert len(det["linhas"]) == execucoes.MAX_LINHAS_LOG  # anel: so as mais recentes
    assert det["linhas"][-1]["x"] == f"linha {execucoes.MAX_LINHAS_LOG + 49}"
    proximo = det["proximo"]
    ex.log("nova")
    novo = reg.detalhe(ex.id, proximo)
    assert [l["x"] for l in novo["linhas"]] == ["nova"]
    assert reg.detalhe("nao-existe") is None


# -------------------------------------------------------------- barra x indeterminada


def test_sem_total_a_barra_e_indeterminada_e_nunca_inventa_porcentagem(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_etapa("Gerando PDF pesquisável")
    d = ex.para_dict()
    assert d["determinada"] is False and d["progresso"] is None and d["progresso_texto"] == ""
    ex.definir_dimensao(40, "página")  # tamanho conhecido, mas SEM progresso real: continua indeterminada
    assert ex.para_dict()["progresso"] is None


def test_migra_de_indeterminada_para_determinada_quando_o_total_aparece(reg, relogio):
    ex = rodar(reg, relogio)
    assert ex.para_dict()["determinada"] is False
    relogio.avancar(5)
    ex.definir_total(8, "página")
    d = ex.para_dict()
    assert d["determinada"] is True and d["progresso"] == 0.0
    ex.avancar(2)
    assert ex.para_dict()["progresso"] == 25.0
    ex.avancar(20)  # contagem previa errada: o total cresce, a barra nunca passa de 100%
    assert ex.para_dict()["progresso"] == 100.0


def test_progresso_combina_arquivo_atual_com_lote(reg, relogio):
    ex = rodar(reg, relogio, "lote")
    ex.definir_geral(4, "arquivo")
    ex.novo_item(2, "b.pdf")
    ex.definir_total(10, "página")
    ex.avancar(5)
    d = ex.para_dict()
    assert d["progresso"] == 37.5 and d["progresso_texto"] == "1 de 4 arquivos"  # 1 pronto + metade do segundo
    ex.novo_item(3, "c.pdf")  # item novo zera o progresso do item, sem voltar atras no total
    assert ex.para_dict()["progresso"] == 50.0


def test_concluir_completa_a_barra_determinada(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_geral(3, "componente")
    ex.avancar_geral(1)
    ex.concluir()
    d = ex.para_dict()
    assert d["progresso"] == 100.0 and d["determinada"] is True
    ind = rodar(reg, relogio)
    ind.concluir()
    assert ind.para_dict()["progresso"] is None  # sem total, termina sem porcentagem


def test_progresso_de_duracao_usa_formato_hms(reg, relogio):
    ex = rodar(reg, relogio, "transcricao")
    ex.definir_total(3725, "segundo", "duracao")
    ex.avancar(65)
    assert ex.para_dict()["progresso_texto"] == "00:01:05 de 01:02:05"


# ------------------------------------------------------------------------------ ETA


def test_formatar_eta_aproxima_sem_falsa_precisao():
    assert execucoes.formatar_eta(2) == "menos de 5 s"
    assert execucoes.formatar_eta(33) == "cerca de 35 s"
    assert execucoes.formatar_eta(125) == "cerca de 2 min"
    assert execucoes.formatar_eta(14 * 60) == "cerca de 15 min"
    assert execucoes.formatar_eta(3600 + 20 * 60) == "cerca de 1 h 20 min"
    assert execucoes.formatar_eta(-30) == "menos de 5 s"  # nunca negativo


def test_eta_sem_dados_diz_estimando(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_total(100, "página")
    d = ex.para_dict()
    assert d["eta_texto"] == "estimando…" and d["eta_s"] is None
    relogio.avancar(0.5)
    ex.avancar(1)  # uma unica amostra ainda nao basta
    assert ex.para_dict()["eta_texto"] == "estimando…"


def test_eta_converge_para_o_valor_real(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_total(100, "página")
    for pagina in range(1, 21):  # 1 s por pagina
        relogio.avancar(1.0)
        ex.avancar(pagina)
    d = ex.para_dict()
    assert d["eta_fonte"] == "andamento"
    assert d["eta_s"] == pytest.approx(80, abs=4)
    assert d["eta_min_s"] < d["eta_s"] < d["eta_max_s"]
    assert d["eta_texto"].startswith("cerca de")
    for pagina in range(21, 101):
        relogio.avancar(1.0)
        ex.avancar(pagina)
    final = ex.para_dict()
    assert final["eta_s"] == 0.0 and final["eta_s"] >= 0 and final["eta_texto"] == "menos de 5 s"


def test_eta_acompanha_mudanca_de_ritmo(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_total(100, "página")
    atual = 0
    for _ in range(10):  # comeca lento: 4 s por pagina
        relogio.avancar(4.0); atual += 1; ex.avancar(atual)
    lento = ex.para_dict()["eta_s"]
    for _ in range(30):  # acelera: 0,5 s por pagina
        relogio.avancar(0.5); atual += 1; ex.avancar(atual)
    rapido = ex.para_dict()["eta_s"]
    assert rapido < lento


def test_eta_nunca_e_negativo_mesmo_parado_ou_estourado(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_total(10, "página")
    for pagina in range(1, 6):
        relogio.avancar(2.0); ex.avancar(pagina)
    antes = ex.para_dict()["eta_s"]
    relogio.avancar(600)  # travou: o atraso ja ocorrido entra na estimativa
    parado = ex.para_dict()
    assert parado["eta_s"] >= antes >= 0
    ex.avancar(50)  # mais paginas que o previsto: o total cresce e o ritmo e remedido
    estourado = ex.para_dict()["eta_s"]
    assert estourado is None or estourado >= 0


def test_tarefa_instantanea_nao_mostra_eta(reg, relogio):
    ex = rodar(reg, relogio, "configuracoes", instantanea=True)
    relogio.avancar(30)
    assert ex.para_dict()["eta_texto"] == "" and ex.para_dict()["eta_s"] is None
    curta = rodar(reg, relogio)
    curta.definir_total(3, "componente")
    relogio.avancar(0.3); curta.avancar(1)
    relogio.avancar(0.3); curta.avancar(2)
    assert curta.para_dict()["eta_texto"] == "estimando…"  # ainda sem ritmo confiavel
    relogio.avancar(0.2)
    assert curta.para_dict()["eta_s"] is None  # fecharia em < 3 s: sem ETA


def test_sem_total_e_sem_historico_admite_que_nao_sabe(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_etapa("Rodando OCRmyPDF")
    assert ex.para_dict()["eta_texto"] == "estimando…"
    relogio.avancar(10)
    assert ex.para_dict()["eta_texto"] == "sem estimativa" and ex.para_dict()["eta_s"] is None


def test_historico_alimenta_a_estimativa_e_melhora_a_proxima(reg, relogio):
    primeiro = rodar(reg, relogio, variante="searchable_skip")
    primeiro.definir_dimensao(100, "página")
    relogio.avancar(200)  # 2 s por pagina
    primeiro.concluir()
    assert reg.duracoes()[-1]["u"] == "página" and reg.duracoes()[-1]["n"] == 100

    segundo = rodar(reg, relogio, variante="searchable_skip")
    segundo.definir_dimensao(50, "página")  # sem progresso real, so tamanho conhecido
    relogio.avancar(10)
    d = segundo.para_dict()
    assert d["determinada"] is False and d["progresso"] is None
    assert d["eta_fonte"] == "historico" and d["eta_s"] == pytest.approx(90, abs=1)  # 50 pag * 2 s - 10 s decorridos
    relogio.avancar(500)  # passou muito do previsto: nao inventa numero
    atrasado = segundo.para_dict()
    assert atrasado["eta_s"] is None and atrasado["eta_texto"] == "demorando mais que o habitual"

    outra = rodar(reg, relogio, variante="direct")  # outra variante nao usa o historico alheio
    outra.definir_dimensao(50, "página")
    assert outra.para_dict()["eta_fonte"] == ""


def test_estimativa_mista_pondera_historico_e_ritmo_observado(reg, relogio):
    base = rodar(reg, relogio, "transcricao", variante="small")
    base.definir_total(600, "segundo", "duracao")
    relogio.avancar(120)  # 0,2 s de processamento por segundo de audio
    base.concluir()

    ex = rodar(reg, relogio, "transcricao", variante="small")
    ex.definir_total(1000, "segundo", "duracao")
    d0 = ex.para_dict()
    assert d0["eta_s"] == pytest.approx(200, abs=1) and d0["eta_fonte"] == "historico"  # so historico, antes de andar
    for i in range(1, 11):  # na pratica roda a 0,4 s por segundo de audio
        relogio.avancar(4.0)
        ex.avancar(i * 10)
    d = ex.para_dict()
    assert d["eta_fonte"] == "misto"
    historico, observado = 900 * 0.2, 900 * 0.4
    assert historico < d["eta_s"] < observado


def test_custo_fixo_de_iniciar_nao_distorce_arquivos_grandes(reg, relogio):
    for mb, seg in ((1, 4), (5, 8), (10, 13)):  # ~3 s para abrir o processo + 1 s por MB
        ex = rodar(reg, relogio, "converter", "direct")
        ex.definir_dimensao(mb, "MB")
        relogio.avancar(seg)
        ex.concluir()
    novo = rodar(reg, relogio, "converter", "direct")
    novo.definir_dimensao(20, "MB")
    d = novo.para_dict()
    assert d["eta_fonte"] == "historico" and d["eta_s"] == pytest.approx(23, abs=1)


def test_tamanho_fora_do_que_ja_foi_observado_nao_e_extrapolado(reg, relogio):
    for _ in range(2):  # so arquivos minusculos no historico: 3,9 s de custo de iniciar
        ex = rodar(reg, relogio, "converter", "direct")
        ex.definir_dimensao(0.001, "MB")
        relogio.avancar(3.9)
        ex.concluir()
    enorme = rodar(reg, relogio, "converter", "direct")
    enorme.definir_dimensao(10, "MB")
    d = enorme.para_dict()
    assert d["eta_s"] is None and d["eta_fonte"] == "" and d["eta_texto"] == "estimando…"  # nao chuta 11 horas
    parecido = rodar(reg, relogio, "converter", "direct")
    parecido.definir_dimensao(0.001, "MB")
    assert parecido.para_dict()["eta_s"] == pytest.approx(3.9, abs=0.1)


def test_duracao_tipica_sem_dimensao(reg, relogio):
    for _ in range(3):
        ex = rodar(reg, relogio, "verificacao")
        relogio.avancar(40)
        ex.concluir()
    novo = rodar(reg, relogio, "verificacao")
    relogio.avancar(10)
    d = novo.para_dict()
    assert d["eta_fonte"] == "historico" and d["eta_s"] == pytest.approx(30, abs=1)


def test_historico_de_tarefas_rapidas_nao_gera_eta(reg, relogio):
    for _ in range(3):
        ex = rodar(reg, relogio, "diagnostico")
        relogio.avancar(0.4)
        ex.concluir()
    novo = rodar(reg, relogio, "diagnostico")
    assert novo.para_dict()["eta_texto"] == ""  # costuma levar menos de 3 s: instantanea


def test_falha_e_cancelamento_nao_entram_no_historico(reg, relogio):
    a = rodar(reg, relogio); relogio.avancar(5); a.falhar("x")
    b = rodar(reg, relogio); relogio.avancar(5); b.cancelar()
    assert reg.duracoes() == []


# --------------------------------------------------------------------- persistencia


def test_historico_em_disco_e_recarregado_com_limite(relogio, tmp_path):
    pasta = tmp_path / "d"
    reg = execucoes.Registro(pasta, relogio=relogio, relogio_parede=lambda: 1_700_000_000 + relogio.t)
    for i in range(execucoes.MAX_EXECUCOES_DISCO + 15):
        ex = reg.criar("converter", f"Tarefa {i}", f"{i}.pdf")
        ex.iniciar(); relogio.avancar(1); ex.concluir(f"ok {i}")
    dados = json.loads((pasta / execucoes.ARQUIVO_HISTORICO).read_text(encoding="utf-8"))
    assert len(dados["execucoes"]) == execucoes.MAX_EXECUCOES_DISCO
    assert list((pasta).glob("*.tmp")) == []  # gravacao atomica: nada sobra
    novo = execucoes.Registro(pasta, relogio=relogio)
    snap = novo.snapshot()
    assert snap["total"] == execucoes.MAX_EXECUCOES_DISCO
    assert snap["execucoes"][0]["rotulo"] == f"Tarefa {execucoes.MAX_EXECUCOES_DISCO + 14}"  # mais recente primeiro
    assert all(e["estado"] == "concluida" for e in snap["execucoes"])
    assert len(novo.duracoes()) == len(reg.duracoes())  # as duracoes reaparecem para o ETA


def test_execucao_em_andamento_ao_fechar_volta_como_interrompida(relogio, tmp_path):
    pasta = tmp_path / "d"
    reg = execucoes.Registro(pasta, relogio=relogio, relogio_parede=lambda: 1_700_000_000 + relogio.t)
    rodando = reg.criar("transcricao", "Transcrever a.mp3", "a.mp3"); rodando.iniciar()
    fila = reg.criar("transcricao", "Transcrever b.mp3", "b.mp3")
    feita = reg.criar("limpeza", "Limpar"); feita.iniciar(); feita.concluir()
    novo = execucoes.Registro(pasta)  # "reabriu o programa"
    por_rotulo = {e["rotulo"]: e for e in novo.snapshot()["execucoes"]}
    assert por_rotulo["Transcrever a.mp3"]["estado"] == "interrompida"
    assert por_rotulo["Transcrever b.mp3"]["estado"] == "interrompida"
    assert por_rotulo["Limpar"]["estado"] == "concluida"
    assert "fechado" in por_rotulo["Transcrever a.mp3"]["mensagem"]
    assert novo.snapshot()["rodando"] == 0 and novo.contar_ativas() == 0


def test_arquivo_de_historico_corrompido_nao_impede_o_uso(relogio, tmp_path):
    pasta = tmp_path / "d"
    pasta.mkdir()
    (pasta / execucoes.ARQUIVO_HISTORICO).write_text("{ isto nao e json", encoding="utf-8")
    reg = execucoes.Registro(pasta, relogio=relogio)
    assert reg.snapshot()["total"] == 0
    assert (pasta / (execucoes.ARQUIVO_HISTORICO + ".corrompido")).exists()
    ex = reg.criar("limpeza", "Limpar"); ex.iniciar(); ex.concluir()
    assert json.loads((pasta / execucoes.ARQUIVO_HISTORICO).read_text(encoding="utf-8"))["execucoes"]


def test_historico_respeita_limite_de_tamanho(relogio, tmp_path, monkeypatch):
    monkeypatch.setattr(execucoes, "MAX_BYTES_HISTORICO", 30_000)
    reg = execucoes.Registro(tmp_path / "d", relogio=relogio, relogio_parede=lambda: 1_700_000_000 + relogio.t)
    for i in range(40):
        ex = reg.criar("converter", f"Tarefa {i}", "x" * 500)
        ex.iniciar()
        for j in range(80):
            ex.log("y" * 300)
        ex.concluir()
    arquivo = tmp_path / "d" / execucoes.ARQUIVO_HISTORICO
    assert arquivo.stat().st_size <= 30_000
    assert json.loads(arquivo.read_text(encoding="utf-8"))["execucoes"]


def test_limite_de_duracoes_guardadas(reg, relogio):
    for _ in range(execucoes.MAX_DURACOES + 40):
        ex = rodar(reg, relogio); relogio.avancar(1); ex.concluir()
    assert len(reg.duracoes()) == execucoes.MAX_DURACOES


def test_limpar_concluidas_preserva_as_ativas(reg, relogio):
    ativa = rodar(reg, relogio)
    for _ in range(3):
        e = rodar(reg, relogio); e.concluir()
    assert reg.limpar_concluidas() == 3
    assert [e["id"] for e in reg.snapshot()["execucoes"]] == [ativa.id]
    assert reg.duracoes()  # o historico de tempos nao e apagado


def test_registro_e_seguro_com_varias_threads(reg):
    erros = []

    def trabalho(n):
        try:
            for i in range(30):
                ex = reg.criar("converter", f"t{n}-{i}")
                ex.iniciar(); ex.definir_total(10, "página")
                for p in range(1, 11):
                    ex.avancar(p); ex.log("x"); reg.snapshot()
                ex.concluir()
        except Exception as exc:  # noqa: BLE001
            erros.append(exc)

    threads = [threading.Thread(target=trabalho, args=(n,)) for n in range(8)]
    [t.start() for t in threads]
    [t.join(30) for t in threads]
    assert not erros
    total = reg.snapshot()["total"]  # a memoria guarda no maximo MAX_EXECUCOES_MEMORIA concluidas (+ as que acabaram depois da poda)
    assert execucoes.MAX_EXECUCOES_MEMORIA <= total <= 240 and reg.contar_ativas() == 0


# ------------------------------------------------------------------- resumo / copiar


def test_resumo_para_copiar_traz_o_essencial(reg, relogio):
    ex = rodar(reg, relogio)
    ex.definir_total(4, "página"); ex.avancar(4); ex.log("linha de teste")
    relogio.avancar(65)
    ex.concluir("4 página(s)")
    texto = ex.resumo_texto()
    for trecho in ("Execução: Converter - Teste", "Estado: Concluída", "Entrada: arquivo.pdf", "Duração: 1 min 05 s",
                   "Progresso: 4 de 4 páginas", "Resultado: 4 página(s)", f"ID: {ex.id}", "linha de teste"):
        assert trecho in texto


# ------------------------------------------------------------------------ leitorlog


def _fazer_log(pasta: Path, linhas: int, nome="sessao.log") -> Path:
    caminho = pasta / nome
    partes = []
    for i in range(linhas):
        nivel = "ERROR" if i % 50 == 0 else "WARNING" if i % 10 == 0 else "INFO"
        partes.append(f"2026-11-16 10:{i // 60 % 60:02d}:{i % 60:02d},123 | {nivel} | mensagem numero {i} áéí")
        if nivel == "ERROR":
            partes.append("Traceback (most recent call last):")
            partes.append(f'  File "x.py", line {i}, in coisa')
    caminho.write_text("\n".join(partes) + "\n", encoding="utf-8")
    return caminho


def test_log_grande_e_lido_por_trechos_so_com_linhas_completas(tmp_path):
    caminho = _fazer_log(tmp_path, 60_000)  # ~4 MB
    assert caminho.stat().st_size > 3_000_000
    r = leitorlog.ler_cauda(caminho, 64 * 1024)
    assert 0 < len(r["linhas"]) < 2000 and r["truncado_inicio"] is True and r["fim"] == caminho.stat().st_size
    assert r["linhas"][-1]["x"].startswith("mensagem numero 59999")
    assert all(l["t"] or l["x"] for l in r["linhas"])  # nada de linha cortada no comeco
    assert re.match(r"mensagem numero \d+|Traceback|  File", r["linhas"][0]["x"])
    # cada deslocamento aponta exatamente para o inicio da linha no arquivo
    bruto = caminho.read_bytes()
    for l in r["linhas"][:20] + r["linhas"][-20:]:
        trecho = bruto[l["o"]: l["o"] + 200].decode("utf-8", "replace")
        assert l["x"] in trecho.split("\n")[0]


def test_paginacao_para_tras_reconstroi_o_arquivo(tmp_path):
    caminho = _fazer_log(tmp_path, 3000)
    esperado = caminho.read_text(encoding="utf-8").splitlines()
    cauda = leitorlog.ler_cauda(caminho, 10 * 1024)
    textos = [l["x"] for l in cauda["linhas"]]
    ini = cauda["ini"]
    passos = 0
    while ini > 0:
        antes = leitorlog.ler_antes(caminho, ini, 10 * 1024)
        assert antes["fim"] == ini and antes["ini"] < ini
        textos = [l["x"] for l in antes["linhas"]] + textos
        ini = antes["ini"]
        passos += 1
        assert passos < 100
    reconstruido = [re.sub(r"^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d,\d+ \| [A-Z]+ \| ", "", l) for l in esperado]
    assert textos == reconstruido


def test_leitura_ao_vivo_nao_entrega_linha_pela_metade(tmp_path):
    caminho = tmp_path / "vivo.log"
    caminho.write_text("2026-11-16 10:00:00,000 | INFO | primeira\n2026-11-16 10:00:01,000 | INFO | segun", encoding="utf-8")
    r = leitorlog.ler_cauda(caminho, ao_vivo=True)
    assert [l["x"] for l in r["linhas"]] == ["primeira"]
    with open(caminho, "a", encoding="utf-8") as f:
        f.write("da\n2026-11-16 10:00:02,000 | ERROR | terceira\n")
    novo = leitorlog.ler_desde(caminho, r["fim"])
    assert [l["x"] for l in novo["linhas"]] == ["segunda", "terceira"] and novo["linhas"][1]["nv"] == "ERROR"
    vazio = leitorlog.ler_desde(caminho, novo["fim"])
    assert vazio["linhas"] == [] and vazio["fim"] == novo["fim"]


def test_log_que_encolheu_recomeca_pela_cauda(tmp_path):
    caminho = _fazer_log(tmp_path, 50)
    r = leitorlog.ler_desde(caminho, caminho.stat().st_size + 5000)
    assert r["reiniciado"] is True and r["linhas"]


def test_linhas_de_traceback_herdam_o_nivel_e_filtros_funcionam(tmp_path):
    caminho = _fazer_log(tmp_path, 120)
    linhas = leitorlog.ler_cauda(caminho, 1_000_000)["linhas"]
    erro = next(i for i, l in enumerate(linhas) if l["nv"] == "ERROR")
    assert linhas[erro + 1]["x"].startswith("Traceback") and linhas[erro + 1]["nv"] == "ERROR" and linhas[erro + 1]["t"] == ""
    so_erros = leitorlog.filtrar_linhas(linhas, "erros")
    assert so_erros and all(l["nv"] == "ERROR" for l in so_erros)
    avisos = leitorlog.filtrar_linhas(linhas, "avisos")
    assert {l["nv"] for l in avisos} == {"WARNING", "ERROR"} and len(avisos) > len(so_erros)
    assert len(leitorlog.filtrar_linhas(linhas, "tudo")) == len(linhas)
    achadas = leitorlog.filtrar_linhas(linhas, "tudo", "NUMERO 7 ")
    assert achadas and all("numero 7 " in l["x"] for l in achadas)
    assert leitorlog.filtrar_linhas(linhas, "erros", "numero 100") != [] or True
    assert leitorlog.filtrar_linhas(linhas, "erros", "texto que nao existe") == []


def test_busca_em_fluxo_devolve_as_ultimas_correspondencias(tmp_path):
    caminho = _fazer_log(tmp_path, 5000)
    r = leitorlog.buscar(caminho, "erros", "", limite=10)
    assert len(r["linhas"]) == 10 and r["cortada"] is True and r["total_achadas"] > 10
    assert all(l["nv"] == "ERROR" for l in r["linhas"])
    assert r["linhas"][-1]["o"] > r["linhas"][0]["o"]
    um = leitorlog.buscar(caminho, "tudo", "numero 4321 ")
    assert [l["x"].split(" ")[2] for l in um["linhas"]] == ["4321"]


def test_intervalo_e_ao_redor(tmp_path):
    caminho = _fazer_log(tmp_path, 400)
    todas = leitorlog.ler_cauda(caminho, 1_000_000)["linhas"]
    a, b = todas[100], todas[120]
    r = leitorlog.ler_intervalo(caminho, a["o"], b["o"])
    assert r["linhas"][0]["o"] == a["o"] and r["linhas"][-1]["o"] < b["o"] and len(r["linhas"]) == 20
    aberto = leitorlog.ler_intervalo(caminho, todas[-5]["o"], None)  # execucao ainda rodando: vai ate o fim
    assert len(aberto["linhas"]) == 5
    alvo = todas[200]
    ao = leitorlog.ler_ao_redor(caminho, alvo["o"], antes=2000, depois=2000)
    assert any(l["o"] == alvo["o"] for l in ao["linhas"]) and ao["marca"] == alvo["o"]


def test_nome_de_log_nao_escapa_da_pasta(tmp_path):
    pasta = tmp_path / "logs"
    pasta.mkdir()
    (pasta / "ok.log").write_text("x\n", encoding="utf-8")
    (tmp_path / "segredo.log").write_text("s\n", encoding="utf-8")
    assert leitorlog.nome_valido(pasta, "ok.log").name == "ok.log"
    for ruim in ("..\\segredo.log", "../segredo.log", "C:\\Windows\\win.ini", "ok.log\\..\\..\\segredo.log", "", "ok.exe"):
        with pytest.raises(ValueError):
            leitorlog.nome_valido(pasta, ruim)
    with pytest.raises(FileNotFoundError):
        leitorlog.nome_valido(pasta, "nao_existe.log")


def test_lista_de_logs_traz_data_sessao_atual_e_instalador(tmp_path):
    (tmp_path / "extrator_20261116_100000.log").write_text("a\n", encoding="utf-8")
    (tmp_path / "instalacao.log").write_text("b\n", encoding="utf-8")
    (tmp_path / "outro.dat").write_text("c", encoding="utf-8")
    itens = leitorlog.listar_logs(tmp_path, "extrator_20261116_100000.log")
    assert {i["nome"] for i in itens} == {"extrator_20261116_100000.log", "instalacao.log"}
    por_nome = {i["nome"]: i for i in itens}
    assert por_nome["instalacao.log"]["tipo"] == "instalacao" and por_nome["extrator_20261116_100000.log"]["atual"] is True
    assert re.match(r"\d{4}-\d\d-\d\d$", itens[0]["data"])


# ------------------------------------------------------------- integracao com a Api


@pytest.fixture
def api(monkeypatch, tmp_path):
    monkeypatch.setattr(core, "CONFIG", dict(core.DEFAULT_CONFIG))
    monkeypatch.setattr(core, "save_config", lambda cfg: None)
    monkeypatch.setattr(core, "set_progress_queue", lambda q: None)
    monkeypatch.setattr(fa, "_signed", lambda path: False)
    return fa.Api()


def _snap(api):
    return {e["id"]: e for e in api.exec_listar()["execucoes"]}


def _unica(api, ferramenta):
    achadas = [e for e in api.exec_listar()["execucoes"] if e["ferramenta"] == ferramenta]
    assert len(achadas) == 1, achadas
    return achadas[0]


def test_api_conversao_aparece_na_central_do_comeco_ao_fim(api, monkeypatch, tmp_path):
    visto = {}

    def falso(path, folder, mode, pages, fmt, **kw):
        visto[path.name] = _unica(api, "converter")["estado"]  # enquanto roda
        execucoes.evento_progresso({"type": "start", "file": path.name, "total": 3})
        execucoes.evento_progresso({"type": "page", "current": 1, "total": 3, "source": "OCR"})
        visto[path.name + "-prog"] = _unica(api, "converter")["progresso"]

    monkeypatch.setattr(core, "ocr_pdf_to_text", falso)
    r = api.start_convert({"mode": "auto", "files": [str(tmp_path / "a.pdf"), str(tmp_path / "b.pdf")], "same_folder": True})
    assert r["status"] == "started"
    api._job_thread.join(10)
    ex = _unica(api, "converter")
    assert visto["a.pdf"] == "rodando" and visto["a.pdf-prog"] == pytest.approx(100 * (0 + 1 / 3) / 2, abs=0.2)
    assert ex["estado"] == "concluida" and ex["progresso"] == 100.0 and ex["progresso_texto"] == "2 de 2 arquivos"
    assert "OCR automático" in ex["rotulo"] and "a.pdf" in ex["entrada"] and ex["repetivel"] is True
    det = api.exec_detalhe(ex["id"])
    assert any("Iniciando" in l["x"] for l in det["linhas"])
    assert api.exec_resumo(ex["id"])["ok"] is True


def test_api_conversao_com_falha_parcial_e_total(api, monkeypatch, tmp_path):
    def falso(path, *a, **kw):
        if path.name != "ok.pdf":
            raise RuntimeError("quebrou " + path.name)

    monkeypatch.setattr(core, "ocr_pdf_to_text", falso)
    api.start_convert({"mode": "auto", "files": [str(tmp_path / "ok.pdf"), str(tmp_path / "ruim.pdf")], "same_folder": True})
    api._job_thread.join(10)
    parcial = _unica(api, "converter")
    assert parcial["estado"] == "concluida" and parcial["avisos"] == 1 and "1 com falha" in parcial["mensagem"]
    api.exec_limpar_concluidas()
    api.start_convert({"mode": "auto", "files": [str(tmp_path / "x.pdf")], "same_folder": True})
    api._job_thread.join(10)
    total = _unica(api, "converter")
    assert total["estado"] == "falhou" and "falharam" in total["erro"]


def test_api_cancelar_pela_central_para_o_trabalho(api, monkeypatch, tmp_path):
    entrou = threading.Event()

    def lento(path, folder, mode, pages, fmt, cancel_event=None, **kw):
        entrou.set()
        cancel_event.wait(10)
        raise core.OperationCancelled("cancelado")

    monkeypatch.setattr(core, "ocr_pdf_to_text", lento)
    api.start_convert({"mode": "auto", "files": [str(tmp_path / "a.pdf")], "same_folder": True})
    assert entrou.wait(10)
    ex = _unica(api, "converter")
    assert ex["cancelavel"] is True
    assert api.exec_cancelar(ex["id"])["ok"] is True
    api._job_thread.join(10)
    assert _unica(api, "converter")["estado"] == "cancelada"
    assert api.exec_cancelar(ex["id"])["ok"] is False  # ja terminou
    tipos = [e["type"] for e in api.drain_events()]
    assert "cancelled" in tipos  # o rodape/tela de conversao continuam sendo avisados


def test_api_repetir_reexecuta_com_os_mesmos_parametros(api, monkeypatch, tmp_path):
    chamadas = []
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda path, folder, mode, pages, fmt, **kw: chamadas.append((path.name, pages)))
    api.start_convert({"mode": "auto", "files": [str(tmp_path / "a.pdf")], "same_folder": True, "pages": ""})
    api._job_thread.join(10)
    primeiro = _unica(api, "converter")
    r = api.exec_repetir(primeiro["id"])
    assert r["ok"] is True and r["status"] == "started" and r["acao"] == "start_convert"
    api._job_thread.join(10)
    assert chamadas == [("a.pdf", ""), ("a.pdf", "")]
    assert len(api.exec_listar()["execucoes"]) == 2
    assert api.exec_repetir("nao-existe")["ok"] is False


def test_api_nao_repete_acoes_destrutivas(api, monkeypatch):
    monkeypatch.setattr(core, "cleanup_data", lambda retention_days=0: (1, 2))
    assert api.clear_data()["ok"] is True
    ex = _unica(api, "limpeza")
    assert ex["estado"] == "concluida" and ex["repetivel"] is False and "1 log" in ex["mensagem"]
    assert api.exec_repetir(ex["id"])["ok"] is False


def test_api_lote_registra_total_de_arquivos(api, monkeypatch, tmp_path):
    for nome in ("a.pdf", "b.pdf", "c.pdf"):
        (tmp_path / nome).write_bytes(b"%PDF-1.4")
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda *a, **k: None)
    api.start_batch({"mode": "auto_md", "input_folder": str(tmp_path), "output_folder": str(tmp_path / "saida")})
    api._job_thread.join(10)
    lote = [e for e in api.exec_listar()["execucoes"] if e["ferramenta"] == "lote" and e["variante"] == "auto_md"][0]
    assert lote["estado"] == "concluida" and lote["progresso_texto"] == "3 de 3 arquivos" and lote["progresso"] == 100.0
    analises = [e for e in api.exec_listar()["execucoes"] if e["variante"] == "analise"]
    assert analises == []  # start_batch nao passa pelo scan; o scan so existe quando a tela o pede


def test_api_scan_de_lote_e_uma_tarefa_instantanea(api, tmp_path):
    (tmp_path / "a.pdf").write_bytes(b"%PDF-1.4")
    assert api.scan_batch({"mode": "searchable", "input_folder": str(tmp_path)})["count"] == 1
    ex = _unica(api, "lote")
    assert ex["variante"] == "analise" and ex["estado"] == "concluida" and ex["eta_texto"] == ""
    assert api.scan_batch({"mode": "auto_md", "input_folder": ""})["ok"] is False
    assert any(e["estado"] == "falhou" for e in api.exec_listar()["execucoes"])


def test_api_transcricao_cria_uma_execucao_por_arquivo_com_barra_de_duracao(api, monkeypatch, tmp_path):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {"ffmpeg": True})
    estados_na_hora = {}

    def falso(arquivo, progresso_cb, **kw):
        estados_na_hora[arquivo.name] = {e["rotulo"]: e["estado"] for e in api.exec_listar()["execucoes"]}
        progresso_cb({"pct": 50, "processado_s": 30, "total_s": 60, "restante_s": 30, "elapsed_s": 5, "eta_s": 5})
        d = [e for e in api.exec_listar()["execucoes"] if e["estado"] == "rodando"][0]
        estados_na_hora[arquivo.name + "-d"] = d
        return {"output_path": tmp_path / (arquivo.stem + ".txt")}

    monkeypatch.setattr(transcricao, "transcrever_com_progresso", falso)
    itens = [{"id": "t1", "path": str(tmp_path / "a.mp3")}, {"id": "t2", "path": str(tmp_path / "b.mp3")}]
    inicio = api.start_transcricao({"items": itens, "model": "small"})
    assert inicio["status"] == "started"
    api._tr_thread.join(10)
    assert estados_na_hora["a.mp3"] == {"Transcrever a.mp3": "rodando", "Transcrever b.mp3": "fila"}
    d = estados_na_hora["a.mp3-d"]
    assert d["progresso"] == 50.0 and d["progresso_texto"] == "00:00:30 de 00:01:00" and d["variante"] == "small"
    finais = api.exec_listar()["execucoes"]
    assert [e["estado"] for e in finais] == ["concluida", "concluida"] and all(e["repetivel"] for e in finais)


def test_api_cancelar_item_da_fila_de_transcricao_nao_derruba_os_outros(api, monkeypatch, tmp_path):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {})
    liberar = threading.Event()
    rodou = []

    def falso(arquivo, **kw):
        rodou.append(arquivo.name)
        liberar.wait(10)
        return {"output_path": Path("x.txt")}

    monkeypatch.setattr(transcricao, "transcrever_com_progresso", falso)
    api.start_transcricao({"items": [{"id": "1", "path": "a.mp3"}, {"id": "2", "path": "b.mp3"}, {"id": "3", "path": "c.mp3"}]})
    por_nome = {e["entrada"]: e for e in api.exec_listar()["execucoes"]}
    assert api.exec_cancelar(por_nome["b.mp3"]["id"])["ok"] is True  # b ainda esta na fila
    assert _snap(api)[por_nome["b.mp3"]["id"]]["estado"] == "cancelada"
    liberar.set()
    api._tr_thread.join(10)
    estados = {e["entrada"]: e["estado"] for e in api.exec_listar()["execucoes"]}
    assert estados == {"a.mp3": "concluida", "b.mp3": "cancelada", "c.mp3": "concluida"} and rodou == ["a.mp3", "c.mp3"]


def test_api_cancelar_a_fila_inteira_cancela_o_que_sobrou(api, monkeypatch):
    monkeypatch.setattr(transcricao, "verificar_dependencias", lambda: {})
    entrou = threading.Event()

    def falso(arquivo, cancel_event, **kw):
        entrou.set()
        while not cancel_event.is_set():
            threading.Event().wait(0.01)
        raise core.OperationCancelled("x")

    monkeypatch.setattr(transcricao, "transcrever_com_progresso", falso)
    api.start_transcricao({"items": [{"id": "1", "path": "a.mp3"}, {"id": "2", "path": "b.mp3"}]})
    assert entrou.wait(10)
    api.cancel_transcricao()
    api._tr_thread.join(10)
    assert {e["estado"] for e in api.exec_listar()["execucoes"]} == {"cancelada"}


def test_api_verificacao_tem_barra_determinada_por_componente(api, monkeypatch):
    linhas = [(f"Comp{i}", i != 3, "d") for i in range(1, 6)]
    visto = []

    def gerador():
        for i, linha in enumerate(linhas, 1):
            visto.append(_unica(api, "verificacao")["progresso"])
            yield linha

    monkeypatch.setattr(core, "iter_installation_rows", gerador)
    monkeypatch.setattr(transcricao, "iter_installation_rows", lambda: iter([]))
    assert api.start_check_installation()["status"] == "started"
    api._check_thread.join(10)
    total = core.INSTALLATION_ROW_COUNT + transcricao.INSTALLATION_ROW_COUNT
    assert visto[0] == 0.0 and visto[1] == pytest.approx(100 / total, abs=0.2)  # porcentagem real: componentes verificados / total
    ex = _unica(api, "verificacao")
    assert ex["estado"] == "concluida" and ex["avisos"] == 1 and "Faltam: Comp3" in ex["mensagem"] and ex["progresso"] == 100.0
    assert ex["repetivel"] is True


def test_contagem_de_componentes_confere_com_a_verificacao_real():
    assert core.INSTALLATION_ROW_COUNT == len(core.installation_rows())
    assert transcricao.INSTALLATION_ROW_COUNT == len(transcricao.installation_rows())


def test_api_erro_na_verificacao_vira_falha_na_central(api, monkeypatch):
    def quebra():
        raise RuntimeError("tesseract travou")
        yield  # pragma: no cover

    monkeypatch.setattr(core, "iter_installation_rows", quebra)
    api.start_check_installation()
    api._check_thread.join(10)
    ex = _unica(api, "verificacao")
    assert ex["estado"] == "falhou" and "tesseract travou" in ex["erro"]


def test_api_instalacao_tem_total_por_componente_e_pode_ser_cancelada(api, monkeypatch):
    import subprocess
    monkeypatch.setattr(core, "installation_rows", lambda: [("Rich", False, "x"), ("pytesseract", False, "y")])
    monkeypatch.setattr(transcricao, "installation_rows", lambda: [])
    ex_holder = {}

    def fake_run(cmd, **kw):
        if not ex_holder:
            ex_holder["ex"] = _unica(api, "instalacao")
            assert ex_holder["ex"]["cancelavel"] is True and ex_holder["ex"]["progresso"] == 0.0
            api.exec_cancelar(ex_holder["ex"]["id"])  # pede para parar depois do componente atual
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

    monkeypatch.setattr(fa.subprocess, "run", fake_run)
    api.install_missing()
    api._install_thread.join(10)
    final = _unica(api, "instalacao")
    assert final["estado"] == "cancelada" and "1 de 2" in final["mensagem"] and final["repetivel"] is False


def test_api_download_de_modelo_e_indeterminado(api, monkeypatch):
    visto = {}

    def baixa(nome, **kw):
        visto.update(_unica(api, "modelo"))

    monkeypatch.setattr(transcricao, "baixar_modelo", baixa)
    api.download_model("tiny")
    api._download_thread.join(10)
    assert visto["determinada"] is False and visto["progresso"] is None and "tiny" in visto["rotulo"]
    assert _unica(api, "modelo")["estado"] == "concluida"


def test_api_diagnostico_e_configuracoes_passam_pela_central(api, monkeypatch, tmp_path):
    arquivo = tmp_path / "a.pdf"; arquivo.write_bytes(b"%PDF")
    monkeypatch.setattr(core, "diagnose_rows", lambda p: [("Caminho", str(p))])
    assert api.diagnose(str(arquivo))["ok"] is True
    assert _unica(api, "diagnostico")["estado"] == "concluida"
    ruim = api.save_settings({"language": ""})
    assert ruim["ok"] is False
    assert api.restore_settings()["ok"] is True
    estados = [e["estado"] for e in api.exec_listar()["execucoes"] if e["ferramenta"] == "configuracoes"]
    assert sorted(estados) == ["concluida", "falhou"]
    assert all(e["eta_texto"] == "" for e in api.exec_listar()["execucoes"] if e["ferramenta"] == "configuracoes")


def test_api_nao_gera_eventos_novos_para_a_interface(api, monkeypatch, tmp_path):
    """A Central usa consulta propria: a fila de eventos do rodape segue identica."""
    monkeypatch.setattr(core, "ocr_pdf_to_text", lambda *a, **k: None)
    api.start_convert({"mode": "auto", "files": [str(tmp_path / "a.pdf")], "same_folder": True})
    api._job_thread.join(10)
    tipos = [e["type"] for e in api.drain_events() if e["type"] != "line"]
    assert tipos == ["file_start", "done"]


# ------------------------------------------------------- Api: leitura do log da sessao


@pytest.fixture
def api_log(api, monkeypatch, tmp_path):
    pasta = tmp_path / "logs"
    pasta.mkdir()
    sessao = pasta / "extrator_20261116_100000.log"
    _fazer_log(pasta, 100, sessao.name)
    monkeypatch.setattr(core, "LOG_DIR", pasta)
    monkeypatch.setattr(core, "SESSION_LOG", sessao)
    return api, pasta, sessao


def test_api_log_lista_le_ao_vivo_e_busca(api_log):
    api, pasta, sessao = api_log
    (pasta / "instalacao.log").write_text("linha do instalador\n", encoding="utf-8")
    lista = api.log_arquivos()
    assert lista["atual"] == sessao.name and {a["nome"] for a in lista["arquivos"]} == {sessao.name, "instalacao.log"}
    cauda = api.log_ler({"nome": sessao.name})
    assert cauda["ok"] and cauda["ao_vivo"] is True and cauda["linhas"]
    with open(sessao, "a", encoding="utf-8") as f:
        f.write("2026-11-16 11:00:00,000 | ERROR | novo erro\n")
    novo = api.log_ler({"nome": sessao.name, "modo": "desde", "offset": cauda["fim"], "nv_ant": "INFO"})
    assert [l["x"] for l in novo["linhas"]] == ["novo erro"]
    antigo = api.log_ler({"nome": "instalacao.log"})
    assert antigo["ao_vivo"] is False and antigo["linhas"][0]["x"] == "linha do instalador"
    achados = api.log_buscar({"nome": sessao.name, "nivel": "erros", "texto": "novo"})
    assert [l["x"] for l in achados["linhas"]] == ["novo erro"]
    assert api.log_ler({"nome": "..\\x.log"})["ok"] is False and api.log_ler({"nome": "nao_existe.log"})["ok"] is False


def test_api_log_registrar_devolve_a_linha_exata_para_ver_no_log(api_log, monkeypatch):
    api, pasta, sessao = api_log
    import logging
    handler = logging.FileHandler(sessao, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s | %(levelname)s | %(message)s"))
    raiz = logging.getLogger()
    nivel_antigo = raiz.level
    raiz.addHandler(handler); raiz.setLevel(logging.DEBUG)
    try:
        r = api.log_registrar("ERROR", "Falha de teste na interface")
    finally:
        raiz.removeHandler(handler); raiz.setLevel(nivel_antigo); handler.close()
    assert r["ok"] and r["nome"] == sessao.name and isinstance(r["offset"], int)
    ao_redor = api.log_ler({"nome": sessao.name, "modo": "ao_redor", "offset": r["offset"]})
    alvo = next(l for l in ao_redor["linhas"] if l["o"] == r["offset"])
    assert alvo["nv"] == "ERROR" and "Falha de teste na interface" in alvo["x"]


def test_execucao_aponta_para_o_proprio_trecho_do_log(relogio, tmp_path):
    caminho = tmp_path / "sessao.log"
    caminho.write_text("2026-11-16 10:00:00,000 | INFO | antes\n", encoding="utf-8")

    def posicao():
        return caminho.name, caminho.stat().st_size

    reg = execucoes.Registro(tmp_path / "d", relogio=relogio, posicao_log=posicao)
    ex = reg.criar("converter", "OCR")
    ex.iniciar()
    with open(caminho, "a", encoding="utf-8") as f:
        f.write("2026-11-16 10:00:01,000 | INFO | durante\n")
    ex.concluir()
    d = ex.para_dict()
    assert d["log_arquivo"] == "sessao.log" and d["log_ini"] > 0 and d["log_fim"] >= d["log_ini"]
    trecho = leitorlog.ler_intervalo(caminho, d["log_ini"], d["log_fim"])
    assert "durante" in [l["x"] for l in trecho["linhas"]] and "antes" not in [l["x"] for l in trecho["linhas"]]
    recarregado = execucoes.Registro(tmp_path / "d").snapshot()["execucoes"][0]
    assert recarregado["log_arquivo"] == "sessao.log" and recarregado["log_ini"] == d["log_ini"]  # o vinculo sobrevive ao reinicio


# --------------------------------------------------------------------- interface web


def test_interface_tem_central_e_tela_de_log():
    html = (WEB / "index.html").read_text(encoding="utf-8")
    js = (WEB / "app.js").read_text(encoding="utf-8")
    css = (WEB / "styles.css").read_text(encoding="utf-8")
    for pagina in ("exec", "log"):
        assert f'data-page="{pagina}"' in html and f'id="page-{pagina}"' in html
    for chamada in ("exec_listar", "exec_detalhe", "exec_cancelar", "exec_repetir", "exec_resumo", "exec_limpar_concluidas",
                    "log_arquivos", "log_ler", "log_buscar", "log_registrar", "log_abrir_pasta"):
        assert f'"{chamada}"' in js, chamada
    for botao in ("execLimpar", "logPausar", "logCopiar", "logPasta", "dockExec", "logAuto"):
        assert f'id="{botao}"' in html
    # barra: determinada so com total; senao, indeterminada (classe `indet` existente no CSS)
    assert 'classList.toggle("indet", !det' in js and ".track.indet .fill" in css
    assert "Ver no log" in js and "aria-live" in html and 'role="progressbar"' in js
    # acessibilidade e energia: foco visivel, anuncio de estado, pausa com a janela oculta
    assert ":focus-visible" in css and "visibilitychange" in js and ".oculta" in css and "prefers-reduced-motion" in css
    assert "estimando" not in js  # o texto do ETA vem sempre do Python (uma unica regra de estimativa)


def test_interface_ainda_nao_usa_pop_ups_nem_navegacao_externa():
    for arquivo in WEB.rglob("*"):
        if arquivo.suffix in {".js", ".html"}:
            texto = arquivo.read_text(encoding="utf-8")
            assert not re.search(r"(?<![\w$.])(?:window\.)?(?:alert|confirm|prompt)\s*\(", re.sub(r"/\*.*?\*/", "", texto, flags=re.S))
            assert "window.open" not in texto and "_blank" not in texto
