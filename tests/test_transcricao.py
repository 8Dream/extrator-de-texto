"""Testes automatizados do nucleo de transcricao (transcricao_core.py).

As funcoes puras (formatacao, prompt, deteccao de modelos instalados) sao
testadas diretamente. A transcricao de verdade (carregar o modelo Whisper e
processar audio) foi validada manualmente com fala real gerada via TTS do
Windows durante o desenvolvimento; os testes automatizados aqui nao
dependem disso para rodarem rapido em qualquer maquina.

Executar com: venv\\Scripts\\python.exe -m pytest tests/test_transcricao.py
"""
from __future__ import annotations

import sys
import threading
from pathlib import Path
from types import SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

import transcricao_core as tc


# ---------------------------------------------------------------------------
# format_hhmmss (pedido explicito do usuario: sempre HH:MM:SS)
# ---------------------------------------------------------------------------


def test_format_hhmmss_zero():
    assert tc.format_hhmmss(0) == "00:00:00"


def test_format_hhmmss_seconds_and_minutes():
    assert tc.format_hhmmss(65) == "00:01:05"


def test_format_hhmmss_hours():
    assert tc.format_hhmmss(3725) == "01:02:05"


def test_format_hhmmss_rounds_to_nearest_second():
    assert tc.format_hhmmss(59.6) == "00:01:00"


def test_format_hhmmss_negative_clamped_to_zero():
    assert tc.format_hhmmss(-5) == "00:00:00"


# ---------------------------------------------------------------------------
# Deteccao de modelos instalados (pedido: nao listar modelos indisponiveis)
# ---------------------------------------------------------------------------


def _fake_segment(start, end, text, avg_logprob=-0.2, words=None):
    return SimpleNamespace(start=start, end=end, text=text, avg_logprob=avg_logprob, words=words)


def test_modelos_instalados_empty_when_no_cache_dir(tmp_path, monkeypatch):
    monkeypatch.setattr(tc, "MODELOS_DIR", tmp_path / "nao_existe")
    assert tc.modelos_instalados() == []


def test_modelos_instalados_detects_only_complete_models(tmp_path, monkeypatch):
    monkeypatch.setattr(tc, "MODELOS_DIR", tmp_path)

    completo = tmp_path / "models--Systran--faster-whisper-small" / "snapshots" / "abc123"
    completo.mkdir(parents=True)
    (completo / "model.bin").write_bytes(b"conteudo-fake-do-modelo")

    incompleto = tmp_path / "models--Systran--faster-whisper-medium" / "snapshots" / "def456"
    incompleto.mkdir(parents=True)
    (incompleto / "model.bin.incomplete").write_bytes(b"parcial")

    instalados = tc.modelos_instalados()
    assert instalados == ["small"]
    assert "medium" not in instalados
    assert "tiny" not in instalados


def test_get_model_info_unknown_model_returns_error():
    info = tc.get_model_info("modelo-que-nao-existe")
    assert "erro" in info


# ---------------------------------------------------------------------------
# Prompt PT-BR
# ---------------------------------------------------------------------------


def test_gerar_prompt_ptbr_specific_language():
    prompt = tc.gerar_prompt_ptbr("pt")
    assert "português" in prompt.lower()


def test_gerar_prompt_ptbr_auto():
    prompt = tc.gerar_prompt_ptbr("auto")
    assert "idioma falado" in prompt.lower()


def test_gerar_prompt_ptbr_other_language():
    prompt = tc.gerar_prompt_ptbr("en")
    assert "en" in prompt


# ---------------------------------------------------------------------------
# Formatacao de saida (txt, srt, json, markdown)
# ---------------------------------------------------------------------------


def test_formatar_txt_joins_non_empty_segments():
    segments = [_fake_segment(0, 1, "Ola"), _fake_segment(1, 2, "  "), _fake_segment(2, 3, "mundo")]
    assert tc.formatar_txt(segments) == "Ola\nmundo"


def test_formatar_srt_uses_comma_milliseconds_not_hhmmss_only():
    segments = [_fake_segment(0, 65.5, "Teste")]
    srt = tc.formatar_srt(segments)
    assert "00:00:00,000 --> 00:01:05,500" in srt
    assert "1\n" in srt
    assert "Teste" in srt


def test_formatar_json_includes_confidence_and_words(monkeypatch):
    palavra = SimpleNamespace(word="oi", start=0.0, end=0.5, probability=0.9)
    segments = [_fake_segment(0, 1, "oi", words=[palavra])]
    texto = tc.formatar_json(segments, word_level=True)
    import json
    data = json.loads(texto)
    assert data["full_text"] == "oi"
    assert data["segments"][0]["confidence"] == -0.2
    assert data["segments"][0]["words"][0]["word"] == "oi"


def test_formatar_json_without_word_level_omits_words():
    palavra = SimpleNamespace(word="oi", start=0.0, end=0.5, probability=0.9)
    segments = [_fake_segment(0, 1, "oi", words=[palavra])]
    texto = tc.formatar_json(segments, word_level=False)
    import json
    data = json.loads(texto)
    assert "words" not in data["segments"][0]


def test_formatar_markdown_uses_hhmmss_timestamps_and_header():
    segments = [_fake_segment(0, 65, "Primeiro segmento"), _fake_segment(65, 130, "Segundo segmento")]
    meta = {
        "titulo": "reuniao", "nome_arquivo": "reuniao.mp3", "idioma_detectado": "pt",
        "confianca_idioma": 0.987, "duracao_s": 130, "modelo": "small",
        "gerado_em": "2026-01-01 10:00:00",
    }
    md = tc.formatar_markdown(segments, meta)
    assert "# reuniao" in md
    assert "Arquivo de origem: `reuniao.mp3`" in md
    assert "Idioma detectado: pt (98.7%)" in md
    assert "Duração: 00:02:10" in md
    assert "**[00:00:00 - 00:01:05]** Primeiro segmento" in md
    assert "**[00:01:05 - 00:02:10]** Segundo segmento" in md


def test_md_is_offered_as_output_format():
    assert "md" in tc.FORMATOS_SAIDA


# ---------------------------------------------------------------------------
# Cancelamento e pausa cooperativos (sem carregar o modelo real)
# ---------------------------------------------------------------------------


def test_transcrever_com_progresso_respects_pre_cancelled_event(monkeypatch, tmp_path):
    """Cancela antes mesmo do primeiro segmento: nao deve tentar carregar o
    modelo Whisper de verdade (o que tornaria o teste lento e dependente de
    internet/CPU)."""
    audio = tmp_path / "audio.wav"
    audio.write_bytes(b"fake")

    monkeypatch.setattr(tc, "verificar_ffmpeg", lambda: "ffmpeg.exe")

    class FakeModel:
        def transcribe(self, *_a, **_k):
            segments = [_fake_segment(0, 1, "nao deveria processar isto")]
            lang_info = SimpleNamespace(language="pt", language_probability=1.0)
            return iter(segments), lang_info

    monkeypatch.setattr(tc, "carregar_modelo", lambda **_k: FakeModel())
    monkeypatch.setattr(tc, "extrair_infos_audio", lambda _a: {"duracao": 10.0})

    cancel_event = threading.Event()
    cancel_event.set()
    pause_event = threading.Event()

    with pytest.raises(tc.core.OperationCancelled):
        tc.transcrever_com_progresso(
            arquivo=audio, modelo="small", idioma="pt", format_saida="txt",
            word_level=False, device="cpu", beam_size=1,
            log=lambda m: None, progresso_cb=lambda i: None,
            cancel_event=cancel_event, pause_event=pause_event,
        )
