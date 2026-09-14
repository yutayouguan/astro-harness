"""Validate and assemble 72 Blender PNG frames into a 24 fps APNG study."""
import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

PETS = ('naitang', 'pudding')
FRAME_COUNT = 72
FPS = 24


def load_frames(folder, pet):
    frames = []
    for index in range(1, FRAME_COUNT + 1):
        with Image.open(folder / f'{pet}-{index:03d}.png') as source:
            source.load()
            if source.mode != 'RGBA':
                raise ValueError(f'{pet} frame {index}: expected native RGBA')
            frame = source.copy()
        if frames and frame.size != frames[0].size:
            raise ValueError(f'{pet} frame {index}: inconsistent canvas')
        alpha = frame.getchannel('A')
        box = alpha.getbbox()
        if not box or min(box[0], box[1]) <= 0 or box[2] >= frame.width or box[3] >= frame.height:
            raise ValueError(f'{pet} frame {index}: empty or clipped silhouette')
        frames.append(frame)
    endpoint_diff = ImageChops.difference(frames[0], frames[-1])
    extrema = endpoint_diff.getextrema()
    # Cycles parallel floating-point reductions can change a handful of pixels
    # by one 8-bit level even with fixed seed and identical evaluated geometry.
    # Never accept alpha changes, visible RGB changes, or a broadly changed frame.
    if (extrema[3][1] != 0 or max(hi for _, hi in extrema[:3]) > 1
            or max(ImageStat.Stat(endpoint_diff).mean[:3]) > .001):
        raise ValueError(f'{pet}: loop endpoint differs from neutral')
    frames[-1] = frames[0].copy()
    if not any(ImageChops.difference(frames[0], f).getchannel('A').getbbox() for f in frames[1:]):
        raise ValueError(f'{pet}: no silhouette motion')
    return frames


def package(folder, study='head-study'):
    if study not in ('head-study', 'secondary-study'):
        raise ValueError('Unsupported study label')
    folder = Path(folder)
    outputs = [folder / f'{pet}-{study}.apng' for pet in PETS]
    report_path = folder / 'apng-validation.json'
    if any(p.exists() for p in outputs + [report_path]):
        raise FileExistsError('Refusing to overwrite a packaged study')
    # Validate both pets before writing either result.
    images = {pet: load_frames(folder, pet) for pet in PETS}
    durations = [round((i+1)*1000/FPS)-round(i*1000/FPS) for i in range(FRAME_COUNT)]
    reports = []
    for pet, output in zip(PETS, outputs):
        frames = images[pet]
        frames[0].save(output, format='PNG', save_all=True,
                       append_images=frames[1:], duration=durations, loop=0,
                       disposal=0, blend=0)
        with Image.open(output) as animation:
            assert animation.is_animated and animation.info['loop'] == 0
            total = 0
            source_index = 0
            for i in range(animation.n_frames):
                animation.seek(i)
                decoded = animation.convert('RGBA')
                # Pillow may coalesce identical hold frames. Compare each decoded
                # frame to its first source frame, then advance by its duration.
                diff = ImageChops.difference(decoded, frames[source_index])
                assert all(hi == 0 for _, hi in diff.getextrema())
                total += animation.info['duration']
                while source_index < FRAME_COUNT and sum(durations[:source_index+1]) <= total + .01:
                    # Also validate every original frame covered by a coalesced
                    # hold; a matching first frame alone cannot prove no loss.
                    covered = ImageChops.difference(decoded, frames[source_index])
                    assert all(hi == 0 for _, hi in covered.getextrema())
                    source_index += 1
                assert abs(sum(durations[:source_index])-total) < .01
            assert abs(total - 3000) < .01 and source_index == FRAME_COUNT
            reports.append(dict(pet=pet, study=study, source_frames=FRAME_COUNT,
                                encoded_frames=animation.n_frames,
                                duration_ms=total, size=list(animation.size),
                                lossless_decode=True, transparent=True,
                                endpoint_quantization_normalized=True,
                                production_ready=False))
    report_path.write_text(json.dumps(reports, indent=2))
    return reports


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    parser.add_argument('--study', choices=('head-study', 'secondary-study'), default='head-study')
    args = parser.parse_args()
    print(json.dumps(package(args.folder, args.study), indent=2))
