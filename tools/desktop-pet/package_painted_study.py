"""Assemble an isolated v3 QA package; never installs or marks it released."""
import argparse
import copy
import hashlib
import io
import json
import re
import zipfile
from pathlib import Path

from PIL import Image


def validate_clip(data, spec):
    if len(data) > 16*1024*1024 or spec["columns"] != 1 or spec.get("neutralBookends"):
        raise ValueError("Unsupported APNG clip")
    width, height = spec["frameWidth"], spec["frameHeight"]
    times = spec["durationsMs"]
    if width not in (192, 256) or height != 208 or not 2 <= len(times) <= 128:
        raise ValueError("Invalid APNG dimensions/frame count")
    if not all(20 <= duration <= 10000 for duration in times):
        raise ValueError("Invalid frame duration")
    start, end, repeat = spec["loopStart"], spec["loopEnd"], spec["loopRepeats"]
    if not (0 <= start < end <= len(times) and 1 <= repeat <= 8):
        raise ValueError("Invalid loop bounds")
    if sum(times)+(repeat-1)*sum(times[start:end]) > 60000:
        raise ValueError("Clip is too long")
    with Image.open(io.BytesIO(data)) as im:
        if im.format != "PNG" or im.size != (width, height) or im.n_frames != len(times):
            raise ValueError("APNG payload differs from manifest")
        first = last = None
        for i, duration in enumerate(times):
            im.seek(i)
            if abs(im.info["duration"]-duration) > 1:
                raise ValueError("Embedded frame timing differs")
            frame = im.convert("RGBA")
            low, high = frame.getchannel("A").getextrema()
            if low != 0 or high == 0:
                raise ValueError("Opaque or empty frame")
            if i == 0:
                first = frame.tobytes()
            last = frame.tobytes()
    return first, last


def build(source, blink, actions, output):
    if output.exists() or output.with_suffix(".zip").exists():
        raise ValueError("Use a new QA destination")
    manifest = json.loads((source / "pet.json").read_text())
    clips = copy.deepcopy(manifest["motionClips"])
    if len(clips) > 24 or any(not re.fullmatch(r"[a-z-]{1,32}", name) for name in clips):
        raise ValueError("Invalid action catalog")
    files = {}
    for name, spec in clips.items():
        filename = spec["path"]
        if Path(filename).name != filename or not filename.endswith(".apng"):
            raise ValueError("QA package expects flat relative APNG paths")
        files[name] = (source / filename).read_bytes()
    replacements = {"idle": blink}
    replacements.update({name: actions/name/"candidate" for name in ("kneading", "grooming")})
    neutral = None
    for name, folder in replacements.items():
        spec = json.loads((folder / "clip.json").read_text())
        filename = spec["path"]
        if Path(filename).name != filename:
            raise ValueError("Invalid replacement asset path")
        data = (folder / filename).read_bytes()
        first, last = validate_clip(data, spec)
        if first != last or (neutral is not None and first != neutral):
            raise ValueError("Replacement action neutral seams differ")
        neutral = first
        spec["path"] = f"{name}.apng"
        clips[name], files[name] = spec, data
    for name, spec in clips.items():
        validate_clip(files[name], spec)
    required = {"idle", "running-left", "running-right", "waving", "jumping", "failed", "waiting", "running", "review", "look"}
    if not required.issubset(clips) or len(clips["look"]["durationsMs"]) != 16:
        raise ValueError("Missing required v3 actions/look directions")
    manifest.update(id="qa-naitang-painted-v1", displayName="奶糖 · 二维绘制验收候选",
                    description="眨眼、踩奶、舔爪二维绘制候选；其他动作沿用现有素材。未通过完整原生验收，不自动发布。",
                    spriteVersionNumber=3, spritesheetPath="idle.apng", motionClips=clips)
    output.mkdir(parents=True)
    for name, spec in clips.items():
        (output/spec["path"]).write_bytes(files[name])
    (output/"pet.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+"\n")
    (output/"motion-clips.json").write_text(json.dumps(clips, indent=2)+"\n")
    with zipfile.ZipFile(output.with_suffix(".zip"), "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(output.iterdir()):
            archive.write(path, path.name)
    ledger = {"approved": False, "installed": False, "replacedActions": list(replacements),
              "inheritedActions": sorted(set(clips)-set(replacements)),
              "hashes": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()},
              "validation": "PNG/APNG dimensions, timing, alpha, loop bounds, shared replacement neutral endpoints; not native or artistic acceptance"}
    (output.parent/(output.name+"-validation.json")).write_text(json.dumps(ledger, indent=2)+"\n")
    print(json.dumps({"package": str(output), "clips": len(clips), "installed": False}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "blink", "actions", "output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    build(args.source, args.blink, args.actions, args.output)
