"""Prepare a fixed-camera original-art grid and a forepaw/mouth edit mask."""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw

PAW_MASK = [(86, 207), (86, 177), (82, 143), (88, 115), (99, 96), (99, 83),
            (120, 83), (133, 94), (131, 119), (119, 146), (118, 207)]


def prepare(source, output):
    with Image.open(source) as image:
        neutral = image.convert("RGBA").crop((0, 0, 192, 208))
    grid = Image.new("RGBA", (1024, 1536), (255, 0, 255, 255))
    mask = Image.new("RGBA", grid.size, (0, 0, 0, 255))
    draw = ImageDraw.Draw(mask)
    for i in range(24):
        x, y = i % 4 * 256 + 32, i // 4 * 256 + 24
        grid.alpha_composite(neutral, (x, y))
        if i not in (0, 23):
            draw.polygon([(px + x, py + y) for px, py in PAW_MASK], fill=(0, 0, 0, 0))
    output.mkdir(parents=True, exist_ok=True)
    grid.save(output / "grooming-edit-grid.png")
    mask.save(output / "grooming-edit-mask.png")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    prepare(args.source, args.output)
