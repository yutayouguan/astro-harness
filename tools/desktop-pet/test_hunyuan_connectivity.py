import unittest
import numpy as np
from hunyuan_connectivity import inspect, component_counts


class ConnectivityTests(unittest.TestCase):
    def test_uv_split_is_not_a_separate_body_part(self):
        vertices = np.array([[0.,0,0], [1,0,0], [1,0,0], [2,0,0]])
        edges = np.array([[0,1], [2,3]])
        before = vertices.copy()
        result = inspect(vertices, edges)
        self.assertEqual(result['raw_components'], 2)
        self.assertEqual(result['components_after_virtual_exact_weld'], 1)
        np.testing.assert_array_equal(before, vertices)

    def test_nearby_vertices_are_not_welded(self):
        vertices = np.array([[0.,0,0], [1,0,0], [1.00001,0,0], [2,0,0]])
        self.assertEqual(inspect(vertices, np.array([[0,1],[2,3]]))['components_after_virtual_exact_weld'], 2)

    def test_isolated_vertices(self):
        self.assertEqual(component_counts(3, np.empty((0,2), dtype=int)), [1,1,1])

    def test_invalid_edges(self):
        with self.assertRaises(ValueError):
            component_counts(2, np.array([[0,2]]))

    def test_chained_connectivity(self):
        edges = np.column_stack((np.arange(9999), np.arange(1,10000)))
        self.assertEqual(component_counts(10000, edges), [10000])


if __name__ == '__main__':
    unittest.main()
