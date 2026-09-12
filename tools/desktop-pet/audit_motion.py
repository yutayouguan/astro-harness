"""Read-only review of shipped motion clips using their runtime neutral bookends.

Writes diagnostic contact sheets, slow GIFs and measurements, never source assets.
Measurements flag pairs to inspect; they are not an automatic artistic verdict.
"""
import argparse
import hashlib
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw


def effective_frames(atlas, spec, neutral):
    width, height, columns = spec["frameWidth"], spec["frameHeight"], spec["columns"]
    count = len(spec["durationsMs"])
    if min(width, height, columns) <= 0 or count < 2:
        raise ValueError("Clip must contain at least two positive-sized cells")
    if atlas.size != (columns * width, ((count + columns - 1) // columns) * height):
        raise ValueError("Clip dimensions do not match its manifest")
    frames = [atlas.crop((i % columns * width, i // columns * height,
                          (i % columns + 1) * width, (i // columns + 1) * height))
              for i in range(count)]
    if spec.get("neutralBookends"):
        if (width, height) != neutral.size:
            raise ValueError("This audit expects neutral-sized clip cells")
        frames[0], frames[-1] = neutral.copy(), neutral.copy()
    return frames


def frame_metrics(frame):
    alpha = frame.getchannel("A")
    solid = alpha.point(lambda value: 255 if value > 32 else 0)
    bounds = solid.getbbox()
    if bounds is None:
        raise ValueError("Empty motion frame")
    left, top, right, bottom = bounds
    band_top = max(top, bottom - max(1, (bottom - top) // 5))
    footprint = solid.crop((0, band_top, frame.width, bottom)).getbbox()
    return {
        "bounds": list(bounds),
        "silhouetteArea": sum(solid.histogram()[1:]),
        "baseline": bottom,
        "footprintCenter": (footprint[0] + footprint[2]) / 2,
        "sha256": hashlib.sha256(frame.tobytes()).hexdigest(),
    }


def motion_sequence(spec):
    """Match petMotionClip.ts: one entry, repeated loop, one exit."""
    count = len(spec["durationsMs"])
    if (not 0 <= spec["loopStart"] < spec["loopEnd"] <= count
            or spec["loopRepeats"] < 1
            or any(duration <= 0 for duration in spec["durationsMs"])):
        raise ValueError("Invalid motion timing or loop bounds")
    return (list(range(spec["loopStart"]))
            + list(range(spec["loopStart"], spec["loopEnd"])) * spec["loopRepeats"]
            + list(range(spec["loopEnd"], count)))


def analyze(assets, output):
    assets, output = Path(assets).resolve(), Path(output).resolve()
    if output == assets or assets in output.parents:
        raise ValueError("Diagnostics must not be written into source assets")
    output.mkdir(parents=True, exist_ok=True)
    neutral = Image.open(assets / "spritesheet.webp").convert("RGBA").crop((0, 0, 192, 208))
    neutral.save(output / "neutral.png")
    specs = json.loads((assets / "motion-clips.json").read_text())
    report = {"assets": str(assets), "clips": {}}
    for name, spec in specs.items():
        if (spec["frameWidth"], spec["frameHeight"]) != neutral.size:
            raise ValueError("This audit supports the shipped 192x208 clip format")
        sequence = motion_sequence(spec)
        atlas = Image.open(assets / spec["path"]).convert("RGBA")
        frames = effective_frames(atlas, spec, neutral)
        count = len(frames)
        frames[1].save(output / f"{name}-entry-target.png")
        rows = (count + 7) // 8
        contact = Image.new("RGB", (8 * 192, rows * 234 * 2), "white")
        draw = ImageDraw.Draw(contact)
        for background_index, background in enumerate(("white", "#171923")):
            top = background_index * rows * 234
            draw.rectangle((0, top, contact.width, top + rows * 234), fill=background)
            for i, frame in enumerate(frames):
                x, y = i % 8 * 192, top + i // 8 * 234
                contact.paste(frame, (x, y), frame)
                phase = "entry" if i < spec["loopStart"] else "loop" if i < spec["loopEnd"] else "exit"
                draw.text((x + 4, y + 210), f"{i}: {phase} {spec['durationsMs'][i]}ms", fill="black" if background_index == 0 else "white")
        contact.save(output / f"{name}-contact.png")
        rendered = []
        for i in sequence:
            canvas = Image.new("RGB", frames[i].size, "white")
            canvas.paste(frames[i], (0, 0), frames[i])
            rendered.append(canvas)
        rendered[0].save(output / f"{name}-slow.gif", save_all=True,
                         append_images=rendered[1:], loop=0, disposal=2,
                         duration=[spec["durationsMs"][i] * 3 for i in sequence])
        metrics = [frame_metrics(frame) for frame in frames]
        pairs = []
        for a, b in dict.fromkeys(zip(sequence, sequence[1:])):
            alpha_diff = ImageChops.difference(frames[a].getchannel("A"), frames[b].getchannel("A"))
            changed = sum(alpha_diff.point(lambda value: 255 if value > 32 else 0).histogram()[1:])
            pairs.append({"from": a, "to": b, "baselineDelta": metrics[b]["baseline"] - metrics[a]["baseline"],
                          "footprintDelta": round(metrics[b]["footprintCenter"] - metrics[a]["footprintCenter"], 2),
                          "changedSilhouettePixels": changed})
        report["clips"][name] = {"frames": metrics, "sequence": sequence,
                                 "baselineRange": max(m["baseline"] for m in metrics) - min(m["baseline"] for m in metrics),
                                 "bookendsIdentical": frames[0].tobytes() == frames[-1].tobytes(),
                                 "pairs": sorted(pairs, key=lambda pair: pair["changedSilhouettePixels"], reverse=True)}
    (output / "measurements.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("assets", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = analyze(args.assets, args.output)
    print(json.dumps({name: {"baselineRange": clip["baselineRange"], "bookendsIdentical": clip["bookendsIdentical"], "largestPairs": clip["pairs"][:3]}
                      for name, clip in result["clips"].items()}, indent=2))
