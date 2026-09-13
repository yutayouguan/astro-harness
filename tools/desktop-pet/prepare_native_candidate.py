"""Package review candidates for an isolated native QA app, never for release."""
import argparse
import json
import shutil
from pathlib import Path


def prepare(repo, pet, destination, label=None):
    source = repo / "apps/desktop/src/assets/pets" / pet / "apng"
    review = repo / "output/qa/real-pet-motion" / pet
    if destination.exists():
        raise ValueError("Use a new QA destination; candidates must not overwrite a prior review")
    destination.mkdir(parents=True)
    manifest = json.loads((source / "pet.json").read_text())
    clips = manifest["motionClips"]
    assets = {name: source / clip["path"] for name, clip in clips.items()}
    for name, clip in json.loads((review / "gestures/gestures.json").read_text()).items():
        clips[name] = clip
        assets[name] = review / "gestures" / clip["path"]
    for direction in ("left", "right"):
        name = f"running-{direction}"
        clips[name] = json.loads((review / "complete" / f"{name}.json").read_text())
        assets[name] = review / "complete" / clips[name]["path"]
    # Naitang kneading/grooming now come from the source-bound bundled candidates,
    # not the earlier whole-character model grids with mismatched neutral poses.
    for name, clip in clips.items():
        target = f"{name}.apng"
        shutil.copyfile(assets[name], destination / target)
        clip["path"] = target
    manifest["id"] = f"qa-{pet}-natural-candidate"
    manifest["displayName"] = label or manifest["displayName"] + " · APNG 验收候选"
    manifest["description"] = "仅供隔离原生测试。步态与接缝尚未通过，不得作为已验收素材发布。"
    (destination / "pet.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    (destination / "motion-clips.json").write_text(json.dumps(clips, indent=2) + "\n")
    print(destination / "pet.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("pet", choices=["naitang", "pudding"])
    parser.add_argument("destination", type=Path)
    parser.add_argument("--label", help="Distinct test-library name (does not rename installed pets)")
    args = parser.parse_args()
    prepare(Path(__file__).resolve().parents[2], args.pet, args.destination.resolve(), args.label)
