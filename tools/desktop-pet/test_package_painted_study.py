import copy
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw

from export_apng import write_apng
from package_painted_study import validate_clip


class PaintedPackageTests(unittest.TestCase):
    def fixture(self, folder):
        frames = []
        for x in (30, 31):
            frame = Image.new("RGBA", (256, 208))
            ImageDraw.Draw(frame).ellipse((x, 30, x+20, 70), fill="orange")
            frames.append(frame)
        path = folder / "idle.apng"
        spec = write_apng(path, frames, [90, 120])
        return path.read_bytes(), spec

    def test_accepts_real_apng(self):
        with tempfile.TemporaryDirectory() as tmp:
            data, spec = self.fixture(Path(tmp))
            first, last = validate_clip(data, spec)
            self.assertNotEqual(first, last)

    def test_rejects_timing_size_and_loop_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            data, spec = self.fixture(Path(tmp))
            for key, value in (("durationsMs", [80, 120]), ("frameWidth", 192), ("loopEnd", 3), ("loopRepeats", 9)):
                bad = copy.deepcopy(spec)
                bad[key] = value
                with self.assertRaises(ValueError):
                    validate_clip(data, bad)

    def test_static_png_is_not_an_animation(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            _, spec = self.fixture(folder)
            Image.new("RGBA", (256, 208)).save(folder / "static.png")
            with self.assertRaises(ValueError):
                validate_clip((folder / "static.png").read_bytes(), spec)


if __name__ == "__main__":
    unittest.main()
