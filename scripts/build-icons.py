#!/usr/bin/env python3
"""Génère les images PNG du projet à partir de la géométrie du logo.

Le logo est décrit une seule fois, dans web/static/icon.svg ; ce script en
reprend les coordonnées pour les formats qui exigent du PNG : icônes d'écran
d'accueil (iOS, Android) et image d'aperçu du dépôt.

Usage : scripts/build-icons.py, depuis la racine du dépôt.
Dépendances : Pillow, et les polices installées par `pnpm install` dans web/.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

NIGHT, WHITE, SUN = (22, 32, 58), (255, 255, 255), (255, 197, 61)
ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "web" / "static"
MIST = (158, 171, 198)
FONTS = ROOT / "web" / "node_modules" / "@fontsource-variable"
DISPLAY = FONTS / "bricolage-grotesque" / "files" / "bricolage-grotesque-latin-wdth-normal.woff2"
TEXT = FONTS / "atkinson-hyperlegible-next" / "files" / "atkinson-hyperlegible-next-latin-wght-normal.woff2"
SS = 4  # suréchantillonnage, pour des bords lisses


def lerp(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)


def line(a, b, n=40):
    return [lerp(a, b, i / n) for i in range(n + 1)]


def quad(a, c, b, n=40):
    return [lerp(lerp(a, c, i / n), lerp(c, b, i / n), i / n) for i in range(n + 1)]


def cubic(a, c1, c2, b, n=120):
    out = []
    for i in range(n + 1):
        t = i / n
        p, q, r = lerp(a, c1, t), lerp(c1, c2, t), lerp(c2, b, t)
        out.append(lerp(lerp(p, q, t), lerp(q, r, t), t))
    return out


# Moitié gauche du bouclier, dans le repère 64 × 64 du logo.
HALF = (
    line((26.2, 7.2), (15.5, 10.4))
    + quad((15.5, 10.4), (11, 11.2), (11, 15.8))
    + line((11, 15.8), (11, 30.5))
    + cubic((11, 30.5), (11, 44.2), (19.6, 52.6), (27.8, 57.2))
)
HANDS = line((32, 24.6), (32, 30.8)) + line((32, 30.8), (36.4, 33.4))


def render(size, scale):
    px = size * SS
    image = Image.new("RGB", (px, px), NIGHT)
    draw = ImageDraw.Draw(image)
    unit = px / 64

    def place(p):
        # Même transformation que dans icon.svg : mise à l'échelle autour du centre.
        return ((32 + (p[0] - 32) * scale) * unit, (32.5 + (p[1] - 32) * scale) * unit)

    def stroke(points, width, color):
        r = width * scale * unit / 2
        for p in points:
            x, y = place(p)
            draw.ellipse((x - r, y - r, x + r, y + r), fill=color)

    stroke(HALF, 6.6, WHITE)
    stroke([(64 - x, y) for x, y in HALF], 6.6, WHITE)
    stroke([(32, 30.8)], 20.8, SUN)
    stroke(HANDS, 3.3, NIGHT)
    return image.resize((size, size), Image.LANCZOS)


def social_preview():
    """Image d'aperçu du dépôt, au format attendu par GitHub : 1280 × 640.

    À déposer à la main dans Settings › General › Social preview : GitHub ne
    propose pas d'API pour cette image.
    """
    w, h, ss = 1280, 640, 2
    image = Image.new("RGB", (w * ss, h * ss), NIGHT)
    draw = ImageDraw.Draw(image)

    mark = 300 * ss
    left = 150 * ss
    image.paste(render(mark, 0.92), (left, (h * ss - mark) // 2))

    title = ImageFont.truetype(str(DISPLAY), 148 * ss)
    title.set_variation_by_axes([700, 84])  # graisse, largeur
    body = ImageFont.truetype(str(TEXT), 40 * ss)
    body.set_variation_by_axes([450])

    x = left + mark + 44 * ss
    draw.text((x, 196 * ss), "Cotutelle", font=title, fill=WHITE)
    draw.text((x + 6 * ss, 372 * ss), "Le temps d’écran et le filtrage,", font=body, fill=MIST)
    draw.text((x + 6 * ss, 424 * ss), "pour toute la famille.", font=body, fill=MIST)

    target = ROOT / "docs" / "assets" / "social-preview.png"
    target.parent.mkdir(parents=True, exist_ok=True)
    image.resize((w, h), Image.LANCZOS).save(target, optimize=True)
    return target


if __name__ == "__main__":
    # Icônes plein cadre : le système arrondit lui-même les coins.
    for size in (180, 192, 512):
        render(size, 0.72).save(OUT / f"icon-{size}.png", optimize=True)
    # Icône masquable : le motif tient dans la zone sûre centrale.
    render(512, 0.56).save(OUT / "icon-maskable-512.png", optimize=True)
    print("icônes écrites dans", OUT)
    print("aperçu du dépôt écrit dans", social_preview())
