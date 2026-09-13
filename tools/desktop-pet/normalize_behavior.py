"""Normalize generated seated behaviour without rescaling individual poses.

Retains the accepted neutral at both ends and records review candidates only.
The frame list and body loop are explicit, not inferred from file names.
"""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw
from assemble_motion import pose_bounds
from audit_motion import frame_metrics
from export_apng import write_apng


def normalize(source, neutral_path, output, indices, loop_start, loop_end, duration=70):
    source, neutral_path, output = Path(source).resolve(), Path(neutral_path).resolve(), Path(output).resolve()
    if output == source.parent or source.parent in output.parents:
        raise ValueError("Review output must be separate from source art")
    if (not indices or any(not 0 <= i < 24 for i in indices)
            or not 0 < loop_start < loop_end < len(indices) or not 20 <= duration <= 2000):
        raise ValueError("Invalid behaviour sequence")
    with Image.open(source) as image:
        art = image.convert("RGBA")
    with Image.open(neutral_path) as image:
        neutral = image.convert("RGBA")
    if neutral.size != (256, 208):
        raise ValueError("Expected the canonical padded neutral")
    boxes = pose_bounds(art, 4, 6)
    info = frame_metrics(neutral)
    scale = (info["bounds"][3] - info["bounds"][1]) / (boxes[0][3] - boxes[0][1])
    # Generated ears may rise above the reference. Use one safe scale for the
    # entire sheet, never crop ears or independently resize individual frames.
    scale = min(scale, (info["baseline"] - 4) / max(boxes[i][3] - boxes[i][1] + 8 for i in indices))
    frames = []
    for i in indices:
        left, top, right, bottom = boxes[i]
        cut = art.crop((left - 4, top - 4, right + 4, bottom + 4))
        cut = cut.resize((round(cut.width * scale), round(cut.height * scale)), Image.Resampling.LANCZOS)
        metrics = frame_metrics(cut)
        # A stable seated tail/hindquarters anchor; do not fit every pose's height.
        x = round(info["bounds"][0] - metrics["bounds"][0])
        y = info["baseline"] - metrics["baseline"]
        frame = Image.new("RGBA", neutral.size)
        frame.alpha_composite(cut, (x, y))
        if sum(frame.getchannel("A").histogram()[1:]) != sum(cut.getchannel("A").histogram()[1:]):
            raise ValueError(f"Pose {i} clips at the common camera scale")
        frames.append(frame)
    frames[0] = neutral.copy()
    frames[-1] = neutral.copy()
    times = [duration] * len(frames)
    times[0], times[-1] = 180, 240
    output.mkdir(parents=True, exist_ok=True)
    clip = write_apng(output / "action.apng", frames, times, loop_start, loop_end, 3)
    for i, frame in enumerate(frames):
        frame.save(output / f"frame-{i:02}.png")
    rows = (len(frames) + 5) // 6
    contact = Image.new("RGB", (256 * 6, 226 * rows * 2))
    draw = ImageDraw.Draw(contact)
    for j, background in enumerate(("white", "#171923")):
        offset = j * rows * 226
        draw.rectangle((0, offset, contact.width, offset + rows * 226), fill=background)
        for i, frame in enumerate(frames):
            at = (i % 6 * 256, offset + i // 6 * 226)
            contact.paste(frame, at, frame)
            draw.text((at[0] + 4, at[1] + 208), str(i), fill="black" if j == 0 else "white")
    contact.save(output / "contact.png")
    (output / "clip.json").write_text(json.dumps(clip, indent=2) + "\n")
    report = dict(source=str(source), sourceIndices=indices, scale=scale, approved=False,
                  metrics=[frame_metrics(frame) for frame in frames])
    (output / "review.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(dict(frames=len(frames), scale=scale, approved=False)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "neutral", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--indices", required=True)
    parser.add_argument("--loop-start", type=int, required=True)
    parser.add_argument("--loop-end", type=int, required=True)
    parser.add_argument("--duration", type=int, default=70)
    args = parser.parse_args()
    normalize(args.source, args.neutral, args.output, [int(i) for i in args.indices.split(",")],
              args.loop_start, args.loop_end, args.duration)
