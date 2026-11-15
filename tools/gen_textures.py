#!/usr/bin/env python3
"""Generates Hopper's pixel-art textures (original, procedural; no game assets).

Run from the repo root:  python3 tools/gen_textures.py
Each texel is drawn as a SCALE x SCALE block so the result stays crisp.
"""
import random
from PIL import Image

SCALE = 3


def save(texels, w, h, path):
    img = Image.new("RGB", (w * SCALE, h * SCALE))
    px = img.load()
    for y in range(h):
        for x in range(w):
            c = texels[y][x]
            for dy in range(SCALE):
                for dx in range(SCALE):
                    px[x * SCALE + dx, y * SCALE + dy] = c
    img.save(path, optimize=True)


def jitter(rng, base, amount):
    d = rng.randint(-amount, amount)
    return tuple(max(0, min(255, v + d)) for v in base)


def grass_strip(path, w=640, h=9):
    rng = random.Random(7)
    greens = [(79, 145, 52), (93, 163, 60), (68, 128, 45), (104, 178, 66)]
    dirts = [(121, 85, 58), (104, 72, 49), (134, 96, 66), (89, 61, 42)]
    rows = [[None] * w for _ in range(h)]
    for x in range(w):
        # grass depth wobbles between 2 and 4 texels, with occasional drips
        depth = 2 + rng.choice([0, 0, 1, 1, 2])
        if rng.random() < 0.08:
            depth += 1
        for y in range(h):
            if y < depth:
                rows[y][x] = rng.choice(greens)
            else:
                c = rng.choice(dirts)
                if rng.random() < 0.06:
                    c = (150, 150, 150)  # pebble
                rows[y][x] = c
    save(rows, w, h, path)


def stone(path, w=96, h=560):
    rng = random.Random(11)
    base = (44, 44, 49)
    rows = [[jitter(rng, base, 5) for _ in range(w)] for _ in range(h)]
    # a few darker cracks and lighter flecks, kept subtle so text stays readable
    for _ in range(w * h // 40):
        x, y = rng.randrange(w), rng.randrange(h)
        rows[y][x] = jitter(rng, (34, 34, 38), 3)
        if x + 1 < w and rng.random() < 0.6:
            rows[y][x + 1] = jitter(rng, (34, 34, 38), 3)
    for _ in range(w * h // 90):
        x, y = rng.randrange(w), rng.randrange(h)
        rows[y][x] = jitter(rng, (58, 58, 64), 4)
    save(rows, w, h, path)


if __name__ == "__main__":
    grass_strip("assets/textures/grass_strip.png")
    stone("assets/textures/stone.png")
