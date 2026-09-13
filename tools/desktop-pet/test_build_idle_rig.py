import json
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from build_idle_rig import build, opaque_fingerprint

ROOT = Path(__file__).resolve().parents[2]


class IdleRigTests(unittest.TestCase):
    def test_shipped_layers_are_reproducible_and_keep_the_original_paws(self):
        for pet in ("naitang", "pudding"):
            source = ROOT / "apps/desktop/src/assets/pets" / pet
            with tempfile.TemporaryDirectory() as folder:
                out = Path(folder)
                config = build(source, out)
                self.assertEqual(config, json.loads((source / "idle-rig/rig.json").read_text()))
                with Image.open(out / "neutral.png") as neutral, Image.open(out / "body.png") as body:
                    self.assertEqual(opaque_fingerprint(neutral), config["fingerprint"])
                    self.assertEqual(neutral.crop((86, 185, 160, 208)).tobytes(), body.crop((86, 185, 160, 208)).tobytes())
                    self.assertEqual(neutral.getpixel((0, 0))[3], 0)
                for name in ("neutral", "body", "heads", "tail"):
                    self.assertEqual((out / f"{name}.png").read_bytes(), (source / "idle-rig" / f"{name}.png").read_bytes())

    def test_fingerprint_is_sensitive_to_opaque_character_changes(self):
        image = Image.new("RGBA", (192, 208), (200, 50, 30, 255))
        before = opaque_fingerprint(image)
        image.putpixel((90, 50), (201, 50, 30, 255))
        self.assertNotEqual(before, opaque_fingerprint(image))


if __name__ == "__main__":
    unittest.main()
