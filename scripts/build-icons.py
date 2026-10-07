#!/usr/bin/env python3
"""Génère les icônes PNG de l'application à partir de la géométrie du logo.

Le logo est décrit une seule fois, dans web/static/icon.svg ; ce script en
reprend les coordonnées pour les formats qui exigent du PNG (écran d'accueil
iOS et Android). Usage : scripts/build-icons.py, depuis la racine du dépôt.
Dépendance : Pillow.
"""
from pathlib import Path
from PIL import Image, ImageDraw

NIGHT, WHITE, SUN = (22, 32, 58), (255, 255, 255), (255, 197, 61)
OUT = Path(__file__).resolve().parent.parent / "web" / "static"
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


if __name__ == "__main__":
    # Icônes plein cadre : le système arrondit lui-même les coins.
    for size in (180, 192, 512):
        render(size, 0.72).save(OUT / f"icon-{size}.png", optimize=True)
    # Icône masquable : le motif tient dans la zone sûre centrale.
    render(512, 0.56).save(OUT / "icon-maskable-512.png", optimize=True)
    print("icônes écrites dans", OUT)
