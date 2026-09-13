import unittest
from walk_pose_guide import foot, LEGS, STRIDE

class WalkGuideTests(unittest.TestCase):
    def test_ground_contacts_stay_fixed_in_world_coordinates(self):
        for _,x,_,offset,_,_ in LEGS:
            contacts={}
            for i in range(240):
                phase=i/240;fx,fy,planted=foot(phase,offset,x)
                if planted:
                    self.assertEqual(fy,202)
                    cycle=int(phase+offset)
                    world=fx+STRIDE*phase
                    if cycle in contacts:self.assertAlmostEqual(world,contacts[cycle])
                    contacts[cycle]=world
    def test_slow_walk_never_has_an_airborne_phase(self):
        for i in range(240):
            self.assertGreaterEqual(sum(foot(i/240,offset,x)[2] for _,x,_,offset,_,_ in LEGS),3)

if __name__=="__main__":unittest.main()
