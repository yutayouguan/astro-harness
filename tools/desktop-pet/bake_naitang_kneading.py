"""Bake alternating forepaw kneading from Naitang's calibrated neutral pixels.

This is local raster articulation, not AI-generated in-between poses. Face,
torso, tail and hindquarters remain unchanged. Output is a review candidate.
"""
import argparse
import json
import math
from pathlib import Path
from PIL import Image, ImageDraw
from build_idle_rig import opaque_fingerprint
from export_apng import write_apng

FINGERPRINT = "314feb52eab753d1d6ef11950a9a3013ee60904309bd2788eb120348459b693c"
PAWS = [(88, 145, 118, 208), (118, 145, 143, 208)]


def smooth(t):
    t = max(0, min(1, t))
    return t * t * (3 - 2 * t)


def lift_pair(phase, strength=1):
    wave = math.sin(2 * math.pi * phase)
    return (5 * max(0, wave) ** 2 * strength, 5 * max(0, -wave) ** 2 * strength)


def sample_vertical(pixels, x, y):
    if y < 0 or y > 207:
        return (0, 0, 0, 0)
    top = int(y)
    amount = y - top
    a, b = pixels[x, top], pixels[x, min(207, top + 1)]
    alpha = a[3] * (1 - amount) + b[3] * amount
    if alpha < 0.5:
        return (0, 0, 0, 0)
    # Interpolate premultiplied values so translucent fur does not gain a dark fringe.
    rgb = tuple(round((a[c] * a[3] * (1 - amount) + b[c] * b[3] * amount) / alpha) for c in range(3))
    return rgb + (round(alpha),)


def articulate(neutral, left_lift, right_lift):
    if neutral.size != (192, 208) or any(not 0 <= value <= 5 for value in (left_lift, right_lift)):
        raise ValueError("Invalid calibrated paw motion")
    out = neutral.copy()
    source, target = neutral.load(), out.load()
    for box, lift in zip(PAWS, (left_lift, right_lift)):
        if lift < 1e-9:
            continue
        left, root, right, bottom = box
        for x in range(left, right):
            # Narrow feather only outside the paw, not across the moving toe pad.
            shoulder_weight = smooth((x - left) / 4) * smooth((right - 1 - x) / 4)
            for y in range(root + 1, bottom):
                # Move each complete toe pad; taper only where the foreleg joins
                # the torso. Tapering across toes leaves an artificial fork.
                weight = shoulder_weight + (1 - shoulder_weight) * smooth((y - 167) / 15)
                ratio = 1 - lift * weight / (203 - root)
                mapped = root + (y - root) / ratio
                if weight > 0:
                    target[x, y] = sample_vertical(source, x, mapped)
    return out


def sequence():
    entry = [lift_pair(i / 8 * 0.125, smooth(i / 8)) for i in range(8)]
    body = [lift_pair(0.125 + i / 32) for i in range(32)]
    exit_frames = [lift_pair(0.125 + i / 64, 1 - smooth(i / 8)) for i in range(9)]
    return entry + body + exit_frames


def bake(source, output):
    with Image.open(source) as image:
        neutral = image.convert("RGBA").crop((0, 0, 192, 208))
    if opaque_fingerprint(neutral) != FINGERPRINT:
        raise ValueError("Naitang neutral changed; recalibrate paw masks before baking")
    poses = sequence()
    frames = [articulate(neutral, *pose) for pose in poses]
    if frames[0].tobytes() != neutral.tobytes() or frames[-1].tobytes() != neutral.tobytes():
        raise ValueError("Neutral endpoints changed")
    padded = []
    for frame in frames:
        canvas = Image.new("RGBA", (256, 208))
        canvas.paste(frame, (32, 0))
        padded.append(canvas)
    output.mkdir(parents=True, exist_ok=True)
    times = [42] * len(frames)
    times[0], times[-1] = 180, 240
    clip = write_apng(output / "kneading.apng", padded, times, 8, 40, 3)
    (output / "clip.json").write_text(json.dumps(clip, indent=2) + "\n")
    contact = Image.new("RGB", (256 * 6, 226 * 2), "white")
    draw = ImageDraw.Draw(contact)
    selected = [0, 12, 16, 24, 32, 48]
    for row, background in enumerate(("white", "#171923")):
        draw.rectangle((0, row * 226, contact.width, (row + 1) * 226), fill=background)
        for column, index in enumerate(selected):
            contact.paste(padded[index], (column * 256, row * 226), padded[index])
            draw.text((column * 256 + 8, row * 226 + 210), str(index), fill="black" if row == 0 else "white")
    contact.save(output / "contact.png")
    report = dict(approved=False, method="calibrated-local-forepaw-articulation", newModelRequests=0,
                  source=str(source), poses=poses, bodyUnchanged=True, neutralEndpointsExact=True)
    (output / "review.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(dict(frames=len(clip["durationsMs"]), approved=False, neutralEndpointsExact=True)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    bake(args.source, args.output)
