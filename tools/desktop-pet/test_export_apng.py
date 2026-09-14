import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from PIL import Image
from export_apng import read_apng_frames, write_apng


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
            decoded = read_apng_frames(Path(directory) / "test.apng", spec)
            self.assertEqual([frame.tobytes() for frame in decoded],
                             [first.tobytes(), second.tobytes(), first.tobytes()])

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

    def test_failed_roundtrip_preserves_existing_candidate_and_removes_temporary(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "preserved.apng"
            path.write_bytes(b"previous candidate")
            first = Image.new("RGBA", (256, 208))
            second = first.copy()
            second.putpixel((50, 80), (255, 110, 70, 255))
            with patch("export_apng.read_apng_frames", side_effect=ValueError("decode failed")):
                with self.assertRaisesRegex(ValueError, "decode failed"):
                    write_apng(path, [first, second], [80, 90])
            self.assertEqual(path.read_bytes(), b"previous candidate")
            self.assertEqual(list(Path(directory).iterdir()), [path])

    def test_rejects_invalid_and_overlong_coalesced_timing_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.apng"
            first = Image.new("RGBA", (256, 208))
            second = first.copy()
            second.putpixel((50, 80), (255, 110, 70, 255))
            for invalid in (0, -20, 19, 10001, float("nan"), 80.5, True):
                with self.assertRaises(ValueError):
                    write_apng(path, [first, second], [80, invalid])
                self.assertFalse(path.exists())
            with self.assertRaises(ValueError):
                write_apng(path, [first, first, second], [10000, 10000, 80])
            self.assertFalse(path.exists())

    def test_rejects_long_repeated_action(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Image.new("RGBA", (256, 208))
            second = first.copy()
            second.putpixel((50, 80), (255, 110, 70, 255))
            with self.assertRaisesRegex(ValueError, "60 seconds"):
                write_apng(Path(directory) / "long.apng", [first, second], [5000, 5000], repeats=8)


if __name__ == "__main__":
    unittest.main()
