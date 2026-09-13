import tempfile
import unittest
from pathlib import Path
from PIL import Image
from paint_blink_sample import composite, eye_mask, prepare, origin, GRID, SIZE


class PaintedBlinkTests(unittest.TestCase):
    def test_painter_cannot_change_body_or_alpha(self):
        neutral = Image.new("RGBA", SIZE, (120, 80, 30, 170))
        result = composite(neutral, Image.new("RGBA", SIZE, (0, 255, 255, 0)))
        self.assertEqual(result.getchannel("A").tobytes(), neutral.getchannel("A").tobytes())
        mask = eye_mask()
        for y in range(SIZE[1]):
            for x in range(SIZE[0]):
                if mask.getpixel((x, y)) == 0:
                    self.assertEqual(result.getpixel((x, y)), neutral.getpixel((x, y)))
        self.assertNotEqual(result.getpixel((92, 69)), neutral.getpixel((92, 69)))

    def test_rejects_unregistered_size(self):
        with self.assertRaises(ValueError):
            composite(Image.new("RGBA", SIZE), Image.new("RGBA", (384, 416)))

    def test_grid_mask_keeps_first_pose_and_body_protected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "source.png"
            Image.new("RGBA", SIZE, (120, 80, 30, 255)).save(source)
            prepare(source, root / "study")
            with Image.open(root / "study/edit-mask.png") as mask:
                self.assertEqual(mask.size, GRID)
                first_x, first_y = origin(0)
                x, y = origin(1)
                self.assertEqual(mask.getpixel((first_x+184, first_y+138))[3], 255)
                self.assertEqual(mask.getpixel((x+184, y+138))[3], 0)
                self.assertEqual(mask.getpixel((x+184, y+350))[3], 255)
            with self.assertRaises(FileExistsError):
                prepare(source, root / "study")


if __name__ == "__main__":
    unittest.main()
