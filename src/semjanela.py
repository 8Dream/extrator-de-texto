"""
Extrator de Texto - impede que subprocessos abram janelas de console.

O programa roda com pythonw.exe (sem console). No Windows, todo processo
filho de console iniciado a partir dele (tesseract, gswin64c, ffmpeg...)
ganha uma janela preta propria, que pisca na tela. Isso vale ate para
bibliotecas que chamam subprocess por conta propria (pytesseract, ocrmypdf,
static_ffmpeg, av). A correcao e global: `instalar()` envolve
subprocess.Popen.__init__ e acrescenta CREATE_NO_WINDOW (e SW_HIDE) a toda
chamada que nao pediu explicitamente outro comportamento de console.

Chamar `instalar()` no inicio do processo, antes de importar o resto.
"""
from __future__ import annotations

import os
import subprocess

_CREATE_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000)
_CREATE_NEW_CONSOLE = getattr(subprocess, "CREATE_NEW_CONSOLE", 0x00000010)
_DETACHED_PROCESS = getattr(subprocess, "DETACHED_PROCESS", 0x00000008)
_CONSOLE_FLAGS = _CREATE_NO_WINDOW | _CREATE_NEW_CONSOLE | _DETACHED_PROCESS


def aplicar_flags(creationflags: int, startupinfo):
    """Devolve (creationflags, startupinfo) ja sem janela de console."""
    if not creationflags & _CONSOLE_FLAGS:
        creationflags |= _CREATE_NO_WINDOW
    if startupinfo is None and hasattr(subprocess, "STARTUPINFO"):
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startupinfo.wShowWindow = subprocess.SW_HIDE
    return creationflags, startupinfo


def instalar(force: bool = False) -> bool:
    """Aplica o patch (idempotente). So age no Windows, salvo force=True (testes)."""
    if os.name != "nt" and not force:
        return False
    original = subprocess.Popen.__init__
    if getattr(original, "_semjanela", False):
        return True

    def __init__(self, *args, **kwargs):
        flags, startup = aplicar_flags(int(kwargs.get("creationflags", 0) or 0), kwargs.get("startupinfo"))
        kwargs["creationflags"] = flags
        if startup is not None:
            kwargs["startupinfo"] = startup
        original(self, *args, **kwargs)

    __init__._semjanela = True  # type: ignore[attr-defined]
    __init__._original = original  # type: ignore[attr-defined]
    subprocess.Popen.__init__ = __init__  # type: ignore[method-assign]
    return True


def desinstalar() -> None:
    """Remove o patch (usado pelos testes)."""
    atual = subprocess.Popen.__init__
    original = getattr(atual, "_original", None)
    if original is not None:
        subprocess.Popen.__init__ = original  # type: ignore[method-assign]
