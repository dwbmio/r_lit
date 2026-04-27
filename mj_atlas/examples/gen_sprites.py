#!/usr/bin/env python3
"""Procedurally generate a small set of demo sprites for mj_atlas examples.

These are used in the README/tutorials and `cargo run` walkthroughs. They are
NOT bundled in the repo — re-run this script if you need them on disk:

    python3 examples/gen_sprites.py [output_dir]
"""

import os
import sys
import random
from PIL import Image, ImageDraw

OUT_DIR = sys.argv[1] if len(sys.argv) > 1 else "examples/sprites"

PALETTE = [
    (231, 76, 60),
    (52, 152, 219),
    (46, 204, 113),
    (241, 196, 15),
    (155, 89, 182),
    (26, 188, 156),
    (230, 126, 34),
    (52, 73, 94),
]


def circle(name, size, color):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse((1, 1, size - 2, size - 2), fill=color + (255,))
    return name, img


def square(name, size, color):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, size - 1, size - 1), fill=color + (255,))
    return name, img


def rounded(name, w, h, color):
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, w - 1, h - 1), radius=min(w, h) // 4, fill=color + (255,))
    return name, img


def diamond(name, size, color):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.polygon(
        [(size // 2, 0), (size - 1, size // 2), (size // 2, size - 1), (0, size // 2)],
        fill=color + (255,),
    )
    return name, img


def multi_blob(name):
    """Multi-component sprite — 3 disjoint blobs in one image. Demonstrates
    the connected-component polygon mesh feature."""
    img = Image.new("RGBA", (96, 56), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse((4, 4, 32, 32), fill=PALETTE[0] + (255,))
    d.rectangle((44, 8, 80, 48), fill=PALETTE[2] + (255,))
    d.polygon(
        [(20, 42), (32, 50), (20, 54), (8, 50)],
        fill=PALETTE[3] + (255,),
    )
    return name, img


def walk_anim_frame(name, phase):
    """Small walk-cycle frame for animation auto-detection demo (frame N is
    `walk_NN.png`)."""
    img = Image.new("RGBA", (32, 48), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    body = (40, 120, 200)
    d.ellipse((10, 4, 22, 16), fill=body + (255,))           # head
    d.rectangle((11, 16, 21, 32), fill=body + (255,))        # body
    leg_offset = phase * 3
    d.line((16, 32, 12 - leg_offset, 46), fill=body + (255,), width=3)
    d.line((16, 32, 20 + leg_offset, 46), fill=body + (255,), width=3)
    return name, img


def main():
    random.seed(42)
    os.makedirs(OUT_DIR, exist_ok=True)
    sprites = []

    sprites.append(circle("icon_red.png", 32, PALETTE[0]))
    sprites.append(circle("icon_blue.png", 28, PALETTE[1]))
    sprites.append(square("badge_green.png", 36, PALETTE[2]))
    sprites.append(square("badge_yellow.png", 24, PALETTE[3]))
    sprites.append(rounded("button_purple.png", 64, 32, PALETTE[4]))
    sprites.append(rounded("button_teal.png", 48, 24, PALETTE[5]))
    sprites.append(diamond("gem_orange.png", 40, PALETTE[6]))
    sprites.append(diamond("gem_dark.png", 28, PALETTE[7]))

    sprites.append(multi_blob("multi_blob.png"))

    for i in range(4):
        sprites.append(walk_anim_frame(f"walk_{i + 1:02d}.png", i))

    for name, img in sprites:
        path = os.path.join(OUT_DIR, name)
        img.save(path)
        print(f"  {path}  {img.width}x{img.height}")

    print(f"\nGenerated {len(sprites)} sprites in {OUT_DIR}")


if __name__ == "__main__":
    main()
