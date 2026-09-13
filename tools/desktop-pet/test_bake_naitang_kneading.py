import unittest
import tempfile
import json
from pathlib import Path
from PIL import Image
from bake_naitang_kneading import articulate, lift_pair, sequence, bake

ROOT = Path(__file__).resolve().parents[2]


class KneadingTests(unittest.TestCase):
    def test_bundled_action_reproduces_exactly_with_matching_loop_metadata(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            source = ROOT / "apps/desktop/src/assets/pets/naitang"
            bake(source / "spritesheet.webp", output)
            self.assertEqual((output / "kneading.apng").read_bytes(), (source / "apng/kneading.apng").read_bytes())
            self.assertEqual(json.loads((output / "clip.json").read_text()), json.loads((source / "apng/motion-clips.json").read_text())["kneading"])

    def test_one_forepaw_remains_planted_and_the_other_has_a_bounded_lift(self):
        for index in range(256):
            a, b = lift_pair(index / 256)
            self.assertTrue(a == 0 or b == 0)
            self.assertTrue(0 <= a <= 5 and 0 <= b <= 5)

    def test_face_body_and_tail_do_not_change_and_endpoints_are_exact(self):
        with Image.open(ROOT / "apps/desktop/src/assets/pets/naitang/spritesheet.webp") as image:
            neutral = image.convert("RGBA").crop((0, 0, 192, 208))
        for lifts in sequence():
            frame = articulate(neutral, *lifts)
            self.assertEqual(frame.crop((0, 0, 192, 146)).tobytes(), neutral.crop((0, 0, 192, 146)).tobytes())
            self.assertEqual(frame.crop((0, 146, 76, 208)).tobytes(), neutral.crop((0, 146, 76, 208)).tobytes())
        self.assertEqual(articulate(neutral, *sequence()[0]).tobytes(), neutral.tobytes())
        self.assertEqual(articulate(neutral, *sequence()[-1]).tobytes(), neutral.tobytes())


if __name__ == "__main__":
    unittest.main()
