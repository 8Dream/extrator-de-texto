#!/usr/bin/env python3
"""
Extrator de Texto - ponto de entrada da interface (HTML dentro de uma janela nativa).

A interface fica em web/ (HTML, CSS e JavaScript) e roda no WebView2 do
Windows por meio do pywebview. O motor continua sendo o mesmo Python
(extrator_core.py e transcricao_core.py), ligado por extrator_api.Api.

Tudo que cria janela fica dentro de main(), chamado so sob
`if __name__ == "__main__":`. Isso e essencial: o motor de OCR usa
multiprocessing e, no Windows, cada processo-filho reimporta este script;
sem o guard, cada filho abriria sua propria janela.
"""
from __future__ import annotations

import semjanela

# Antes de qualquer outro import: nenhum subprocesso (tesseract, gs, ffmpeg...)
# pode abrir janela de console, nem os que bibliotecas de terceiros iniciam.
# Fica fora do main() de proposito: os processos-filho do OCR (multiprocessing,
# spawn) reimportam este arquivo e tambem precisam do patch.
semjanela.instalar()

import ctypes  # noqa: E402
import json
import logging  # noqa: E402
import sys  # noqa: E402
import threading  # noqa: E402
import time  # noqa: E402
from pathlib import Path  # noqa: E402

WEB_DIR = Path(__file__).resolve().parent / "web"
ICON_PATH = Path(__file__).resolve().parent / "assets" / "icone.ico"
APP_USER_MODEL_ID = "AmancioFavarato.ExtratorDeTexto"
PUMP_INTERVAL_S = 0.1


def _fatal(message: str) -> None:
    """Mostra o erro numa caixa nativa: sem console (pythonw) uma falha ao
    iniciar encerraria o programa sem nenhuma pista para o usuario."""
    logging.error(message)
    try:
        ctypes.windll.user32.MessageBoxW(0, message, "Extrator de Texto - falha ao iniciar", 0x10)
    except Exception:
        print(message, file=sys.stderr)
    raise SystemExit(1)


def _pump_events(window, api, stop: threading.Event) -> None:
    """Empurra os eventos do motor para o JavaScript em lotes."""
    while not stop.is_set():
        events = api.drain_events()
        if events:
            try:
                window.evaluate_js(f"window.__onEvents({json.dumps(events, default=str)})")
            except Exception:
                logging.exception("Falha ao enviar eventos para a interface")
        time.sleep(PUMP_INTERVAL_S)


def _register_drop(window, api) -> None:
    """Arrastar e soltar: o JS nao enxerga o caminho real do arquivo, mas o
    pywebview o entrega no evento DOM de drop (pywebviewFullPath)."""
    try:
        from webview.dom import DOMEventHandler

        def on_drop(event):
            files = (event.get("dataTransfer") or {}).get("files") or []
            paths = [f.get("pywebviewFullPath") for f in files if f.get("pywebviewFullPath")]
            if paths:
                api._events.put({"type": "files_dropped", "paths": paths})

        window.dom.document.events.drop += DOMEventHandler(on_drop, True, True)
    except Exception:
        logging.exception("Arrastar e soltar indisponivel")


def main() -> None:
    try:
        import webview

        import extrator_core as core
        from extrator_api import Api
    except Exception as exc:  # dependencia ausente ou instalacao corrompida
        _fatal(
            f"Não foi possível iniciar o programa.\n\n{exc}\n\n"
            "Execute scripts\\Instalar.bat (ou o atalho 'Reparar instalação') e tente novamente."
        )
        return

    try:
        # Faz a barra de tarefas agrupar a janela como este aplicativo (com o
        # icone proprio) em vez de como "pythonw.exe".
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(APP_USER_MODEL_ID)
    except Exception:
        logging.debug("AppUserModelID indisponivel", exc_info=True)

    api = Api()
    window = webview.create_window(
        f"Extrator de Texto v{core.APP_VERSION}",
        url=str(WEB_DIR / "index.html"),
        js_api=api,
        width=1220,
        height=820,
        min_size=(1000, 660),
        background_color="#f6f7f9",
    )
    api.attach_window(window)
    stop = threading.Event()

    close_warned = {"done": False}

    def on_closing():
        """Sem caixa de dialogo: com trabalho em andamento, o primeiro pedido
        de fechar so avisa DENTRO da janela; fechar de novo encerra."""
        busy = api.is_busy()
        if not (busy["ocr"] or busy["transcricao"]) or close_warned["done"]:
            api.request_stop_all()
            return True
        close_warned["done"] = True
        api.notify_close_blocked()
        return False

    def on_loaded():
        _register_drop(window, api)

    window.events.closing += on_closing
    window.events.loaded += on_loaded
    threading.Thread(target=_pump_events, args=(window, api, stop), daemon=True).start()

    try:
        webview.start(
            debug="--debug" in sys.argv,
            private_mode=False,
            storage_path=str(core.APP_DATA_DIR / "interface"),
            icon=str(ICON_PATH) if ICON_PATH.exists() else None,
        )
    except Exception as exc:
        logging.exception("Falha ao abrir a janela")
        _fatal(
            f"Não foi possível abrir a janela.\n\n{exc}\n\n"
            "Verifique se o Microsoft Edge WebView2 Runtime está instalado "
            "(https://developer.microsoft.com/microsoft-edge/webview2/)."
        )
    finally:
        stop.set()


if __name__ == "__main__":
    main()
