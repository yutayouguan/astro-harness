import tempfile
import unittest
from pathlib import Path
from PIL import Image
from audit_motion import analyze, effective_frames, frame_metrics, motion_sequence


class MotionAuditTests(unittest.TestCase):
    def test_runtime_bookends_replace_generated_lookalikes(self):
        neutral = Image.new("RGBA", (192, 208), (0, 0, 0, 0))
        neutral.putpixel((50, 200), (200, 100, 40, 255))
        atlas = Image.new("RGBA", (384, 208), (255, 0, 0, 255))
        spec = dict(frameWidth=192, frameHeight=208, columns=2,
                    durationsMs=[100, 100], neutralBookends=True)
        frames = effective_frames(atlas, spec, neutral)
        self.assertEqual(frames[0].tobytes(), neutral.tobytes())
        self.assertEqual(frames[-1].tobytes(), neutral.tobytes())
        self.assertEqual(frame_metrics(frames[0])["baseline"], 201)

    def test_dimensions_and_empty_frames_fail_closed(self):
        empty = Image.new("RGBA", (192, 208))
        with self.assertRaises(ValueError):
            frame_metrics(empty)
        with self.assertRaises(ValueError):
            effective_frames(empty, dict(frameWidth=192, frameHeight=208,
                             columns=2, durationsMs=[100, 100]), empty)
        with self.assertRaises(ValueError):
            effective_frames(empty, dict(frameWidth=192, frameHeight=208,
                             columns=1, durationsMs=[]), empty)

    def test_sequence_contains_actual_loop_seam(self):
        spec = dict(durationsMs=[100] * 6, loopStart=2, loopEnd=4, loopRepeats=3)
        self.assertEqual(motion_sequence(spec), [0, 1, 2, 3, 2, 3, 2, 3, 4, 5])
        for invalid in (dict(loopStart=-1), dict(loopEnd=7), dict(loopRepeats=0),
                        dict(durationsMs=[0] * 6)):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                motion_sequence({**spec, **invalid})

    def test_metrics_exclude_faint_alpha_fringe(self):
        frame = Image.new("RGBA", (192, 208))
        frame.putpixel((10, 200), (200, 100, 40, 255))
        frame.putpixel((11, 207), (200, 100, 40, 32))
        metrics = frame_metrics(frame)
        self.assertEqual(metrics["baseline"], 201)
        self.assertEqual(metrics["silhouetteArea"], 1)

    def test_diagnostics_cannot_overwrite_source_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                analyze(root, root)
            with self.assertRaises(ValueError):
                analyze(root, root / "nested")


if __name__ == "__main__":
    unittest.main()
