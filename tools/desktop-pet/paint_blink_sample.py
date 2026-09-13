"""Prepare and composite a hand/AI-painted blink study; never installs a pet.

The painter edits a registered 4x2 sheet. Only eye RGB is admitted; original
RGBA outside the eye masks and original alpha everywhere stay byte-exact.
"""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

from export_apng import write_apng

SIZE = (192, 208)
GRID = (1536, 1024)
STAGES = (0, 25, 50, 75, 100, 75, 50, 25)
# The painter opened cell 7 too early and changed its iris. Omit it rather
# than introducing a different open eye immediately before the original.
ACCEPTED_CELLS = (0, 1, 2, 3, 4, 5, 6)


def eye_mask():
    mask = Image.new("L", SIZE)
    draw = ImageDraw.Draw(mask)
    for x, y, rx, ry in ((92, 69, 14, 16), (123, 58, 12, 15)):
        draw.ellipse((x-rx, y-ry, x+rx, y+ry), fill=255)
    return ImageChops.multiply(mask, mask.filter(ImageFilter.GaussianBlur(.7)))


def read_neutral(path):
    with Image.open(path) as image:
        if image.size != SIZE:
            raise ValueError("Use the registered 192x208 original neutral PNG")
        return image.convert("RGBA")


def origin(index):
    return (index % 4 * 384, index // 4 * 512 + 48)


def prepare(source, output):
    neutral = read_neutral(source)
    output.mkdir(parents=True, exist_ok=False)
    neutral.save(output / "neutral.png")
    grid = Image.new("RGBA", GRID, (255, 0, 255, 255))
    edit_mask = Image.new("RGBA", GRID, (0, 0, 0, 255))
    enlarged = neutral.resize((384, 416), Image.Resampling.LANCZOS)
    hole = Image.new("RGBA", (384, 416))
    hole.putalpha(ImageChops.invert(eye_mask().resize(hole.size)))
    for index in range(8):
        grid.alpha_composite(enlarged, origin(index))
        if index:
            edit_mask.paste(hole, origin(index))
    grid.save(output / "edit-grid.png")
    edit_mask.save(output / "edit-mask.png")
    (output / "source.json").write_text(json.dumps({
        "source": str(source), "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "stagesPercentClosed": STAGES, "installed": False,
    }, indent=2) + "\n")


def composite(neutral, painted):
    if neutral.size != SIZE or painted.size != SIZE:
        raise ValueError("Unexpected cell dimensions")
    result = Image.composite(painted, neutral, eye_mask())
    result.putalpha(neutral.getchannel("A"))
    return result


def compose(output, painted_path):
    neutral = read_neutral(output / "neutral.png")
    with Image.open(painted_path) as image:
        if image.size != GRID:
            raise ValueError("Painter changed the sheet dimensions; do not auto-rescale")
        painted = image.convert("RGBA")
    frames = [neutral.copy()]
    for index in ACCEPTED_CELLS[1:]:
        x, y = origin(index)
        cell = painted.crop((x, y, x+384, y+416)).resize(SIZE, Image.Resampling.LANCZOS)
        frames.append(composite(neutral, cell))
    frames.append(neutral.copy())
    padded = []
    for index, frame in enumerate(frames):
        tile = Image.new("RGBA", (256, 208))
        tile.paste(frame, (32, 0))
        padded.append(tile)
        tile.save(output / f"frame-{index:02}.png")
    durations = [1800, 40, 40, 40, 70, 50, 60, 600]
    clip = write_apng(output / "blink-study.apng", padded, durations)
    (output / "clip.json").write_text(json.dumps(clip, indent=2) + "\n")
    contact = Image.new("RGB", (256*3, 232*3*3))
    for bg_row, color in enumerate(("white", "#171923", "#c8ccd3")):
        for index, frame in enumerate(padded):
            x, y = index % 3 * 256, bg_row*696 + index//3*232
            bg = Image.new("RGB", (256, 232), color)
            if bg_row == 2:
                d = ImageDraw.Draw(bg)
                for yy in range(0, 208, 16):
                    for xx in range(0, 256, 16):
                        if (xx//16+yy//16) % 2:
                            d.rectangle((xx, yy, xx+15, yy+15), fill="#edf0f5")
            bg.paste(frame, (0, 0), frame)
            ImageDraw.Draw(bg).text((8, 212), f"{index} / {durations[index]} ms", fill="white" if bg_row==1 else "black")
            contact.paste(bg, (x, y))
    contact.save(output / "contact.png")
    previews = []
    for frame in padded:
        bg = Image.new("RGB", frame.size, "#f4f1eb")
        bg.paste(frame, (0, 0), frame)
        previews.append(bg.resize((512, 416), Image.Resampling.LANCZOS))
    for name, speed in (("preview", 1), ("slow", 3)):
        previews[0].save(output / f"{name}.gif", save_all=True, append_images=previews[1:],
                         duration=[t*speed for t in durations], loop=0, disposal=2)
    (output / "review.json").write_text(json.dumps({
        "approved": False, "installed": False, "method": "painted-eye-frames-original-body-alpha",
        "acceptedSourceCells": ACCEPTED_CELLS,
        "rejectedSourceCells": {"7": "fully open instead of partial reopening; iris differs from original"},
        "uniqueFrames": len({frame.tobytes() for frame in frames}),
        "originalAlphaExact": all(frame.getchannel("A").tobytes()==neutral.getchannel("A").tobytes() for frame in frames),
        "endpointsExact": frames[0].tobytes()==frames[-1].tobytes(),
        "requiresVisualReview": ["eye registration", "lid progression", "fur seams", "native-size playback"],
    }, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("source", type=Path)
    p.add_argument("output", type=Path)
    c = sub.add_parser("compose")
    c.add_argument("output", type=Path)
    c.add_argument("painted", type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.source, args.output)
    else:
        compose(args.output, args.painted)
