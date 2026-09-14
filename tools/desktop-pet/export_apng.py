"""Export shipped art to individually playable, lossless APNG actions.

No new poses are invented here: conversion and motion-quality repair are separate.
Source WebP assets remain untouched as the reproducible source archive.
"""
import argparse
import json
import os
import tempfile
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


def _validate_timing(durations, start, end, repeats):
    if not durations or any(type(t) is not int or not 20 <= t <= 10000 for t in durations):
        raise ValueError("APNG frame durations must be integer milliseconds in 20..10000")
    if any(type(v) is not int for v in (start, end, repeats)) or not (0 <= start < end <= len(durations) and 1 <= repeats <= 8):
        raise ValueError("Invalid loop bounds")
    if sum(durations) + (repeats-1)*sum(durations[start:end]) > 60000:
        raise ValueError("APNG action exceeds 60 seconds")


def read_apng_frames(path, spec):
    """Decode the serialized playback source, not the pre-coalescing inputs."""
    path = Path(path)
    if path.stat().st_size > 16*1024*1024:
        raise ValueError("APNG exceeds 16 MiB")
    _validate_timing(spec["durationsMs"], spec["loopStart"], spec["loopEnd"], spec["loopRepeats"])
    size = spec["frameWidth"], spec["frameHeight"]
    if size not in ((192, 208), (256, 208)) or spec["columns"] != 1 or spec.get("neutralBookends"):
        raise ValueError("Unsupported APNG layout")
    frames = []
    with Image.open(path) as image:
        if image.format != "PNG" or image.size != size or not 2 <= image.n_frames <= 128 or image.n_frames != len(spec["durationsMs"]):
            raise ValueError("APNG frame count or geometry differs from manifest")
        for i, duration in enumerate(spec["durationsMs"]):
            image.seek(i)
            if abs(image.info["duration"]-duration) > .01:
                raise ValueError("APNG timing differs from manifest")
            frames.append(image.convert("RGBA"))
    return frames


def write_apng(path, frames, durations, loop_start=0, loop_end=None, repeats=1):
    path = Path(path)
    if len(frames) != len(durations) or not frames:
        raise ValueError("Frame/timing mismatch")
    if frames[0].size not in ((192, 208), (256, 208)) or any(frame.size != frames[0].size for frame in frames):
        raise ValueError("APNG frames must share a supported canvas")
    if any(frame.mode != "RGBA" for frame in frames):
        raise ValueError("APNG source frames must be RGBA")
    loop_end = len(frames) if loop_end is None else loop_end
    _validate_timing(durations, loop_start, loop_end, repeats)
    # Coalesce only byte-identical neighboring frames; preserve total duration.
    unique, times, mapping = [], [], []
    for index, (frame, duration) in enumerate(zip(frames, durations)):
        if unique and frame.tobytes() == unique[-1].tobytes():
            if index in (loop_start, loop_end):
                raise ValueError("Identical frames cross a loop boundary; repair timing explicitly")
            times[-1] += duration
        else:
            unique.append(frame)
            times.append(duration)
        mapping.append(len(unique) - 1)
    # All action files are genuinely animated PNGs, even a stationary pose bank.
    if len(unique) < 2:
        raise ValueError("Action has no changing frames")
    if len(unique) > 128:
        raise ValueError("APNG exceeds 128 encoded frames")
    spec = dict(path=path.name, frameWidth=frames[0].width, frameHeight=frames[0].height, columns=1,
                durationsMs=times, loopStart=mapping[loop_start],
                loopEnd=mapping[loop_end] if loop_end < len(mapping) else len(times), loopRepeats=repeats,
                neutralBookends=False)
    _validate_timing(times, spec["loopStart"], spec["loopEnd"], repeats)
    # Only replace an existing candidate once the temporary file round-trips.
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}-", suffix=".tmp", delete=False) as handle:
        temporary = Path(handle.name)
    try:
        unique[0].save(temporary, format="PNG", save_all=True, append_images=unique[1:],
                       duration=times, loop=0, disposal=0, blend=0, optimize=False)
        decoded = read_apng_frames(temporary, spec)
        for i, (actual, expected) in enumerate(zip(decoded, unique)):
            if actual.tobytes() != expected.tobytes():
                raise ValueError(f"APNG pixel roundtrip failed at frame {i}")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    return spec


def export_pet(source, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output:
        raise ValueError("Use a separate APNG output directory")
    output.mkdir(parents=True, exist_ok=True)
    atlas = Image.open(source / "spritesheet.webp").convert("RGBA")
    cell = lambda row, col: atlas.crop((col * 192, row * 208, (col + 1) * 192, (row + 1) * 208))
    neutral = cell(0, 0)
    def pad(frame):
        canvas = Image.new("RGBA", (256, 208))
        canvas.paste(frame, (32, 0))
        return canvas
    clips = {}
    for name, (row, durations) in ROWS.items():
        indices = [0, 1, 0] if name == "idle" else range(len(durations))
        frames = [pad(cell(row, column)) for column in indices]
        clips[name] = write_apng(output / f"{name}.apng", frames, durations)
    # This is a seekable 16-pose bank, not a free-running look-around action.
    clips["look"] = write_apng(output / "look.apng",
                               [pad(cell(9 + i // 8, i % 8)) for i in range(16)], [100] * 16)
    specs = json.loads((source / "motion-clips.json").read_text())
    for name, spec in specs.items():
        asset = source / spec["path"]
        if name == "tail-wag" and not asset.exists():
            asset = source / "tail-wag.webp"  # shipped install filename differs
        frames = effective_frames(Image.open(asset).convert("RGBA"), spec, neutral)
        clips[name] = write_apng(output / f"{name}.apng", [pad(frame) for frame in frames], spec["durationsMs"],
                                 spec["loopStart"], spec["loopEnd"], spec["loopRepeats"])
        if sum(clips[name]["durationsMs"][i] for i in motion_sequence(clips[name])) != sum(spec["durationsMs"][i] for i in motion_sequence(spec)):
            raise ValueError("Action duration changed during APNG export")
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
