"""Read-only mesh connectivity diagnostics; virtual welding never edits input."""
import numpy as np


def component_counts(vertex_count, edges):
    if vertex_count < 0:
        raise ValueError('Invalid vertex count')
    edges = np.asarray(edges)
    if edges.ndim != 2 or edges.shape[1] != 2 or not np.issubdtype(edges.dtype, np.integer):
        raise ValueError('Expected integer edge pairs')
    if edges.size and (edges.min() < 0 or edges.max() >= vertex_count):
        raise ValueError('Edge references a missing vertex')
    parents = np.arange(vertex_count, dtype=np.int64)
    for _ in range(100):
        roots = parents[edges]
        low, high = roots.min(axis=1), roots.max(axis=1)
        if np.all(low == high):
            return sorted(np.unique(parents, return_counts=True)[1].tolist(), reverse=True)
        np.minimum.at(parents, high, low)
        while np.any(parents != parents[parents]):
            parents = parents[parents]
    raise RuntimeError('Connectivity did not converge')


def inspect(vertices, edges):
    vertices = np.asarray(vertices)
    edges = np.asarray(edges)
    if vertices.ndim != 2 or vertices.shape[1] != 3 or not np.all(np.isfinite(vertices)):
        raise ValueError('Expected finite vertex coordinates')
    raw = component_counts(len(vertices), edges)
    unique, inverse = np.unique(vertices, axis=0, return_inverse=True)
    welded = component_counts(len(unique), inverse[edges])
    return {'original_vertices': len(vertices), 'raw_components': len(raw),
            'exact_unique_positions': len(unique),
            'components_after_virtual_exact_weld': len(welded),
            'largest_welded_vertex_counts': welded[:8], 'input_mutated': False}
