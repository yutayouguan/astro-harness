"""Synthetic geometry fixtures only; never pet artwork or shipped assets."""
import json
import tempfile
import unittest
from pathlib import Path
from PIL import Image, ImageDraw
from assemble_motion import assemble, pose_bounds


class MotionAssemblyTests(unittest.TestCase):
    def fixture(self, rows):
        # Height deliberately not divisible by rows: AI grids are not pixel grids.
        source = Image.new("RGBA", (400, rows * 100 + 3))
        draw = ImageDraw.Draw(source)
        for i in range(rows * 4):
            x, y = (i % 4) * 100, (i // 4) * 100
            draw.rectangle((x + 30, y + 20, x + 70, y + 80), fill=(110 + i, 80, 50, 255))
        return source

    def test_recovers_twenty_actual_poses_without_inventing_missing_frames(self):
        image = self.fixture(5)
        self.assertEqual(len(pose_bounds(image, 4, 5)), 20)
        with self.assertRaises(ValueError):
            pose_bounds(image, 4, 6)

    def test_neutral_bookends_explicit_reverse_sequence_and_hold_timing(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / "source.png"
            self.fixture(5).save(source)
            neutral = Image.new("RGBA", (192, 208))
            ImageDraw.Draw(neutral).rectangle((60, 40, 130, 180), fill=(100, 90, 70, 255))
            neutral.save(root / "neutral.png")
            sequence = [0, 1, 2, 3, 2, 1, 19]
            output = root / "clip.webp"
            assemble(source, output, sequence, 1, 6, 1, 100,
                     source_rows=5, neutral_cell=root / "neutral.png", holds={3: 250})
            manifest = json.loads(output.with_suffix(".json").read_text())
            report = json.loads(output.with_suffix(".qa.json").read_text())
            self.assertEqual(manifest["durationsMs"][3], 250)
            self.assertTrue(manifest["neutralBookends"])
            self.assertEqual(report["generatedPoseCount"], 20)
            self.assertEqual(report["uniqueSourcePoses"], 3)
            self.assertEqual(report["neutralReplacements"], [0, 6])
            sheet = Image.open(output).convert("RGBA")
            self.assertEqual(sheet.crop((0, 0, 192, 208)).tobytes(), neutral.tobytes())
            self.assertEqual(sheet.crop((384, 208, 576, 416)).tobytes(), neutral.tobytes())
            self.assertEqual(sheet.crop((576, 208, 768, 416)).getchannel("A").getbbox(), None)
            self.assertTrue(output.with_suffix(".black.png").exists())
            self.assertTrue(output.with_suffix(".checker.png").exists())
            self.assertTrue(output.with_suffix(".slow.gif").exists())


if __name__ == "__main__":
    unittest.main()
