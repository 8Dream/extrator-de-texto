"""Gera src/assets/icone.ico (um "F" branco sobre quadrado azul arredondado).

Uso (precisa do Pillow, ja instalado no ambiente do programa):
    venv\\Scripts\\python.exe scripts\\dev\\gerar_icone.py
"""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src" / "assets" / "icone.ico"
ACCENT = (31, 79, 209, 255)
SIZES = [16, 24, 32, 48, 64, 128, 256]


def _font(size: int) -> ImageFont.FreeTypeFont:
    for name in ("segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def render(px: int) -> Image.Image:
    scale = 4  # desenha grande e reduz, para bordas suaves
    big = px * scale
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * 0.22), fill=ACCENT)
    font = _font(int(big * 0.62))
    box = draw.textbbox((0, 0), "F", font=font)
    x = (big - (box[2] - box[0])) / 2 - box[0]
    y = (big - (box[3] - box[1])) / 2 - box[1]
    draw.text((x, y), "F", font=font, fill=(255, 255, 255, 255))
    return img.resize((px, px), Image.LANCZOS)


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    base = render(256)
    base.save(OUT, format="ICO", sizes=[(s, s) for s in SIZES])
    print(f"Gerado: {OUT}")


if __name__ == "__main__":
    main()
