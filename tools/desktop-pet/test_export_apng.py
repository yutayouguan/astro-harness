import tempfile
import unittest
from pathlib import Path
from PIL import Image
from export_apng import write_apng


class ApngExportTests(unittest.TestCase):
    def test_rgba_roundtrip_preserves_translucent_edges_and_duration(self):
        first = Image.new("RGBA", (192, 208))
        first.putpixel((80, 100), (200, 110, 40, 128))
        second = first.copy()
        second.putpixel((81, 100), (255, 220, 160, 255))
        with tempfile.TemporaryDirectory() as directory:
            spec = write_apng(Path(directory) / "test.apng", [first, first, second, first], [100, 80, 90, 200])
            self.assertEqual(spec["durationsMs"], [180, 90, 200])
            self.assertEqual(spec["loopEnd"], 3)

    def test_does_not_claim_static_or_mismatched_frames_are_animated(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.apng"
            image = Image.new("RGBA", (192, 208))
            with self.assertRaises(ValueError):
                write_apng(path, [image], [100, 200])
            with self.assertRaises(ValueError):
                write_apng(path, [image, image], [100, 200])

    def test_keeps_loop_indices_without_encoding_repeated_cycles(self):
        with tempfile.TemporaryDirectory() as directory:
            frames = []
            for i in range(5):
                frame = Image.new("RGBA", (192, 208))
                frame.putpixel((50 + i, 100), (255, 100, 40, 255))
                frames.append(frame)
            spec = write_apng(Path(directory) / "test.apng", frames, [100] * 5, 1, 4, 3)
            self.assertEqual((spec["loopStart"], spec["loopEnd"], spec["loopRepeats"]), (1, 4, 3))
            with Image.open(Path(directory) / "test.apng") as image:
                self.assertEqual(image.n_frames, 5)
            with self.assertRaises(ValueError):
                write_apng(Path(directory) / "invalid.apng", [frames[0], frames[0], frames[1]], [100] * 3, 1, 2, 2)


if __name__ == "__main__":
    unittest.main()
