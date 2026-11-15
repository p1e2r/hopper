#!/usr/bin/env python3
"""Generates Hopper's app icon (original pixel art) in every format we ship.

Run from the repo root:  python3 tools/gen_icon.py
Writes assets/icon/icon.png (1024), icon.ico (Windows), icon.icns (macOS)
and favicon.png (site).
"""
from PIL import Image, ImageDraw

# 16x16 art. '.' = transparent (shows the tile behind it).
ART = [
    "......gGGg......",
    "......gGGg......",
    "...ggggGGgggg...",
    "....gGGGGGGg....",
    ".....gGGGGg.....",
    "......gGGg......",
    "..kkkkkkkkkkkk..",
    "..kLLLLLLLLLLk..",
    "..kLddddddddLk..",
    "..kMMMMMMMMMMk..",
    "..kMmmmmmmmmMk..",
    "...kMMMMMMMMk...",
    "....kMmmmmMk....",
    ".....kMMMMk.....",
    "......kMMk......",
    ".......kk.......",
]
# '.' is transparent; the green arrow drops into the hopper's mouth.

COLORS = {
    "k": (26, 26, 28),
    "L": (168, 168, 176),
    "M": (120, 120, 128),
    "m": (92, 92, 100),
    "d": (40, 40, 44),
    "g": (63, 138, 42),
    "G": (127, 207, 91),
}

TILE = (44, 44, 49)
TILE_DARK = (30, 30, 34)
TILE_EDGE = (108, 195, 73)


def render(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    # Rounded tile, inset like macOS icons (~10% margin).
    margin = round(size * 0.09)
    radius = round(size * 0.18)
    box = (margin, margin, size - margin - 1, size - margin - 1)
    draw.rounded_rectangle(box, radius, fill=TILE_EDGE)
    edge = max(1, round(size * 0.025))
    inner = (box[0] + edge, box[1] + edge, box[2] - edge, box[3] - edge)
    draw.rounded_rectangle(inner, max(1, radius - edge), fill=TILE)
    # Darker lower half for depth.
    half = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    hd = ImageDraw.Draw(half)
    hd.rounded_rectangle(inner, max(1, radius - edge), fill=TILE_DARK)
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rectangle((0, size // 2, size, size), fill=255)
    img.paste(half, (0, 0), Image.composite(half, Image.new("RGBA", (size, size)), mask).split()[3])
    # Pixel art, nearest-neighbour, centred in the tile.
    art = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    for y, row in enumerate(ART):
        for x, ch in enumerate(row):
            if ch in COLORS:
                art.putpixel((x, y), COLORS[ch] + (255,))
    art_size = (inner[2] - inner[0]) * 15 // 16 // 16 * 16 or 16
    art = art.resize((art_size, art_size), Image.NEAREST)
    off = (size - art_size) // 2
    img.alpha_composite(art, (off, off))
    return img


if __name__ == "__main__":
    big = render(1024)
    big.save("assets/icon/icon.png")
    render(64).save("assets/icon/favicon.png")
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    render(256).save("assets/icon/icon.ico", sizes=[(s, s) for s in ico_sizes],
                     append_images=[render(s) for s in ico_sizes])
    big.save("assets/icon/icon.icns")
    print("icons written")
