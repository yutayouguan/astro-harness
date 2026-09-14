import unittest
import numpy as np
from hunyuan_secondary_weights import partition, regions, WEIGHT_SCALE


class WeightTests(unittest.TestCase):
    def test_partition_stays_normalized_over_model_volume(self):
        points = np.random.default_rng(109).uniform([-.4, 0, -.55], [.3, .9, .4], (100000, 3))
        for pet in ('naitang', 'pudding'):
            weights = partition(pet, points)
            self.assertTrue(np.all(sum(weights.values()) == WEIGHT_SCALE))
            self.assertTrue(all(np.all((w >= 0) & (w <= WEIGHT_SCALE)) for w in weights.values()))

    def test_core_face_paws_and_haunch_have_no_secondary_weights(self):
        points = np.array([[0, .65, .30], [.09, .70, .29],
                           [0, .05, .12], [.13, .05, .12], [0, .25, -.3]])
        for pet in ('naitang', 'pudding'):
            weights = partition(pet, points)
            for bone in ('ear.L', 'ear.R', 'tail.base', 'tail.tip'):
                self.assertTrue(np.all(weights[bone] == 0), (pet, bone))

    def test_naitang_tail_does_not_reach_front_leg(self):
        masks = regions('naitang', np.array([[-.18, .135, .08], [-.24, .06, .12]]))
        self.assertEqual(masks['tail'][0], 0)
        self.assertGreater(masks['tail'][1], .9)

    def test_cat_ear_does_not_reach_brow(self):
        weights = partition('naitang', np.array([[-.097245, .719797, .252916]]))
        self.assertEqual(weights['ear.L'][0], 0)

    def test_puppy_tail_tip_is_not_haunch(self):
        weights = partition('pudding', np.array([[-.32, .23, -.27], [-.20, .23, -.27]]))
        self.assertEqual(weights['tail.tip'][0], WEIGHT_SCALE)
        self.assertEqual(weights['tail.tip'][1], 0)

    def test_unknown_asset_rejected(self):
        with self.assertRaises(ValueError):
            partition('unknown', np.zeros((1, 3)))


if __name__ == '__main__':
    unittest.main()
