"""Composite generated forepaw/mouth edits over the unchanged approved character.

Only the explicit local mask is admitted. Outside it, source RGBA is retained
byte-for-byte. No per-pose scale changes or full-frame crossfades are applied.
"""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter, ImageChops
from prepare_grooming_layers import PAW_MASK
from export_apng import write_apng
from build_idle_rig import opaque_fingerprint
from bake_naitang_kneading import FINGERPRINT

INDICES = list(range(16)) + [17, 18, 23]


def local_mask():
    mask = Image.new("L", (192, 208))
    ImageDraw.Draw(mask).polygon(PAW_MASK, fill=255)
    return ImageChops.multiply(mask, mask.filter(ImageFilter.GaussianBlur(0.6)))


def compose(neutral, generated, mask):
    if neutral.size != (192, 208) or generated.size != neutral.size:
        raise ValueError("Expected calibrated 192x208 cells")
    return Image.composite(generated, neutral, mask)


def build(source, grid, output):
    with Image.open(source) as image:
        neutral = image.convert("RGBA").crop((0, 0, 192, 208))
    if opaque_fingerprint(neutral) != FINGERPRINT:
        raise ValueError("The grooming mask is calibrated only to the original Naitang artwork")
    with Image.open(grid) as image:
        edited = image.convert("RGBA")
    if edited.size != (1024, 1536):
        raise ValueError("The generated grid must retain its original camera layout")
    mask = local_mask()
    frames = []
    for index in INDICES:
        x, y = index % 4 * 256 + 32, index // 4 * 256 + 24
        frame = compose(neutral, edited.crop((x, y, x + 192, y + 208)), mask)
        if index in (0, 23):
            frame = neutral.copy()
        padded = Image.new("RGBA", (256, 208))
        padded.paste(frame, (32, 0))
        frames.append(padded)
    output.mkdir(parents=True, exist_ok=True)
    source_mask = Image.new("L", edited.size)
    draw_source = ImageDraw.Draw(source_mask)
    for index in INDICES[1:-1]:
        x, y = index % 4 * 256 + 32, index // 4 * 256 + 24
        draw_source.polygon([(px + x, py + y) for px, py in PAW_MASK], fill=255)
    # Retain only the accepted model-painted region as a compact reproducible
    # source; the original full API response remains in output/imagegen.
    Image.composite(edited, Image.new("RGBA", edited.size), source_mask).save(output / "source-edits.png")
    times = [90] * len(frames)
    times[0], times[-1] = 180, 240
    clip = write_apng(output / "grooming.apng", frames, times, 6, 16, 3)
    (output / "clip.json").write_text(json.dumps(clip, indent=2) + "\n")
    contact = Image.new("RGB", (256 * 5, 226 * 4 * 2))
    draw = ImageDraw.Draw(contact)
    for background_row, background in enumerate(("white", "#171923")):
        start = background_row * 226 * 4
        draw.rectangle((0, start, contact.width, start + 226 * 4), fill=background)
        for i, frame in enumerate(frames):
            x, y = i % 5 * 256, start + i // 5 * 226
            contact.paste(frame, (x, y), frame)
            draw.text((x + 6, y + 210), f"{i}: source {INDICES[i]}", fill="black" if background_row == 0 else "white")
    contact.save(output / "contact.png")
    mask.save(output / "accepted-edit-mask.png")
    (output / "review.json").write_text(json.dumps(dict(approved=False, source=str(source), editedGrid=str(grid), sourceIndices=INDICES, method="local-masked-generated-forepaw-and-mouth", neutralEndpointsExact=True), indent=2) + "\n")
    print(json.dumps(dict(frames=len(clip["durationsMs"]), approved=False)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "grid", "output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    build(args.source, args.grid, args.output)
