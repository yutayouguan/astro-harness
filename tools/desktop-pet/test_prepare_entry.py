import tempfile
import unittest
from pathlib import Path
from PIL import Image, ImageDraw
from prepare_entry import prepare


class EntryPreparationTests(unittest.TestCase):
    def test_keeps_exact_endpoints_and_never_marks_candidate_approved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            art = root / "art"
            art.mkdir()
            source = Image.new("RGBA", (600, 600))
            draw = ImageDraw.Draw(source)
            for i in range(6):
                x, y = 30 + i % 3 * 200, 30 + i // 3 * 300
                draw.rectangle((x, y, x + 80, y + 180), fill=(200, 100 + i, 40, 255))
            source.save(art / "poses.png")
            neutral = Image.new("RGBA", (192, 208))
            ImageDraw.Draw(neutral).rectangle((60, 30, 130, 201), fill=(220, 130, 40, 255))
            target = neutral.copy()
            target.putpixel((90, 90), (255, 255, 255, 255))
            neutral.save(art / "neutral.png")
            target.save(art / "target.png")
            report = prepare(art / "poses.png", art / "neutral.png", art / "target.png", root / "qa")
            self.assertFalse(report["approved"])
            self.assertEqual(Image.open(root / "qa/frame-0.png").tobytes(), neutral.tobytes())
            self.assertEqual(Image.open(root / "qa/frame-5.png").tobytes(), target.tobytes())
            self.assertEqual({item["baseline"] for item in report["metrics"]}, {202})

    def test_refuses_output_inside_source_art(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                prepare(root / "poses.png", root / "neutral.png", root / "target.png", root / "nested")


if __name__ == "__main__":
    unittest.main()
