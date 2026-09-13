import unittest
import tempfile
import json
from pathlib import Path
from PIL import Image
from compose_grooming_layers import compose, local_mask, build


class GroomingLayersTests(unittest.TestCase):
    def test_bundled_action_reproduces_from_the_retained_local_model_edit(self):
        root = Path(__file__).resolve().parents[2] / "apps/desktop/src/assets/pets/naitang"
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            build(root / "spritesheet.webp", root / "source/grooming-local-edits.png", output)
            self.assertEqual((output / "grooming.apng").read_bytes(), (root / "apng/grooming.apng").read_bytes())
            self.assertEqual(json.loads((output / "clip.json").read_text()), json.loads((root / "apng/motion-clips.json").read_text())["grooming"])

    def test_protected_original_pixels_survive_even_a_completely_different_model_output(self):
        neutral = Image.new("RGBA", (192, 208), (200, 120, 50, 127))
        generated = Image.new("RGBA", neutral.size, (30, 220, 80, 255))
        mask = local_mask()
        result = compose(neutral, generated, mask)
        self.assertEqual(result.crop((0, 0, 192, 80)).tobytes(), neutral.crop((0, 0, 192, 80)).tobytes())
        self.assertEqual(result.crop((135, 0, 192, 208)).tobytes(), neutral.crop((135, 0, 192, 208)).tobytes())
        self.assertEqual(result.getpixel((10, 180)), neutral.getpixel((10, 180)))
        self.assertNotEqual(result.getpixel((105, 150)), neutral.getpixel((105, 150)))


if __name__ == "__main__":
    unittest.main()
