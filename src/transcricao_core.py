"""
transcricao_core.py - Nucleo de transcricao de audio/video para texto.

Usa faster-whisper (Whisper local, sem enviar audio a servicos externos),
seguindo a mesma filosofia de "tudo local" do motor de OCR (extrator_core.py).
Integrado ao Extrator de Texto: reaproveita a mesma pasta autocontida do programa
(dados/transcricao/ para o cache de modelos). O ffmpeg nao e mais um .exe
empacotado manualmente: e obtido via pip (pacote static-ffmpeg), que baixa o
binario oficial na primeira execucao - instalacao 100% via Python/pip.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Callable, Optional

import extrator_core as core
import execucoes

TRANSCRICAO_DATA_DIR = core.APP_DATA_DIR / "transcricao"
MODELOS_DIR = TRANSCRICAO_DATA_DIR / "modelos"
TEMP_DIR = TRANSCRICAO_DATA_DIR / "temp"

MODELOS_DISPONIVEIS = {
    "tiny": {"tamanho_mb": 76, "precisao": "Baixa", "velocidade": "Muito rapida", "descricao": "Para experimentacao rapida"},
    "base": {"tamanho_mb": 142, "precisao": "Razoavel", "velocidade": "Rapida", "descricao": "Equilibrio basico"},
    "small": {"tamanho_mb": 460, "precisao": "Boa", "velocidade": "Moderada", "descricao": "Melhor equilibrio (padrao)"},
    "medium": {"tamanho_mb": 1500, "precisao": "Muito boa", "velocidade": "Lenta", "descricao": "Alta precisao para PT-BR"},
    "large-v3": {"tamanho_mb": 3000, "precisao": "Excelente", "velocidade": "Muito lenta", "descricao": "Maxima precisao (CPU pesado)"},
}
MODELO_PADRAO = "small"

IDIOMAS = ["pt", "en", "es", "fr", "de", "ja", "ko", "auto"]
FORMATOS_SAIDA = ["txt", "md", "srt", "json"]
VIDEO_EXTS = {".mp4", ".mkv", ".mov", ".avi", ".webm", ".flv", ".wmv", ".m4v"}
AUDIO_EXTS = {".mp3", ".wav", ".m4a", ".ogg", ".flac", ".aac", ".wma"}
ENTRADA_EXTS = AUDIO_EXTS | VIDEO_EXTS

# Mapeia nomes de linha de installation_rows() para o especificador pip
# correspondente (mesmo mecanismo de extrator_core.PIP_REQUIREMENTS).
PIP_REQUIREMENTS = {
    "faster-whisper": "faster-whisper==1.2.1",
    "soundfile": "soundfile==0.14.0",
    "ffmpeg (transcricao)": "static-ffmpeg>=2.5",
}


def format_hhmmss(segundos: float) -> str:
    """Formata segundos como HH:MM:SS. Usado em toda a interface (log,
    progresso, estimativas) conforme preferencia do usuario - o formato
    'SRT' com milissegundos so e usado dentro do proprio arquivo .srt."""
    segundos = max(0, int(round(segundos)))
    horas, resto = divmod(segundos, 3600)
    minutos, seg = divmod(resto, 60)
    return f"{horas:02d}:{minutos:02d}:{seg:02d}"


# ============================================================
# Dependencias e caminhos
# ============================================================

_ffmpeg_paths_added = False
_ffmpeg_lock = threading.Lock()
_ffmpeg_falha_em: Optional[float] = None
FFMPEG_NOVA_TENTATIVA_S = 30  # depois de uma falha (sem internet), so tenta de novo apos este intervalo


def _ffmpeg_ja_baixado(static_ffmpeg_run) -> bool:
    """True quando o static-ffmpeg ja tem o binario no disco (nada a baixar)."""
    try:
        return (Path(static_ffmpeg_run.get_platform_dir()) / "installed.crumb").exists()
    except Exception:
        return False


def _baixador_com_progresso(ex, cancel: threading.Event):
    """Substitui static_ffmpeg.run.download_file: baixa em blocos reportando os
    bytes a execucao `ex` (nada vai para stdout/console)."""

    def download_file(url: str, local_path: str) -> str:
        import requests

        ex.log(f"Baixando {url}")
        try:
            with requests.get(url, stream=True, timeout=(15, 60)) as resp:
                resp.raise_for_status()
                try:
                    total = int(resp.headers.get("content-length") or 0)
                except ValueError:
                    total = 0
                if total > 0:
                    ex.definir_total(total, "byte", "bytes")
                feito = 0
                with open(local_path, "wb") as arquivo:
                    for bloco in resp.iter_content(256 * 1024):
                        if cancel.is_set():
                            raise core.OperationCancelled("Download do ffmpeg cancelado.")
                        arquivo.write(bloco)
                        feito += len(bloco)
                        if total > 0:
                            ex.avancar(feito)
        except BaseException:
            try:
                os.remove(local_path)  # nao deixa um .zip pela metade
            except OSError:
                pass
            raise
        ex.definir_etapa("Extraindo o ffmpeg", registrar=True)
        return local_path

    return download_file


def _baixar_ffmpeg(static_ffmpeg) -> None:
    """Primeiro uso: o download (~72 MB) vira uma execucao visivel na Central."""
    from static_ffmpeg import run as sf_run

    reg = execucoes.REGISTRO
    ex = reg.criar("instalacao", "Baixando ffmpeg", "static-ffmpeg (~72 MB, só na primeira vez)", variante="ffmpeg")
    cancel = threading.Event()
    ex.definir_cancelamento(cancel.set)
    original = sf_run.download_file
    with reg.executar(ex):
        ex.definir_etapa("Baixando ffmpeg", registrar=True)
        sf_run.download_file = _baixador_com_progresso(ex, cancel)
        try:
            static_ffmpeg.add_paths()
        except core.OperationCancelled:
            ex.cancelar("Download do ffmpeg cancelado.")
            raise
        finally:
            sf_run.download_file = original
        ex.concluir("ffmpeg pronto.")


def _garantir_ffmpeg_no_path() -> None:
    """Adiciona ao PATH o ffmpeg/ffprobe obtidos via pip (pacote static-ffmpeg) -
    nada de .exe empacotado manualmente no repositorio nem instalado via winget;
    e apenas mais uma dependencia Python. Na primeira vez o pacote baixa o
    binario: isso acontece como execucao visivel ("Baixando ffmpeg"). Se falhar
    (sem internet), uma nova tentativa so ocorre depois de FFMPEG_NOVA_TENTATIVA_S."""
    global _ffmpeg_paths_added, _ffmpeg_falha_em
    if _ffmpeg_paths_added:
        return
    with _ffmpeg_lock:
        if _ffmpeg_paths_added:
            return
        if _ffmpeg_falha_em is not None and time.monotonic() - _ffmpeg_falha_em < FFMPEG_NOVA_TENTATIVA_S:
            return
        try:
            import static_ffmpeg
            from static_ffmpeg import run as sf_run
        except Exception:
            _ffmpeg_paths_added = True  # pacote ausente: nada a fazer
            return
        try:
            if _ffmpeg_ja_baixado(sf_run):
                static_ffmpeg.add_paths()
            else:
                _baixar_ffmpeg(static_ffmpeg)
            _ffmpeg_paths_added = True
        except Exception:
            _ffmpeg_falha_em = time.monotonic()


def verificar_ffmpeg() -> Optional[str]:
    _garantir_ffmpeg_no_path()
    return shutil.which("ffmpeg")


def verificar_ffprobe() -> Optional[str]:
    _garantir_ffmpeg_no_path()
    return shutil.which("ffprobe")


def verificar_dependencias() -> dict:
    deps = {"ffmpeg": verificar_ffmpeg() is not None}
    try:
        import faster_whisper  # noqa: F401
        deps["faster_whisper"] = True
    except ImportError:
        deps["faster_whisper"] = False
    try:
        import soundfile  # noqa: F401
        deps["soundfile"] = True
    except ImportError:
        deps["soundfile"] = False
    return deps


def modelos_instalados() -> list[str]:
    """Modelos cujo cache local ja esta completo (model.bin presente).

    So esses aparecem como opcao pronta na interface - selecionar um modelo
    nao baixado ainda dispararia um download de dezenas a milhares de MB sem
    aviso previo, o que nao condiz com a proposta de ferramenta local.
    """
    instalados = []
    if not MODELOS_DIR.exists():
        return instalados
    for nome in MODELOS_DISPONIVEIS:
        pasta_modelo = MODELOS_DIR / f"models--Systran--faster-whisper-{nome}"
        if not pasta_modelo.exists():
            continue
        for model_bin in pasta_modelo.glob("snapshots/*/model.bin"):
            if model_bin.stat().st_size > 0:
                instalados.append(nome)
                break
    return instalados


INSTALLATION_ROW_COUNT = 4  # linhas de iter_installation_rows (total conhecido da verificacao)


def iter_installation_rows():
    """Gera, uma a uma, as linhas (componente, ok, detalhes) da verificacao,
    no mesmo formato de extrator_core.iter_installation_rows()."""
    ffmpeg_path = verificar_ffmpeg()
    yield ("ffmpeg (transcricao)", ffmpeg_path is not None, ffmpeg_path or "nao encontrado")
    try:
        import faster_whisper  # noqa: F401
        yield ("faster-whisper", True, "instalado")
    except ImportError as exc:
        yield ("faster-whisper", False, str(exc))
    try:
        import soundfile  # noqa: F401
        yield ("soundfile", True, "instalado")
    except ImportError as exc:
        yield ("soundfile", False, str(exc))
    instalados = modelos_instalados()
    yield (
        "Modelo de transcricao",
        bool(instalados),
        ", ".join(instalados) if instalados else "nenhum (será baixado no 1º uso)",
    )


def installation_rows() -> list[tuple[str, bool, str]]:
    """Lista completa (componente, ok, detalhes); a GUI concatena com a do
    extrator_core para dar uma visao unica do programa integrado."""
    return list(iter_installation_rows())


def get_model_info(nome_modelo: str) -> dict:
    if nome_modelo in MODELOS_DISPONIVEIS:
        info = dict(MODELOS_DISPONIVEIS[nome_modelo])
        info["nome"] = nome_modelo
        return info
    return {"erro": f"Modelo '{nome_modelo}' nao encontrado"}


# ============================================================
# Audio
# ============================================================

def extrair_infos_audio(arquivo: Path) -> dict:
    import soundfile as sf

    infos = {
        "formato": "desconhecido", "canais": 0, "duracao": 0.0,
        "taxa_amostragem": 0, "tamanho_mb": arquivo.stat().st_size / (1024 * 1024),
        "erro": None,
    }
    try:
        info_audio = sf.info(str(arquivo))
        infos["formato"] = str(info_audio.format)
        infos["canais"] = info_audio.channels
        infos["duracao"] = info_audio.duration
        infos["taxa_amostragem"] = info_audio.samplerate
    except Exception as exc:
        infos["erro"] = str(exc)
    return infos


def obter_duracao_ffprobe(arquivo: Path) -> float:
    ffprobe_path = verificar_ffprobe()
    if not ffprobe_path:
        return 0.0
    try:
        result = subprocess.run(
            [
                ffprobe_path, "-v", "error", "-show_entries", "format=duration",
                "-of", "default=noprint_wrappers=1:nokey=1", str(arquivo),
            ],
            capture_output=True, text=True, timeout=30,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        return float(result.stdout.strip())
    except Exception:
        return 0.0


def extrair_audio_de_video(video: Path) -> Path:
    ffmpeg_path = verificar_ffmpeg()
    if not ffmpeg_path:
        raise RuntimeError("ffmpeg nao encontrado.")
    TEMP_DIR.mkdir(parents=True, exist_ok=True)
    saida = TEMP_DIR / f"{video.stem}_audio.wav"
    cmd = [
        ffmpeg_path, "-y", "-i", str(video),
        "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(saida),
    ]
    result = subprocess.run(
        cmd, capture_output=True, text=True,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Falha ao extrair audio do video:\n{result.stderr[-800:]}")
    return saida


# ============================================================
# Modelo
# ============================================================

def carregar_modelo(nome_modelo: str = MODELO_PADRAO, device: str = "cpu") -> Any:
    from faster_whisper import WhisperModel

    MODELOS_DIR.mkdir(parents=True, exist_ok=True)
    compute_type = "float32" if device == "cpu" else "int8"
    try:
        return WhisperModel(
            nome_modelo, device=device, compute_type=compute_type, download_root=str(MODELOS_DIR)
        )
    except Exception as exc:
        raise RuntimeError(f"Erro ao carregar modelo '{nome_modelo}': {exc}")


def baixar_modelo(nome_modelo: str, log: Callable[[str], None] | None = None) -> None:
    """Forca o download/cache de um modelo ainda nao instalado."""
    if log:
        log(f"Baixando modelo '{nome_modelo}' (isto pode demorar, requer internet)...")
    carregar_modelo(nome_modelo=nome_modelo, device="cpu")
    if log:
        log(f"Modelo '{nome_modelo}' pronto.")


def gerar_prompt_ptbr(idioma: str) -> str:
    if idioma == "pt":
        return (
            "Transcreva em portugues brasileiro. Use palavras e expressoes comuns do "
            "portugues. Nomes proprios devem ser mantidos. Nao traduzir, apenas transcrever "
            "o audio. Respeite a pontuacao e a estrutura natural da fala."
        )
    if idioma == "auto":
        return (
            "Transcreva o audio no idioma falado. Se for portugues brasileiro, use "
            "vocabulario e expressoes comuns do PT-BR."
        )
    return f"Transcreva o audio no idioma {idioma}."


# ============================================================
# Formatacao de saida
# ============================================================

def formatar_txt(segments) -> str:
    return "\n".join(seg.text.strip() for seg in segments if seg.text.strip())


def _srt_time(segundos: float) -> str:
    horas = int(segundos // 3600)
    minutos = int((segundos % 3600) // 60)
    seg = int(segundos % 60)
    ms = int(round((segundos % 1) * 1000))
    return f"{horas:02d}:{minutos:02d}:{seg:02d},{ms:03d}"


def formatar_srt(segments, word_level: bool = False) -> str:
    linhas = []
    for indice, seg in enumerate(segments, 1):
        texto = seg.text.strip()
        if not texto:
            continue
        linhas.append(f"{indice}\n{_srt_time(seg.start)} --> {_srt_time(seg.end)}\n{texto}\n")
    return "\n".join(linhas)


def formatar_json(segments, word_level: bool = False) -> str:
    data = {
        "segments": [],
        "full_text": " ".join(seg.text.strip() for seg in segments if seg.text.strip()),
    }
    for seg in segments:
        entry = {
            "start": round(seg.start, 3),
            "end": round(seg.end, 3),
            "text": seg.text.strip(),
            "confidence": round(seg.avg_logprob, 3) if hasattr(seg, "avg_logprob") else None,
        }
        if word_level and getattr(seg, "words", None):
            entry["words"] = [
                {
                    "word": w.word.strip() if hasattr(w, "word") else str(w),
                    "start": round(w.start, 3),
                    "end": round(w.end, 3),
                    "probability": round(w.probability, 3),
                }
                for w in seg.words
            ]
        data["segments"].append(entry)
    return json.dumps(data, ensure_ascii=False, indent=2)


def formatar_markdown(segments, meta: dict) -> str:
    """Formato Markdown, no mesmo estilo de cabecalho usado pelo motor de OCR
    (extrator_core.write_joined_output): metadados em lista, depois o corpo."""
    linhas = [f"# {meta.get('titulo', 'Transcricao')}", ""]
    linhas.append(f"- Arquivo de origem: `{meta.get('nome_arquivo', '')}`")
    if meta.get("idioma_detectado"):
        linhas.append(
            f"- Idioma detectado: {meta['idioma_detectado']} "
            f"({meta.get('confianca_idioma', 0):.1%})"
        )
    if meta.get("duracao_s") is not None:
        linhas.append(f"- Duracao: {format_hhmmss(meta['duracao_s'])}")
    if meta.get("modelo"):
        linhas.append(f"- Modelo: {meta['modelo']}")
    linhas.append(f"- Gerado em: {meta.get('gerado_em', datetime.now().strftime('%Y-%m-%d %H:%M:%S'))}")
    linhas.append("")
    linhas.append("## Transcricao")
    linhas.append("")
    for seg in segments:
        texto = seg.text.strip()
        if not texto:
            continue
        linhas.append(f"**[{format_hhmmss(seg.start)} - {format_hhmmss(seg.end)}]** {texto}")
        linhas.append("")
    return "\n".join(linhas).rstrip() + "\n"


def salvar_saida(texto: str, arquivo_original: Path, format_saida: str) -> Path:
    output_path = arquivo_original.with_suffix(f".{format_saida}")
    output_path.write_text(texto, encoding="utf-8")
    return output_path


# ============================================================
# Transcricao com progresso, pausa e cancelamento
# ============================================================

class _Heartbeat:
    """Emite uma linha de log periodica para provar que a thread em segundo
    plano ainda esta viva mesmo antes do 1o segmento (modelos grandes/CPU
    fraca podem levar minutos)."""

    def __init__(self, log: Callable[[str], None], intervalo_s: float = 5.0):
        self._log = log
        self._intervalo = intervalo_s
        self._fase = "iniciando"
        self._stop = threading.Event()
        self._inicio = time.monotonic()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def fase(self, nome: str) -> None:
        self._fase = nome
        execucoes.etapa(nome[:1].upper() + nome[1:])  # etapa na Central de execucoes (thread do trabalho)

    def stop(self) -> None:
        self._stop.set()

    def _run(self) -> None:
        while not self._stop.wait(self._intervalo):
            decorrido = time.monotonic() - self._inicio
            self._log(f"  ... ainda em execucao ({self._fase}) - {format_hhmmss(decorrido)} decorridos")


def transcrever_com_progresso(
    arquivo: Path,
    modelo: str,
    idioma: str,
    format_saida: str,
    word_level: bool,
    device: str,
    beam_size: int,
    log: Callable[[str], None],
    progresso_cb: Callable[[dict], None],
    cancel_event: threading.Event,
    pause_event: threading.Event,
) -> dict:
    """Executa uma transcricao emitindo progresso por segmento.

    progresso_cb recebe um dict com pct, processado_s, total_s, restante_s,
    elapsed_s e eta_s (estimativa de tempo restante, None se ainda nao da
    para estimar). Cooperativamente verifica cancel_event (levanta
    core.OperationCancelled) e pause_event (bloqueia entre segmentos,
    nao interrompe um segmento ja em processamento).
    """
    log(
        f"--- Nova transcricao: {arquivo.name} (modelo={modelo}, idioma={idioma}, "
        f"formato={format_saida}, beams={beam_size})"
    )

    heartbeat = _Heartbeat(log)
    heartbeat.start()
    try:
        heartbeat.fase("verificando ffmpeg")
        ffmpeg_path = verificar_ffmpeg()
        if ffmpeg_path is None:
            raise RuntimeError("ffmpeg nao encontrado. Use o atalho 'Reparar instalacao' do Menu Iniciar.")

        arquivo_original = arquivo
        arquivo_para_transcrever = arquivo
        if arquivo.suffix.lower() in VIDEO_EXTS:
            heartbeat.fase("extraindo audio do video")
            log(f"Extraindo audio do video com ffmpeg...")
            arquivo_para_transcrever = extrair_audio_de_video(arquivo)

        heartbeat.fase("carregando modelo")
        log(f"Carregando modelo '{modelo}' (pode demorar na primeira vez)...")
        t0 = time.monotonic()
        model = carregar_modelo(nome_modelo=modelo, device=device)
        log(f"Modelo carregado em {format_hhmmss(time.monotonic() - t0)}")

        heartbeat.fase("analisando audio")
        audio_infos = extrair_infos_audio(arquivo_para_transcrever)
        duracao_total = audio_infos.get("duracao") or 0.0
        if duracao_total <= 0:
            log("  duracao nao disponivel via soundfile; tentando via ffprobe...")
            duracao_total = obter_duracao_ffprobe(arquivo_para_transcrever)
        log(f"Duracao total do audio: {format_hhmmss(duracao_total)}")
        if duracao_total > 0:  # total conhecido: a barra deixa de ser indeterminada
            execucoes.total(duracao_total, "segundo", "duracao")

        initial_prompt = gerar_prompt_ptbr(idioma)
        vad_parameters = dict(
            min_silence_duration_ms=500, max_speech_duration_s=10.0,
            threshold=0.5, speech_pad_ms=200,
        )

        heartbeat.fase("transcrevendo (aguardando 1o segmento)")
        log(f"Transcrevendo... (idioma: {idioma}, beams: {beam_size})")
        inicio_processamento = time.monotonic()
        segments_iter, lang_info = model.transcribe(
            str(arquivo_para_transcrever),
            language=idioma if idioma != "auto" else None,
            beam_size=beam_size,
            vad_filter=True,
            vad_parameters=vad_parameters,
            initial_prompt=initial_prompt,
            word_timestamps=word_level,
        )
        log(f"Idioma detectado: {lang_info.language} ({lang_info.language_probability:.1%})")

        segments_list = []
        for seg in segments_iter:
            if cancel_event.is_set():
                log("Cancelado pelo usuario.")
                raise core.OperationCancelled("Transcricao cancelada pelo usuario.")
            if pause_event.is_set():
                heartbeat.fase("pausado")
                log("Pausado pelo usuario.")
                while pause_event.is_set():
                    if cancel_event.is_set():
                        log("Cancelado pelo usuario (durante pausa).")
                        raise core.OperationCancelled("Transcricao cancelada pelo usuario.")
                    time.sleep(0.2)
                log("Retomado.")

            segments_list.append(seg)
            processado = min(seg.end, duracao_total) if duracao_total > 0 else seg.end
            elapsed = time.monotonic() - inicio_processamento
            pct = min(processado / duracao_total, 1.0) * 100 if duracao_total > 0 else 0.0
            restante_audio = max(0.0, duracao_total - processado)
            taxa = processado / elapsed if elapsed > 0 and processado > 0 else 0.0
            eta = (restante_audio / taxa) if taxa > 0 else None

            heartbeat.fase(f"transcrevendo (segmento {len(segments_list)}, {format_hhmmss(processado)} processados)")
            progresso_cb({
                "pct": pct,
                "processado_s": processado,
                "total_s": duracao_total,
                "restante_s": restante_audio,
                "elapsed_s": elapsed,
                "eta_s": eta,
            })
            log(f"  [{format_hhmmss(seg.start)} -> {format_hhmmss(seg.end)}] {seg.text.strip()}")

        log(f"Transcricao bruta concluida em {format_hhmmss(time.monotonic() - inicio_processamento)} "
            f"({len(segments_list)} segmentos)")

        heartbeat.fase("formatando e salvando")
        meta = {
            "titulo": arquivo_original.stem,
            "nome_arquivo": arquivo_original.name,
            "idioma_detectado": lang_info.language,
            "confianca_idioma": lang_info.language_probability,
            "duracao_s": duracao_total,
            "modelo": modelo,
        }
        if format_saida == "txt":
            texto = formatar_txt(segments_list)
        elif format_saida == "md":
            texto = formatar_markdown(segments_list, meta)
        elif format_saida == "srt":
            texto = formatar_srt(segments_list, word_level=word_level)
        else:
            texto = formatar_json(segments_list, word_level=word_level)

        output_path = salvar_saida(texto, arquivo_original, format_saida)
        log(f"Salvo em: {output_path}")

        return {
            "texto": texto,
            "output_path": str(output_path),
            "segments": segments_list,
            "audio_infos": audio_infos,
            "lang_info": lang_info,
        }
    finally:
        heartbeat.stop()
