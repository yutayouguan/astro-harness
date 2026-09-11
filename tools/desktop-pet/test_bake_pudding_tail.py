"""Regression for the real bundled puppy: no moving body hidden by a tail bbox."""
import unittest
from collections import deque
from pathlib import Path
from PIL import Image
from bake_pudding_tail import CELL, render

ROOT = Path(__file__).resolve().parents[2]


def large_components(image):
    alpha = bytearray(a > 32 for a in image.getchannel("A").getdata())
    width, height = image.size
    sizes = []
    for index in range(len(alpha)):
        if not alpha[index]: continue
        alpha[index] = 0
        queue, count = deque([index]), 0
        while queue:
            i = queue.popleft(); x = i % width; count += 1
            for j in (i-1 if x else -1, i+1 if x+1 < width else -1, i-width, i+width):
                if 0 <= j < len(alpha) and alpha[j]:
                    alpha[j] = 0; queue.append(j)
        if count > 30: sizes.append(count)
    return sizes


class FixedPuppyTailTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.neutral = Image.open(ROOT / "apps/desktop/src/assets/pets/pudding/spritesheet.webp").convert("RGBA").crop((0,0,*CELL))
        cls.frames, cls.protected, _ = render(cls.neutral)

    def test_every_protected_pixel_and_every_paw_stays_identical(self):
        empty = Image.new("RGBA", CELL)
        expected = Image.composite(self.neutral, empty, self.protected).tobytes()
        for i, frame in enumerate(self.frames):
            self.assertEqual(Image.composite(frame, empty, self.protected).tobytes(), expected, i)
            self.assertEqual(frame.crop((58,184,180,208)).tobytes(), self.neutral.crop((58,184,180,208)).tobytes(), i)

    def test_tail_moves_without_detaching_or_touching_frame_edges(self):
        self.assertGreater(len({frame.tobytes() for frame in self.frames}), 30)
        for i, frame in enumerate(self.frames):
            self.assertEqual(len(large_components(frame)), 1, i)
            a = frame.getchannel("A")
            self.assertIsNone(a.crop((0,0,192,1)).getbbox(), i)
            self.assertIsNone(a.crop((0,207,192,208)).getbbox(), i)
            self.assertIsNone(a.crop((0,0,1,208)).getbbox(), i)
            self.assertIsNone(a.crop((191,0,192,208)).getbbox(), i)

    def test_neutral_bookends_are_exact_not_lookalikes(self):
        self.assertEqual(self.frames[0].tobytes(), self.neutral.tobytes())
        self.assertEqual(self.frames[-1].tobytes(), self.neutral.tobytes())

    def test_shipped_webp_preserves_every_body_pixel_after_encoding(self):
        sheet = Image.open(ROOT / "apps/desktop/src/assets/pets/pudding/tail-wag.webp").convert("RGBA")
        empty = Image.new("RGBA", CELL)
        expected = Image.composite(self.neutral, empty, self.protected).tobytes()
        for i in range(49):
            frame = sheet.crop((i%4*192,i//4*208,(i%4+1)*192,(i//4+1)*208))
            self.assertEqual(Image.composite(frame, empty, self.protected).tobytes(),expected,i)


if __name__ == "__main__": unittest.main()
