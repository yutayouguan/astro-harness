"""Extract trusted PNG idle layers from the shipped character, not new AI art.

The opaque-pixel fingerprint binds calibration to the exact neutral artwork.
APNG actions remain independent; this script never modifies installed pets.
"""
import argparse
import hashlib
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageOps
from bake_companion_gestures import blink, EYES
from bake_pudding_tail import masks as pudding_masks


def opaque_fingerprint(image):
    # Browser canvases premultiply translucent fur. Ignore those rounding-sensitive
    # pixels, but compare every fully opaque pixel's RGB and occupancy exactly.
    data = bytearray(image.tobytes())
    for at in range(0, len(data), 4):
        if data[at + 3] != 255:
            data[at:at + 4] = bytes(4)
    return hashlib.sha256(data).hexdigest()


def masked(image, mask):
    out = image.copy()
    out.putalpha(ImageChops.multiply(image.getchannel("A"), mask))
    return out


def build(source, output):
    pet = source.name
    with Image.open(source / "spritesheet.webp") as image:
        atlas = image.convert("RGBA")
    neutral = atlas.crop((0, 0, 192, 208))
    closed = atlas.crop((192, 0, 384, 208))
    output.mkdir(parents=True, exist_ok=True)
    cut = 116 if pet == "naitang" else 96
    tail_mask = Image.new("L", neutral.size)
    if pet == "pudding":
        tail_mask, _, tail_source, _, _ = pudding_masks(neutral)
        pivot = [57, 173]
    else:
        ImageDraw.Draw(tail_mask).polygon([(24, 175), (46, 166), (67, 179), (81, 191), (77, 203), (24, 203)], fill=255)
        tail_source = tail_mask.copy()
        pivot = [78, 190]
    body = masked(neutral, ImageOps.invert(tail_mask))
    body.paste((0, 0, 0, 0), (0, 0, 192, cut))
    # A short neck underlap fills only the subpixel head-tracking displacement.
    # It is hidden behind the neutral head and never affects the paws.
    neck = neutral.crop((0, cut, 192, cut + 4))
    body.paste(neck, (0, cut - 4))
    body.save(output / "body.png")
    masked(neutral, tail_source).save(output / "tail.png")
    neutral.save(output / "neutral.png")
    heads = Image.new("RGBA", (192 * 9, 208))
    for i in range(9):
        head = blink(neutral, closed, i / 8, pet)
        head.paste((0, 0, 0, 0), (0, cut + 4, 192, 208))
        heads.paste(head, (i * 192, 0))
    heads.save(output / "heads.png")
    # Ear triangles/ellipses remain part of the same head bitmap. Their local
    # displacement is rendered through fixed masks, never a second full pet.
    eyes = [dict(x=x, y=y, rx=ax, ry=ay) for x, y, _, _, ax, ay, _, _ in EYES[pet]]
    config = dict(version=1, pet=pet, width=192, height=208, headCut=cut,
                  fingerprint=opaque_fingerprint(neutral), tailPivot=pivot,
                  tailDegrees=2 if pet == "naitang" else 7,
                  eyes=eyes,
                  ears=([[58, 49, 10, 12], [126, 20, 10, 13]] if pet == "naitang"
                        else [[61, 63, 10, 19], [148, 51, 8, 17]]))
    (output / "rig.json").write_text(json.dumps(config, indent=2) + "\n")
    return config


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(build(args.source, args.output)))
