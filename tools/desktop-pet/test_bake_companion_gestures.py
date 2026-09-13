import unittest
from PIL import Image
from bake_companion_gestures import blink, breath, head_pose

class GesturesTests(unittest.TestCase):
    def test_blink_keeps_the_whole_silhouette_and_non_eye_pixels(self):
        neutral=Image.new("RGBA",(192,208),(200,120,50,128));closed=Image.new("RGBA",(192,208),(100,70,30,255))
        for pet in ("naitang","pudding"):
            result=blink(neutral,closed,.7,pet)
            self.assertEqual(result.getchannel("A").tobytes(),neutral.getchannel("A").tobytes())
            self.assertEqual(result.crop((0,100,192,208)).tobytes(),neutral.crop((0,100,192,208)).tobytes())
    def test_breath_does_not_scale_head_paws_or_silhouette(self):
        frame=Image.new("RGBA",(192,208),(200,120,50,200))
        for pet in ("naitang","pudding"):
            result=breath(frame,.02,pet)
            self.assertEqual(result.getchannel("A").tobytes(),frame.getchannel("A").tobytes())
            self.assertEqual(result.crop((0,175,192,208)).tobytes(),frame.crop((0,175,192,208)).tobytes())
            self.assertEqual(head_pose(frame,0,0,pet).tobytes(),frame.tobytes())

if __name__=="__main__":unittest.main()
