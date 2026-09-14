"""Registered, locally painted Naitang actions. No API calls or installation.

Input grids are anatomical guides, not final artwork. A painter supplies the
edited sheet; only the calibrated local mask may replace original RGBA.
"""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter

from audit_motion import frame_metrics, motion_sequence
from bake_naitang_kneading import FINGERPRINT, articulate, lift_pair, smooth
from build_idle_rig import opaque_fingerprint
from export_apng import read_apng_frames, write_apng
from prepare_grooming_layers import PAW_MASK

SIZE = (192, 208)
GRID = (1536, 3072)
COUNT = 24
PAW_CROP = (84, 138, 148, 208)
PAW_GRID = (1536, 768)


def origin(i):
    return i % 4 * 384, i // 4 * 512 + 48


def mask_for(action):
    mask = Image.new("L", SIZE)
    d = ImageDraw.Draw(mask)
    if action == "kneading":
        d.polygon([(84, 208), (84, 162), (88, 138), (139, 138), (146, 167), (147, 208)], fill=255)
    elif action == "grooming":
        d.polygon(PAW_MASK, fill=255)
    else:
        raise ValueError("Unknown painted action")
    # Hard support boundary with a feather entirely inside it.
    return ImageChops.multiply(mask, mask.filter(ImageFilter.GaussianBlur(.6)))


def neutral_from(source):
    with Image.open(source) as im:
        neutral = im.convert("RGBA").crop((0, 0, *SIZE))
    if opaque_fingerprint(neutral) != FINGERPRINT:
        raise ValueError("Recalibrate masks: original Naitang fingerprint differs")
    return neutral


def prepare(source, output, action, grooming=None):
    neutral = neutral_from(source)
    poses = []
    if action == "kneading":
        for i in range(COUNT):
            envelope = smooth(min(i / 4, (23-i) / 4, 1))
            poses.append(articulate(neutral, *lift_pair((i-4)/16, envelope)))
    else:
        if grooming is None:
            raise ValueError("Grooming requires an existing APNG pose guide")
        with Image.open(grooming) as clip:
            for i in range(COUNT):
                clip.seek(round(i / (COUNT-1) * (clip.n_frames-1)))
                poses.append(clip.convert("RGBA").crop((32, 0, 224, 208)))
    poses[0], poses[-1] = neutral.copy(), neutral.copy()
    grid = Image.new("RGBA", GRID, (255, 0, 255, 255))
    mask = Image.new("RGBA", GRID, (0, 0, 0, 255))
    hole = Image.new("RGBA", (384, 416))
    hole.putalpha(ImageChops.invert(mask_for(action).resize(hole.size)))
    for i, pose in enumerate(poses):
        grid.alpha_composite(pose.resize((384, 416), Image.Resampling.LANCZOS), origin(i))
        if i not in (0, COUNT-1):
            mask.paste(hole, origin(i))
    output.mkdir(parents=True, exist_ok=False)
    neutral.save(output / "neutral.png")
    grid.save(output / "edit-grid.png")
    mask.save(output / "edit-mask.png")
    (output / "source.json").write_text(json.dumps({
        "action": action, "source": str(source), "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "groomingGuide": str(grooming) if grooming else None,
        "guideIsFinalArt": False, "grid": GRID, "cells": COUNT, "installed": False,
    }, indent=2) + "\n")


def composite(neutral, painted, action):
    if neutral.size != SIZE or painted.size != SIZE:
        raise ValueError("Wrong calibrated cell size")
    mask = mask_for(action)
    # Blend premultiplied channels to avoid black RGB from a transparent key
    # pixel bleeding into a semitransparent hair. Restore the protected region
    # exactly: round-tripping RGBa alone loses low-alpha channel precision.
    mixed = Image.composite(painted.convert("RGBa"), neutral.convert("RGBa"), mask).convert("RGBA")
    return Image.composite(mixed, neutral, mask.point(lambda v: 255 if v else 0))


def registered_cell(sheet, index, neutral):
    """Translation-only alignment measured from unchanged opaque head pixels.

    No bounding-box fitting/scaling: changing paw positions cannot move the
    body. Keep a margin around the extraction so small model drift is not
    mistaken for a genuinely clipped paw.
    """
    x, y = origin(index)
    patch = sheet.crop((x-16, y-16, x+400, y+432)).resize((208, 224), Image.Resampling.LANCZOS)
    source = np.asarray(patch).astype(np.float32)
    reference = np.asarray(neutral).astype(np.float32)
    stable = reference[:, :, 3] == 255
    stable[82:, :] = False  # mouth/forepaw can move; eyes and ears cannot
    if int(stable.sum()) < 100:
        raise ValueError("Not enough protected head pixels for registration")
    best = None
    for dy in range(-6, 7):
        for dx in range(-6, 7):
            sample = source[8+dy:216+dy, 8+dx:200+dx, :3]
            score = float(np.abs(sample[stable]-reference[:, :, :3][stable]).mean())
            choice = (score, abs(dx)+abs(dy), dx, dy)
            if best is None or choice < best:
                best = choice
    score, _, dx, dy = best
    if abs(dx) == 6 or abs(dy) == 6:
        raise ValueError(f"Registration reached search bound for cell {index}; inspect camera drift")
    return patch.crop((8+dx, 8+dy, 200+dx, 216+dy)), {"dx": dx, "dy": dy, "headMeanError": round(score, 3)}


def clean_ground_residue(cell):
    """Remove low-alpha dark key residue, not fur or opaque toe pixels.

    The magenta matte's despill turns a faint generated contact shadow into
    near-black alpha <= 63. Restrict cleanup to the ground strip; keep the
    original character outside the local replacement mask in composite().
    """
    data = np.array(cell)
    residue = (np.max(data[:, :, :3], axis=2) < 64) & (data[:, :, 3] < 64)
    residue[:190, :] = False
    data[residue] = 0
    return Image.fromarray(data)


def prepare_entry(source, target_sheet, output):
    neutral = neutral_from(source)
    with Image.open(target_sheet) as im:
        target, _ = registered_cell(im.convert("RGBA"), 1, neutral)
    target = composite(neutral, target, "grooming")
    grid = Image.new("RGBA", (1536, 1024), (255, 0, 255, 255))
    mask = Image.new("RGBA", grid.size, (0, 0, 0, 255))
    hole = Image.new("RGBA", (384, 416))
    hole.putalpha(ImageChops.invert(mask_for("grooming").resize(hole.size)))
    for i in range(8):
        grid.alpha_composite((target if i == 7 else neutral).resize((384, 416), Image.Resampling.LANCZOS), origin(i))
        if i not in (0, 7):
            mask.paste(hole, origin(i))
    output.mkdir(parents=True, exist_ok=False)
    grid.save(output / "edit-grid.png")
    mask.save(output / "edit-mask.png")
    target.save(output / "raised-target.png")


def prepare_paw(source, target_sheet, output):
    neutral = neutral_from(source)
    with Image.open(target_sheet) as im:
        target, _ = registered_cell(im.convert("RGBA"), 1, neutral)
    target = composite(neutral, clean_ground_residue(target), "kneading")
    grid = Image.new("RGBA", PAW_GRID, (255, 0, 255, 255))
    mask = Image.new("RGBA", PAW_GRID, (0, 0, 0, 255))
    local = mask_for("kneading")
    ImageDraw.Draw(local).rectangle((0, 0, 117, 208), fill=0)
    hole = Image.new("RGBA", (256, 280))
    hole.putalpha(ImageChops.invert(local.crop(PAW_CROP).resize(hole.size)))
    for i in range(8):
        x, y = i%4*384+64, i//4*384+52
        patch = (target if i==7 else neutral).crop(PAW_CROP).resize((256, 280), Image.Resampling.LANCZOS)
        grid.alpha_composite(patch, (x, y))
        if i not in (0, 7):
            mask.paste(hole, (x, y))
    output.mkdir(parents=True, exist_ok=False)
    grid.save(output / "edit-grid.png")
    mask.save(output / "edit-mask.png")


def gif_delays(durations, speed=1):
    """GIF has 10ms ticks: round the cumulative clock, not every frame.

    For example, repeated 42ms frames must not all become 40ms and steadily
    speed up. APNG remains the exact timing source; GIF is only an approximation.
    """
    elapsed = encoded_elapsed = 0
    result = []
    for duration in durations:
        elapsed += duration * speed
        next_elapsed = ((elapsed + 5) // 10) * 10
        result.append(next_elapsed - encoded_elapsed)
        encoded_elapsed = next_elapsed
    return result


def export_previews(frames, spec, output, name):
    if len(frames) != len(spec["durationsMs"]):
        raise ValueError("Preview frames must match the encoded APNG timing")
    output.mkdir(parents=True, exist_ok=True)
    rows = (len(frames)+5)//6
    for bg_name, color in (("white", "white"), ("black", "#171923"), ("checker", "#c8ccd3")):
        contact = Image.new("RGB", (256*6, 232*rows), color)
        if bg_name == "checker":
            d = ImageDraw.Draw(contact)
            for y in range(0, contact.height, 16):
                for x in range(0, contact.width, 16):
                    if (x//16+y//16)%2:
                        d.rectangle((x, y, x+15, y+15), fill="#edf0f5")
        for i, frame in enumerate(frames):
            x, y = i%6*256, i//6*232
            contact.paste(frame, (x, y), frame)
            ImageDraw.Draw(contact).text((x+4, y+211), f"{i}: {spec['durationsMs'][i]}ms", fill="white" if bg_name=="black" else "black")
        contact.save(output / f"{name}-{bg_name}.png")
    sequence = motion_sequence(spec)
    previews = []
    for i in sequence:
        bg = Image.new("RGB", frames[i].size, "#f4f1eb")
        bg.paste(frames[i], (0, 0), frames[i])
        previews.append(bg.resize((512, 416), Image.Resampling.LANCZOS))
    for suffix, speed in (("preview", 1), ("slow", 3)):
        previews[0].save(output / f"{name}-{suffix}.gif", save_all=True, append_images=previews[1:],
                         duration=gif_delays([spec["durationsMs"][i] for i in sequence], speed), loop=0, disposal=2)


def compose(directory, painted_path, entry_path=None, paw_path=None, output=None):
    meta = json.loads((directory / "source.json").read_text())
    action = meta["action"]
    neutral = neutral_from(directory / "neutral.png")
    with Image.open(painted_path) as image:
        if image.size != GRID:
            raise ValueError("Painter changed layout; do not silently rescale")
        painted = image.convert("RGBA")
    # This command requires already-keyed output, never opaque magenta.
    if painted.getchannel("A").getextrema()[0] != 0:
        raise ValueError("Remove the flat chroma background before compositing")
    frames = []
    registration = []
    cells = []
    for i in range(COUNT):
        cell, adjustment = registered_cell(painted, i, neutral)
        registration.append(adjustment)
        cells.append(neutral.copy() if i in (0, COUNT-1) else composite(neutral, clean_ground_residue(cell), action))
    source_order = list(range(COUNT))
    if entry_path:
        if action != "grooming":
            raise ValueError("Entry repair only applies to grooming")
        with Image.open(entry_path) as im:
            if im.size != (1536, 1024) or im.getchannel("A").getextrema()[0] != 0:
                raise ValueError("Invalid keyed entry repair")
            repair = im.convert("RGBA")
        entry = []
        for i in range(1, 7):
            cell, adjustment = registered_cell(repair, i, neutral)
            registration.append({"entryCell": i, **adjustment})
            entry.append(composite(neutral, clean_ground_residue(cell), action))
        # Reversing the same connected foreleg poses is intentional lowering,
        # not invented extra paintings. The rejected cross-body exit is omitted.
        cells = [neutral.copy()] + entry + cells[1:15] + list(reversed(entry)) + [neutral.copy()]
        source_order = ["neutral"] + [f"entry:{i}" for i in range(1, 7)] + list(range(1, 15)) + [f"entry:{i}" for i in range(6, 0, -1)] + ["neutral"]
    if paw_path:
        if action != "kneading" or entry_path:
            raise ValueError("Paw repair only applies to kneading")
        with Image.open(paw_path) as im:
            if im.size != PAW_GRID or im.getchannel("A").getextrema()[0] != 0:
                raise ValueError("Invalid keyed paw repair")
            repair = im.convert("RGBA")
        paw_frames = []
        local = mask_for("kneading")
        ImageDraw.Draw(local).rectangle((0, 0, 117, 208), fill=0)
        # Cell 7 retains a block from the original planted paw under the raised
        # paw. Do not accept that double-foot artifact as the peak pose.
        for i in range(1, 7):
            x, y = i%4*384+64, i//4*384+52
            patch = repair.crop((x, y, x+256, y+280)).resize((64, 70), Image.Resampling.LANCZOS)
            layer = neutral.copy()
            layer.paste(patch, PAW_CROP[:2])
            layer = clean_ground_residue(layer)
            mixed = Image.composite(layer.convert("RGBa"), neutral.convert("RGBa"), local).convert("RGBA")
            paw_frames.append(Image.composite(mixed, neutral, local.point(lambda v: 255 if v else 0)))
        # Existing painted left-paw poses ordered by observed toe elevation.
        # Do not count the reverse/lowering path as extra unique drawings.
        left_indices = [3, 2, 14, 13, 12, 5, 9]
        left = [cells[i] for i in left_indices]
        cells = [neutral.copy()] + paw_frames + list(reversed(paw_frames[:-1])) + [neutral.copy()] + left + list(reversed(left[:-1])) + [neutral.copy()]
        source_order = ["neutral"] + [f"paw:{i}" for i in range(1, 7)] + [f"paw:{i}" for i in range(5, 0, -1)] + ["neutral"] + left_indices + list(reversed(left_indices[:-1])) + ["neutral"]
    support = mask_for(action)
    outside = support.point(lambda v: 255 if v == 0 else 0)
    for i, frame in enumerate(cells):
        diff = ImageChops.difference(frame, neutral)
        for channel in diff.split():
            if ImageChops.multiply(channel, outside).getbbox():
                raise ValueError("Painter altered protected character pixels")
        padded = Image.new("RGBA", (256, 208))
        padded.paste(frame, (32, 0))
        alpha = padded.getchannel("A")
        box = alpha.getbbox()
        if not box or box[0] == 0 or box[1] == 0 or box[2] == 256 or box[3] == 208:
            raise ValueError(f"Clipped/empty frame {i}")
        frames.append(padded)
    target = output if output is not None else directory / "candidate"
    target.mkdir(parents=True, exist_ok=output is None)
    times = [80 if action=="kneading" else 90]*len(frames)
    times[0], times[-1] = 240, 320
    # A full action once until loop seams have been visually accepted.
    spec = write_apng(target / f"{action}.apng", frames, times)
    (target / "clip.json").write_text(json.dumps(spec, indent=2) + "\n")
    source_groups = []
    for i, source in enumerate(source_order):
        if i and frames[i].tobytes() == frames[i-1].tobytes():
            source_groups[-1].append(source)
        else:
            source_groups.append([source])
    source_frame_count = len(frames)
    frames = read_apng_frames(target / spec["path"], spec)
    if len(source_groups) != len(frames):
        raise ValueError("Encoded APNG provenance differs from source groups")
    export_previews(frames, spec, target, action)
    metrics = [frame_metrics(frame) for frame in frames]
    sequence = motion_sequence(spec)
    (target / "review.json").write_text(json.dumps({
        "approved": False, "installed": False, "sourceCells": source_order, "registration": registration,
        "sourceFrameCount": source_frame_count, "encodedFrameCount": len(frames),
        "encodedSourceCells": source_groups, "previewSource": "decoded-apng",
        "inputHashes": {role: hashlib.sha256(path.read_bytes()).hexdigest()
                        for role, path in (("painted", painted_path), ("entry", entry_path), ("paw", paw_path)) if path},
        "uniqueFrames": len({frame.tobytes() for frame in frames}), "protectedPixelsExact": True,
        "neutralEndpointsExact": frames[0].tobytes()==frames[-1].tobytes(),
        "baselineRange": max(m["baseline"] for m in metrics)-min(m["baseline"] for m in metrics),
        "sequence": sequence, "frames": metrics,
        "requiresVisualReview": ["anatomy", "mask seams", "action progression", "contact baseline", "entry/exit"],
    }, indent=2) + "\n")


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="command", required=True)
    make = sub.add_parser("prepare")
    make.add_argument("source", type=Path)
    make.add_argument("output", type=Path)
    make.add_argument("--action", choices=("kneading", "grooming"), required=True)
    make.add_argument("--grooming", type=Path)
    entry = sub.add_parser("prepare-entry")
    entry.add_argument("source", type=Path)
    entry.add_argument("target", type=Path)
    entry.add_argument("output", type=Path)
    paw = sub.add_parser("prepare-paw")
    paw.add_argument("source", type=Path)
    paw.add_argument("target", type=Path)
    paw.add_argument("output", type=Path)
    assemble = sub.add_parser("compose")
    assemble.add_argument("directory", type=Path)
    assemble.add_argument("painted", type=Path)
    assemble.add_argument("--entry", type=Path)
    assemble.add_argument("--paw", type=Path)
    assemble.add_argument("--output", type=Path, help="Use a new review directory; existing targets are rejected")
    args = p.parse_args()
    if args.command == "prepare":
        prepare(args.source, args.output, args.action, args.grooming)
    elif args.command == "prepare-entry":
        prepare_entry(args.source, args.target, args.output)
    elif args.command == "prepare-paw":
        prepare_paw(args.source, args.target, args.output)
    else:
        compose(args.directory, args.painted, args.entry, args.paw, args.output)
