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


def write_apng(path, frames, durations, loop_start=0, loop_end=None, repeats=1):
    if len(frames) != len(durations) or not frames:
        raise ValueError("Frame/timing mismatch")
    if frames[0].size not in ((192, 208), (256, 208)) or any(frame.size != frames[0].size for frame in frames):
        raise ValueError("APNG frames must share a supported canvas")
    loop_end = len(frames) if loop_end is None else loop_end
    if not 0 <= loop_start < loop_end <= len(frames) or not 1 <= repeats <= 8:
        raise ValueError("Invalid loop bounds")
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
    return dict(path=path.name, frameWidth=frames[0].width, frameHeight=frames[0].height, columns=1,
                durationsMs=times, loopStart=mapping[loop_start],
                loopEnd=mapping[loop_end] if loop_end < len(mapping) else len(times), loopRepeats=repeats,
                neutralBookends=False)


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
