"""Export shipped art to individually playable, lossless APNG actions.

No new poses are invented here: conversion and motion-quality repair are separate.
Source WebP assets remain untouched as the reproducible source archive.
"""
import argparse
import json
from pathlib import Path
from PIL import Image
from audit_motion import effective_frames, motion_sequence

ROWS = {
    "idle": (0, [3200, 80, 620]),
    "running-right": (1, [120] * 7 + [220]),
    "running-left": (2, [120] * 7 + [220]),
    "waving": (3, [140] * 3 + [280]),
    "jumping": (4, [140] * 4 + [280]),
    "failed": (5, [140] * 7 + [240]),
    "waiting": (6, [150] * 5 + [260]),
    "running": (7, [120] * 5 + [220]),
    "review": (8, [150] * 5 + [280]),
}


def write_apng(path, frames, durations):
    if len(frames) != len(durations) or not frames:
        raise ValueError("Frame/timing mismatch")
    # Coalesce only byte-identical neighboring frames; preserve total duration.
    unique, times = [], []
    for frame, duration in zip(frames, durations):
        if unique and frame.tobytes() == unique[-1].tobytes():
            times[-1] += duration
        else:
            unique.append(frame)
            times.append(duration)
    # All action files are genuinely animated PNGs, even a stationary pose bank.
    if len(unique) < 2:
        raise ValueError("Action has no changing frames")
    unique[0].save(path, format="PNG", save_all=True, append_images=unique[1:],
                   duration=times, loop=0, disposal=0, blend=0, optimize=False)
    # Verify full RGBA and timing after decoding; detect Pillow frame coalescing.
    with Image.open(path) as result:
        if result.n_frames != len(unique):
            raise ValueError("Encoder changed frame count")
        for i, expected in enumerate(unique):
            result.seek(i)
            if result.convert("RGBA").tobytes() != expected.tobytes():
                raise ValueError(f"APNG pixel roundtrip failed at frame {i}")
            if abs(result.info["duration"] - times[i]) > 0.01:
                raise ValueError("APNG timing roundtrip failed")
    return dict(path=path.name, frameWidth=192, frameHeight=208, columns=1,
                durationsMs=times, loopStart=0, loopEnd=len(times), loopRepeats=1,
                neutralBookends=False)


def export_pet(source, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output:
        raise ValueError("Use a separate APNG output directory")
    output.mkdir(parents=True, exist_ok=True)
    atlas = Image.open(source / "spritesheet.webp").convert("RGBA")
    cell = lambda row, col: atlas.crop((col * 192, row * 208, (col + 1) * 192, (row + 1) * 208))
    neutral = cell(0, 0)
    clips = {}
    for name, (row, durations) in ROWS.items():
        indices = [0, 1, 0] if name == "idle" else range(len(durations))
        frames = [cell(row, column) for column in indices]
        clips[name] = write_apng(output / f"{name}.apng", frames, durations)
    # This is a seekable 16-pose bank, not a free-running look-around action.
    clips["look"] = write_apng(output / "look.apng",
                               [cell(9 + i // 8, i % 8) for i in range(16)], [100] * 16)
    specs = json.loads((source / "motion-clips.json").read_text())
    for name, spec in specs.items():
        asset = source / spec["path"]
        if name == "tail-wag" and not asset.exists():
            asset = source / "tail-wag.webp"  # shipped install filename differs
        frames = effective_frames(Image.open(asset).convert("RGBA"), spec, neutral)
        sequence = motion_sequence(spec)
        clips[name] = write_apng(output / f"{name}.apng", [frames[i] for i in sequence],
                                 [spec["durationsMs"][i] for i in sequence])
    (output / "motion-clips.json").write_text(json.dumps(clips, indent=2) + "\n")
    name = "奶糖" if source.name == "naitang" else "布丁"
    manifest = dict(id=f"{source.name}-apng", displayName=name,
                    description=f"{name} APNG动作包（素材自然度待验收）", spriteVersionNumber=3,
                    spritesheetPath="idle.apng", motionClips=clips)
    (output / "pet.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    return clips


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    clips = export_pet(args.source, args.output)
    print(json.dumps({name: len(clip["durationsMs"]) for name, clip in clips.items()}, indent=2))
