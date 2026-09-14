import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw
from package_hunyuan_head_study import load_frames, package


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        for pet in ('naitang', 'pudding'):
            for index in range(1, 73):
                frame = Image.new('RGBA', (24, 24))
                offset = 3 if 20 <= index <= 45 else 0
                ImageDraw.Draw(frame).ellipse((5+offset, 5, 12+offset, 15), fill=(220, 130, 50, 255))
                frame.save(self.folder / f'{pet}-{index:03d}.png')

    def test_roundtrip_and_duration(self):
        result = package(self.folder)
        self.assertEqual(len(result), 2)
        self.assertTrue(all(row['duration_ms'] == 3000 for row in result))
        self.assertTrue(all(row['encoded_frames'] < 72 for row in result))
        with self.assertRaises(FileExistsError):
            package(self.folder)

    def test_missing_frame_prevents_any_output(self):
        (self.folder / 'pudding-055.png').unlink()
        with self.assertRaises(FileNotFoundError):
            package(self.folder)
        self.assertFalse((self.folder / 'naitang-head-study.apng').exists())

    def test_opaque_background_rejected(self):
        Image.new('RGBA', (24, 24), 'white').save(self.folder / 'naitang-010.png')
        with self.assertRaisesRegex(ValueError, 'clipped'):
            load_frames(self.folder, 'naitang')

    def test_loop_jump_rejected(self):
        with Image.open(self.folder / 'naitang-029.png') as frame:
            frame.save(self.folder / 'naitang-072.png')
        with self.assertRaisesRegex(ValueError, 'endpoint'):
            load_frames(self.folder, 'naitang')

    def test_broad_one_level_noise_rejected(self):
        path = self.folder / 'naitang-072.png'
        with Image.open(path) as source:
            frame = source.copy()
        frame.putpixel((7, 9), (221, 130, 50, 255))
        frame.save(path)
        # One changed pixel / 576 exceeds the real-render mean threshold.
        with self.assertRaisesRegex(ValueError, 'endpoint'):
            load_frames(self.folder, 'naitang')

    def test_sparse_one_level_noise_normalized(self):
        for path in self.folder.glob('naitang-*.png'):
            with Image.open(path) as source:
                frame = source.resize((48, 48), Image.Resampling.NEAREST)
            frame.save(path)
        path = self.folder / 'naitang-072.png'
        with Image.open(path) as source:
            frame = source.copy()
        frame.putpixel((14, 18), (221, 130, 50, 255))
        frame.save(path)
        frames = load_frames(self.folder, 'naitang')
        self.assertEqual(frames[0].tobytes(), frames[-1].tobytes())


if __name__ == '__main__':
    unittest.main()
