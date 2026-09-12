"""Normalize six generated entry poses without replacing either accepted endpoint.

Outputs review candidates only; never edits installed or bundled pets.
"""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw
from assemble_motion import pose_bounds
from audit_motion import frame_metrics
from export_apng import write_apng


def prepare(source, neutral_path, target_path, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    if output == source.parent or source.parent in output.parents:
        raise ValueError("Review output must be separate from source art")
    image = Image.open(source).convert("RGBA")
    neutral = Image.open(neutral_path).convert("RGBA")
    target = Image.open(target_path).convert("RGBA")
    if neutral.size != (192, 208) or target.size != neutral.size:
        raise ValueError("Endpoints must be 192x208")
    groups = pose_bounds(image, 3, 2)
    metrics = frame_metrics(neutral)
    baseline, anchor = metrics["baseline"], metrics["footprintCenter"]
    height = metrics["bounds"][3] - metrics["bounds"][1]
    scale = height / (groups[0][3] - groups[0][1])
    frames = [neutral]
    for box in groups[1:-1]:
        left, top, right, bottom = box
        # Keep the low-alpha fur fringe rather than cutting exactly at alpha32.
        cut = image.crop((left - 6, top - 6, right + 6, bottom + 6))
        cut = cut.resize((round(cut.width * scale), round(cut.height * scale)), Image.Resampling.LANCZOS)
        part = frame_metrics(cut)
        x, y = round(anchor - part["footprintCenter"]), baseline - part["baseline"]
        if x < 0 or y < 0 or x + cut.width > 192 or y + cut.height > 208:
            raise ValueError("Generated entry clips at canonical size; repair source")
        frame = Image.new("RGBA", (192, 208))
        frame.alpha_composite(cut, (x, y))
        frames.append(frame)
    frames.append(target)
    output.mkdir(parents=True, exist_ok=True)
    for index, frame in enumerate(frames):
        frame.save(output / f"frame-{index}.png")
    clip = write_apng(output / "entry.apng", frames, [180, 60, 60, 60, 60, 180])
    contact = Image.new("RGB", (192 * len(frames), 440))
    draw = ImageDraw.Draw(contact)
    for row, background in enumerate(("white", "#171923")):
        draw.rectangle((0, row * 220, contact.width, (row + 1) * 220), fill=background)
        for index, frame in enumerate(frames):
            contact.paste(frame, (index * 192, row * 220), frame)
            draw.text((index * 192 + 6, row * 220 + 208), str(index), fill="black" if row == 0 else "white")
    contact.save(output / "contact.png")
    report = {"source": str(source), "scale": scale, "sourceIndices": [1, 2, 3, 4],
              "acceptedEndpoints": [0, 5], "metrics": [frame_metrics(frame) for frame in frames],
              "clip": clip, "approved": False}
    (output / "review.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "neutral", "target", "output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    result = prepare(args.source, args.neutral, args.target, args.output)
    print(json.dumps({"approved": False, "frames": len(result["metrics"]), "scale": result["scale"]}))
