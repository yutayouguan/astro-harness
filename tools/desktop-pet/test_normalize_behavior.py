import json
import tempfile
import unittest
from pathlib import Path
from PIL import Image, ImageDraw
from normalize_behavior import normalize


class BehaviorTests(unittest.TestCase):
    def test_preserves_neutral_and_uses_one_camera_scale(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            art = root / "art"
            art.mkdir()
            image = Image.new("RGBA", (1024, 1536))
            draw = ImageDraw.Draw(image)
            for i in range(24):
                x, y = i % 4 * 256 + 60, i // 4 * 256 + 40
                draw.rectangle((x, y + i % 3, x + 100, y + 180), fill=(200, 80 + i, 20, 255))
            image.save(art / "poses.png")
            neutral = Image.new("RGBA", (256, 208))
            ImageDraw.Draw(neutral).rectangle((70, 15, 160, 202), fill=(210, 140, 40, 255))
            neutral.save(art / "neutral.png")
            normalize(art / "poses.png", art / "neutral.png", root / "qa", list(range(24)), 4, 20)
            with Image.open(root / "qa/action.apng") as result:
                self.assertEqual(result.convert("RGBA").tobytes(), neutral.tobytes())
                result.seek(result.n_frames - 1)
                self.assertEqual(result.convert("RGBA").tobytes(), neutral.tobytes())
            review = json.loads((root / "qa/review.json").read_text())
            self.assertFalse(review["approved"])
            self.assertEqual(len(review["sourceIndices"]), 24)
            self.assertEqual({m["baseline"] for m in review["metrics"]}, {203})

    def test_invalid_sequence_fails_before_reading_inputs(self):
        for indices, start, end in [([], 0, 0), ([0, 25], 0, 1), ([0, 1, 2], 0, 2)]:
            with self.assertRaises(ValueError):
                normalize(Path("art/poses.png"), Path("neutral.png"), Path("qa"), indices, start, end)


if __name__ == "__main__":
    unittest.main()
