"""Bake tail-only motion from Pudding's existing AI-created neutral pixels.

The puppy body is copied byte-for-byte in every frame. Only the separated tail
texture is articulated around its fixed root; no new AI frames are claimed.
Requires Pillow. Does not call a model or modify installed pet state.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageOps

CELL = (192, 208)
TAIL_OUTLINE = [(0, 132), (48, 132), (48, 158), (51, 162),
                (51, 166), (56, 170), (56, 184), (0, 184)]
PIVOT = (57, 173)
SWEEP = (8, 125, 72, 188)
NEUTRAL_SHA256 = "6a871c6701e45b4dd6b06d97dab481b7392b0f7c90f575cde14362e19d81bdad"


def masks(neutral):
    visible_tail = Image.new("L", CELL)
    ImageDraw.Draw(visible_tail).polygon(TAIL_OUTLINE, fill=255)
    body = ImageChops.multiply(neutral.getchannel("A").point(lambda a: 255 if a else 0),
                               ImageOps.invert(visible_tail))
    tail_source = visible_tail.copy()
    # Underlap behind the fixed foreground hip prevents a detached tail root.
    ImageDraw.Draw(tail_source).rectangle((49, 165, 68, 180), fill=255)
    sweep = Image.new("L", CELL)
    # Public/test coordinates are half-open; Pillow rectangles are inclusive.
    ImageDraw.Draw(sweep).rectangle((SWEEP[0], SWEEP[1], SWEEP[2]-1, SWEEP[3]-1), fill=255)
    protected = ImageChops.lighter(body, ImageOps.invert(sweep))
    return visible_tail, body, tail_source, sweep, protected


def angles(amplitude=17, entry=8, cycle=32, exit_frames=9):
    # Hermite entry/exit match the loop's angular velocity and settle at rest.
    velocity = amplitude * 2 * math.pi * entry / cycle
    entering = [velocity * ((i / entry)**3 - (i / entry)**2) for i in range(entry)]
    loop = [amplitude * math.sin(2 * math.pi * i / cycle) for i in range(cycle)]
    leaving = [velocity * ((i / (exit_frames-1))**3 - 2*(i / (exit_frames-1))**2
                           + i / (exit_frames-1)) for i in range(exit_frames)]
    return entering + loop + leaving


def render(neutral, amplitude=17):
    neutral = neutral.convert("RGBA")
    if neutral.size != CELL:
        raise ValueError("Expected the 192x208 neutral cell")
    visible_tail, body_mask, source_mask, sweep, protected = masks(neutral)
    empty = Image.new("RGBA", CELL)
    body = Image.composite(empty, neutral, visible_tail)
    tail = Image.composite(neutral, empty, source_mask)
    large_tail = tail.resize((CELL[0]*4, CELL[1]*4), Image.Resampling.LANCZOS)
    frames = []
    for angle in angles(amplitude):
        if abs(angle) < 1e-9:
            frame = neutral.copy()
        else:
            moved = large_tail.rotate(angle, resample=Image.Resampling.BICUBIC,
                                      center=(PIVOT[0]*4, PIVOT[1]*4))
            moved = moved.resize(CELL, Image.Resampling.LANCZOS)
            frame = Image.alpha_composite(moved, body)
            # Restore even the antialiased body edge exactly, not merely its
            # opaque interior. The moving tail cannot move or recolor the puppy.
            frame = Image.composite(neutral, frame, protected)
            frame = Image.composite(frame, empty, frame.getchannel("A").point(lambda a: 255 if a else 0))
        frames.append(frame)
    return frames, protected, visible_tail


def bake(atlas_path, output):
    neutral = Image.open(atlas_path).convert("RGBA").crop((0, 0, *CELL))
    if hashlib.sha256(neutral.tobytes()).hexdigest() != NEUTRAL_SHA256:
        raise ValueError("This tail mask is calibrated to Pudding's approved neutral; review the mask for changed art")
    frames, protected, tail_mask = render(neutral)
    sheet = Image.new("RGBA", (768, math.ceil(len(frames)/4)*208))
    protected_reference = Image.composite(neutral, Image.new("RGBA", CELL), protected).tobytes()
    for i, frame in enumerate(frames):
        if Image.composite(frame, Image.new("RGBA", CELL), protected).tobytes() != protected_reference:
            raise ValueError(f"Body changed in frame {i}")
        sheet.alpha_composite(frame, (i % 4 * 192, i // 4 * 208))
    if frames[0].tobytes() != neutral.tobytes() or frames[-1].tobytes() != neutral.tobytes():
        raise ValueError("Bookends differ from neutral")
    output.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(output, lossless=True, exact=True)
    protected.save(output.with_suffix(".body-mask.png"))
    tail_mask.save(output.with_suffix(".tail-mask.png"))
    durations = [40] * len(frames)
    durations[0], durations[-1] = 180, 220
    clip = dict(path="tail-wag-v2.webp", frameWidth=192, frameHeight=208, columns=4,
                durationsMs=durations, loopStart=8, loopEnd=40, loopRepeats=2, neutralBookends=True)
    output.with_suffix(".json").write_text(json.dumps(clip, indent=2)+"\n")
    playback = list(range(8)) + list(range(8, 40))*2 + list(range(40, len(frames)))
    for background in ("white", "black", "checker"):
        contact = Image.new("RGB", sheet.size, "white" if background == "checker" else background)
        if background == "checker":
            draw = ImageDraw.Draw(contact)
            for y in range(0,contact.height,12):
                for x in range(0,contact.width,12):
                    draw.rectangle((x,y,x+11,y+11),fill="#eee" if (x//12+y//12)%2 else "#bbb")
        contact.paste(sheet, (0,0), sheet)
        contact.save(output.with_suffix(f".{background}.png"))
    preview = []
    for i in playback:
        canvas = Image.new("RGB", CELL, "white")
        canvas.paste(frames[i], (0,0), frames[i])
        preview.append(canvas)
    for suffix, speed in ((".gif",1),(".slow.gif",3)):
        preview[0].save(output.with_suffix(suffix), save_all=True, append_images=preview[1:],
                        duration=[durations[i]*speed for i in playback], loop=0, disposal=2)
    report = dict(ok=True, source=str(atlas_path), method="fixed-body-articulated-original-tail",
                  newModelRequests=0, frames=len(frames), playbackFrames=len(playback),
                  protectedPixels=sum(a>0 for a in protected.getdata()), bodyChangedPixels=0,
                  bodyDriftPixels=0, pivot=PIVOT, angles=angles(),
                  neutralSha256=hashlib.sha256(neutral.tobytes()).hexdigest(),
                  assetSha256=hashlib.sha256(output.read_bytes()).hexdigest())
    output.with_suffix(".qa.json").write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps({k:v for k,v in report.items() if k != "angles"}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("atlas", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    bake(args.atlas, args.output)
