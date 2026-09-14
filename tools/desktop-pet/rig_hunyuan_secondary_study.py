"""Extend fresh independent copies of the head study with ear/tail controls.

Run in Blender with SECONDARY_OUTPUT. Requires the original coat study scenes.
"""
import json
import math
from pathlib import Path
import runpy

import bpy
import numpy as np
from mathutils import Quaternion, Vector

ROOT = Path(__file__).resolve().parent
OUT = Path(SECONDARY_OUTPUT).resolve()
HEAD = runpy.run_path(str(ROOT/'rig_hunyuan_head_study.py'), init_globals={'RIG_OUTPUT': str(OUT)})
MASKS = runpy.run_path(str(ROOT/'hunyuan_secondary_weights.py'))
studies = HEAD['studies']

LANDMARKS = {
    'naitang': {
        'tail.base': ((-.25, .08, -.08), (-.24, .08, .04), 'body_anchor'),
        'tail.tip': ((-.24, .08, .04), (-.15, .065, .16), 'tail.base'),
        'ear.L': ((-.13, .725, .20), (-.17, .80, .23), 'head'),
        'ear.R': ((.15, .75, .19), (.18, .84, .18), 'head'),
    },
    'pudding': {
        'tail.base': ((-.255, .07, -.36), (-.32, .16, -.30), 'body_anchor'),
        'tail.tip': ((-.32, .16, -.30), (-.30, .27, -.24), 'tail.base'),
        'ear.L': ((-.15, .72, .08), (-.20, .51, .10), 'head'),
        'ear.R': ((.20, .75, .08), (.23, .54, .12), 'head'),
    },
}


def create(pet):
    HEAD['create'](pet)
    state = studies[pet]
    scene, rig, mesh = (state[k] for k in ('scene', 'rig', 'mesh'))
    scene.name = f'HY_Secondary_{pet}_{HEAD["RUN"]}'
    scene['study_scope'] = 'small head glance, ear response and tail tip; not production ready'
    scene.view_layers[0].objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode='EDIT')
    for name, (start, end, parent) in LANDMARKS[pet].items():
        bone = rig.data.edit_bones.new(name)
        bone.head, bone.tail = start, end
        bone.parent = rig.data.edit_bones[parent]
    bpy.ops.object.mode_set(mode='OBJECT')
    weights = update_weights(pet)
    animate_secondary(pet)
    scene.frame_set(1)
    print(json.dumps({k: int(np.count_nonzero(v)) for k, v in weights.items()}))


def animate_secondary(pet):
    state = studies[pet]
    rig = state['rig']
    # Ear response precedes the head glance; puppy's soft ears settle later.
    for name in ('ear.L', 'ear.R'):
        sign = 1 if name == 'ear.L' else -1
        delay = 0 if name == 'ear.L' else 3
        amplitude = 2 if pet == 'naitang' else 3
        keys = [(1, 0), (9+delay, 0), (15+delay, amplitude*sign),
                (21+delay, -.3*amplitude*sign), (29+delay, 0), (73, 0)]
        animate(rig, name, (0, 0, 1), keys)
    for name in ('tail.base', 'tail.tip'):
        amplitude = (1.2 if pet == 'naitang' else 6) * (1 if name == 'tail.base' else .6)
        delay = 0 if name == 'tail.base' else 2
        keys = [(1, 0), (27+delay, 0), (35+delay, amplitude),
                (44+delay, -amplitude*.65), (53+delay, amplitude*.35),
                (65, 0), (73, 0)]
        animate(rig, name, (0, 1, 0), keys)
    for layer in rig.animation_data.action.layers:
        for strip in layer.strips:
            for bag in strip.channelbags:
                for curve in bag.fcurves:
                    for key in curve.keyframe_points:
                        key.interpolation = 'BEZIER'
                        key.handle_left_type = key.handle_right_type = 'AUTO_CLAMPED'
    state['scene'].frame_set(1)


def update_weights(pet):
    state = studies[pet]
    mesh = state['mesh']
    weights = MASKS['partition'](pet, state['coords'])
    mesh.vertex_groups.clear()
    for name, bins in weights.items():
        group = mesh.vertex_groups.new(name=name)
        order = np.argsort(bins)
        boundaries = np.flatnonzero(np.diff(bins[order]))+1
        for indices in np.split(order, boundaries):
            value = bins[indices[0]]
            if value:
                group.add(indices.tolist(), int(value)/MASKS['WEIGHT_SCALE'], 'REPLACE')
    state['weights'] = weights
    return weights


def animate(rig, name, axis, keys):
    pose = rig.pose.bones[name]
    pose.rotation_mode = 'QUATERNION'
    orientation = pose.bone.matrix_local.to_quaternion()
    for frame, angle in keys:
        pose.rotation_quaternion = orientation.inverted() @ Quaternion(axis, math.radians(angle)) @ orientation
        pose.keyframe_insert(data_path='rotation_quaternion', frame=frame)


def render_weights(pet, view='front'):
    state = studies[pet]
    scene, mesh = state['scene'], state['mesh']
    bpy.context.window.scene = scene
    scene.frame_set(1)
    material = bpy.data.materials.new(f'Weights_{pet}_{view}_{HEAD["RUN"]}')
    material.use_nodes = True
    nodes = material.node_tree.nodes
    nodes.clear()
    output = nodes.new('ShaderNodeOutputMaterial')
    emit = nodes.new('ShaderNodeEmission')
    color = nodes.new('ShaderNodeVertexColor')
    color.layer_name = 'SecondaryWeights'
    material.node_tree.links.new(color.outputs['Color'], emit.inputs['Color'])
    material.node_tree.links.new(emit.outputs[0], output.inputs['Surface'])
    attribute = mesh.data.color_attributes.get(color.layer_name)
    if attribute is None:
        attribute = mesh.data.color_attributes.new(name=color.layer_name, type='FLOAT_COLOR', domain='POINT')
    colors = np.full((len(mesh.data.vertices), 4), .06, dtype=np.float32)
    colors[:, 3] = 1
    for channel, names in enumerate((('tail.base','tail.tip'), ('ear.L',), ('ear.R',))):
        colors[:, channel] += .94*sum(state['weights'][n] for n in names)/MASKS['WEIGHT_SCALE']
    attribute.data.foreach_set('color', colors.ravel())
    originals = [slot.material for slot in mesh.material_slots]
    camera_matrix = scene.camera.matrix_world.copy()
    old_view = scene.view_settings.view_transform
    try:
        for slot in mesh.material_slots:
            slot.material = material
        scene.view_settings.view_transform = 'Standard'
        if view == 'back':
            scene.camera.location.y *= -1
            center = Vector((-.045, .08, .43))
            scene.camera.rotation_euler = (center-scene.camera.location).to_track_quat('-Z', 'Y').to_euler()
        scene.render.filepath = str(OUT/f'{pet}-weights-{view}.png')
        bpy.ops.render.render(write_still=True, scene=scene.name)
    finally:
        for slot, material in zip(mesh.material_slots, originals):
            slot.material = material
        scene.camera.matrix_world = camera_matrix
        scene.view_settings.view_transform = old_view


def render(pet, frame):
    HEAD['render'](pet, frame)


def evaluated_points(scene, mesh, frame):
    bpy.context.window.scene = scene
    scene.frame_set(frame)
    evaluated = mesh.evaluated_get(bpy.context.evaluated_depsgraph_get())
    data = evaluated.to_mesh()
    try:
        result = np.empty(len(data.vertices)*3, dtype=np.float32)
        data.vertices.foreach_get('co', result)
        return result.reshape(-1, 3)
    finally:
        evaluated.to_mesh_clear()


def validate(pet):
    state = studies[pet]
    mesh, scene, coords = state['mesh'], state['scene'], state['coords']
    rig = state['rig']
    assert max(abs(rig.matrix_world[r][c]-mesh.matrix_world[r][c])
               for r in range(4) for c in range(4)) < 1e-6, 'Rig/mesh coordinate mismatch'
    x, y, z = coords.T
    paws = (y < .12) & (x > -.11) & (z > -.1)
    face = (y > .60) & (y < .72) & (x > -.10) & (x < .15) & (z > .22)
    haunch = (y > .16) & (y < .34) & (abs(x) < .20) & (z < -.2)
    assert all(np.count_nonzero(m) > 100 for m in (paws, face, haunch))
    edges = np.empty(len(mesh.data.edges)*2, dtype=np.int32)
    mesh.data.edges.foreach_get('vertices', edges)
    edges = edges.reshape(-1, 2)
    rest_lengths = np.linalg.norm(coords[edges[:, 0]]-coords[edges[:, 1]], axis=1)
    valid_edges = rest_lengths > 1e-5
    maxima = dict(paw_drift=0., additional_face_drift=0., additional_haunch_drift=0., max_edge_strain=0.)
    try:
        for frame in range(1, 74):
            points = evaluated_points(scene, mesh, frame)
            delta = np.linalg.norm(points-coords, axis=1)
            bone = rig.pose.bones['head']
            matrix = np.asarray(bone.matrix @ bone.bone.matrix_local.inverted())
            expected_face = coords[face] @ matrix[:3, :3].T + matrix[:3, 3]
            maxima['paw_drift'] = max(maxima['paw_drift'], float(delta[paws].max()))
            maxima['additional_face_drift'] = max(maxima['additional_face_drift'], float(np.linalg.norm(points[face]-expected_face, axis=1).max()))
            maxima['additional_haunch_drift'] = max(maxima['additional_haunch_drift'], float(delta[haunch].max()))
            assert maxima['paw_drift'] < 1e-6
            assert maxima['additional_face_drift'] < 1e-6
            assert maxima['additional_haunch_drift'] < 1e-6
            assert float(points[:, 1].min()) > -1e-6, 'Below ground'
            if frame in (1, 65, 73):
                assert float(delta.max()) < 1e-6, 'Did not return to bind pose'
            lengths = np.linalg.norm(points[edges[:, 0]]-points[edges[:, 1]], axis=1)
            strain = float(np.max(abs(lengths[valid_edges]/rest_lengths[valid_edges]-1)))
            maxima['max_edge_strain'] = max(maxima['max_edge_strain'], strain)
            assert strain < .3, 'Excessive edge stretching'
    finally:
        scene.frame_set(1)
        bpy.context.window.scene = scene
    assert HEAD['geometry_digest'](state['original'].data) == state['before']
    assert HEAD['geometry_digest'](mesh.data) == state['before']
    assert not state['original'].modifiers and not state['original'].vertex_groups
    report = dict(pet=pet, validated_frames=73, source_and_bind_geometry_unchanged=True,
                  production_ready=False, **maxima)
    (OUT/f'{pet}-secondary-validation.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report))


def save():
    for state in studies.values():
        state['scene'].frame_set(1)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'hunyuan-secondary-study.blend'), copy=True, compress=True)
