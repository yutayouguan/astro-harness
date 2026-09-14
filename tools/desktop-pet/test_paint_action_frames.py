import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

from export_apng import read_apng_frames, write_apng
from paint_action_frames import COUNT, GRID, SIZE, clean_ground_residue, composite, export_previews, gif_delays, mask_for, neutral_from, origin, registered_cell


class PaintedActionTests(unittest.TestCase):
    def test_only_local_mask_can_change_including_translucent_pixels(self):
        for action in ("kneading", "grooming"):
            neutral = Image.new("RGBA", SIZE, (191, 104, 41, 71))
            painted = Image.new("RGBA", SIZE, (11, 221, 137, 142))
            result = composite(neutral, painted, action)
            outside = mask_for(action).point(lambda v: 255 if v == 0 else 0)
            for channel in ImageChops.difference(neutral, result).split():
                self.assertIsNone(ImageChops.multiply(channel, outside).getbbox())
            self.assertNotEqual(result.tobytes(), neutral.tobytes())

    def test_alpha_feather_does_not_add_black_hair(self):
        neutral = Image.new("RGBA", SIZE, (255, 255, 255, 255))
        result = composite(neutral, Image.new("RGBA", SIZE), "kneading")
        for red, green, blue, alpha in result.get_flattened_data():
            if alpha:
                self.assertEqual((red, green, blue), (255, 255, 255))

    def test_unregistered_character_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "other.png"
            Image.new("RGBA", SIZE, "orange").save(path)
            with self.assertRaisesRegex(ValueError, "fingerprint"):
                neutral_from(path)

    def test_invalid_action_and_cell_sizes_rejected(self):
        with self.assertRaises(ValueError):
            mask_for("tail-wag")
        with self.assertRaises(ValueError):
            composite(Image.new("RGBA", SIZE), Image.new("RGBA", (1, 1)), "kneading")

    def test_all_cells_fit_the_registered_grid_without_overlap(self):
        positions = [origin(i) for i in range(COUNT)]
        self.assertEqual(len(set(positions)), COUNT)
        for x, y in positions:
            self.assertLessEqual(x+384, GRID[0])
            self.assertLessEqual(y+416, GRID[1])
        self.assertEqual(origin(4), (0, 560))

    def test_residue_cleanup_preserves_fur_opaque_details_and_upper_body(self):
        cell = Image.new("RGBA", SIZE)
        cell.putpixel((100, 205), (0, 21, 0, 12))
        cell.putpixel((101, 205), (244, 201, 161, 12))
        cell.putpixel((102, 205), (0, 21, 0, 255))
        cell.putpixel((100, 100), (0, 21, 0, 12))
        result = clean_ground_residue(cell)
        self.assertEqual(result.getpixel((100, 205)), (0, 0, 0, 0))
        for coordinate in ((101, 205), (102, 205), (100, 100)):
            self.assertEqual(result.getpixel(coordinate), cell.getpixel(coordinate))

    def test_head_registration_ignores_paw_motion(self):
        neutral = Image.new("RGBA", SIZE)
        # Distinct head texture, not a uniform color with ambiguous alignment.
        for y in range(10, 80):
            for x in range(50, 140):
                neutral.putpixel((x, y), ((x*7+y*3)%256, (x+y*9)%256, (x*3+y*5)%256, 255))
        sheet = Image.new("RGBA", (1536, 1024))
        shifted = neutral.resize((384, 416), Image.Resampling.LANCZOS)
        sheet.paste(shifted, (4, 48-4))
        _, metrics = registered_cell(sheet, 0, neutral)
        self.assertEqual((metrics["dx"], metrics["dy"]), (2, -2))

    def test_preview_uses_encoded_frame_order_after_coalescing(self):
        red = Image.new("RGBA", (256, 208))
        blue = red.copy()
        ImageDraw.Draw(red).rectangle((40, 40, 70, 70), fill="red")
        ImageDraw.Draw(blue).rectangle((40, 40, 70, 70), fill="blue")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frames = [red, red, blue, red]
            spec = write_apng(root / "test.apng", frames, [100, 80, 90, 200])
            with self.assertRaisesRegex(ValueError, "encoded APNG"):
                export_previews(frames, spec, root / "invalid", "test")
            self.assertFalse((root / "invalid").exists())
            decoded = read_apng_frames(root / "test.apng", spec)
            export_previews(decoded, spec, root, "test")
            with Image.open(root / "test-preview.gif") as gif:
                self.assertEqual(gif.n_frames, 3)
                colors, times = [], []
                for i in range(3):
                    gif.seek(i)
                    colors.append(gif.convert("RGB").getpixel((100, 100)))
                    times.append(gif.info["duration"])
                self.assertEqual(colors, [(255, 0, 0), (0, 0, 255), (255, 0, 0)])
                self.assertEqual(times, [180, 90, 200])

    def test_gif_tick_rounding_does_not_accumulate_playback_speed_error(self):
        durations = [42] * 100
        for speed in (1, 3):
            rounded = gif_delays(durations, speed)
            self.assertEqual(sum(rounded), sum(durations)*speed)
            for count in range(1, len(durations)+1):
                self.assertLessEqual(abs(sum(rounded[:count])-sum(durations[:count])*speed), 5)
        self.assertEqual(gif_delays([80, 90, 320]), [80, 90, 320])


if __name__ == "__main__":
    unittest.main()
