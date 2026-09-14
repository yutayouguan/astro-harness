"""Conservative head-motion study, NOT a production locomotion/facial rig.

Execute inside Blender with RIG_OUTPUT pointing at an existing empty run folder.
Append the two coat-study scenes first. Mesh coordinates remain glTF Y-up so
the existing generated-coordinate coat material keeps its original mapping.
"""
from array import array
import hashlib
import json
import math
from pathlib import Path

import bpy
import numpy as np
from mathutils import Vector

OUT = Path(RIG_OUTPUT).resolve()
RUN = OUT.name.rsplit('-', 1)[-1]
SOURCES = {p: f'HY_Coat_{p}_GQPpJm' for p in ('naitang', 'pudding')}
studies = {}


def geometry_digest(mesh):
    coordinates = array('f', [0]) * (len(mesh.vertices) * 3)
    mesh.vertices.foreach_get('co', coordinates)
    return hashlib.sha256(coordinates.tobytes()).hexdigest()


def create(pet):
    source = bpy.data.scenes[SOURCES[pet]]
    name = f'HY_HeadStudy_{pet}_{RUN}'
    if name in bpy.data.scenes:
        raise RuntimeError('Refusing to overwrite existing study')
    original = next(o for o in source.objects if o.type == 'MESH')
    if original.vertex_groups or original.modifiers or original.data.shape_keys:
        raise RuntimeError('Expected untouched unrigged input')
    before = geometry_digest(original.data)
    scene = bpy.data.scenes.new(name)
    scene['source_scene'] = source.name
    scene['source_geometry_sha256'] = before
    scene['production_ready'] = False
    scene['study_scope'] = 'head glance only; no tail, facial or locomotion rig'
    bpy.context.window.scene = scene
    scene.world = source.world.copy()
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 12
    scene.cycles.use_denoising = True
    scene.cycles.seed = 0
    scene.render.resolution_x = scene.render.resolution_y = 384
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGBA'
    scene.render.fps = 24
    scene.view_settings.view_transform = source.view_settings.view_transform
    scene.view_settings.look = source.view_settings.look
    scene.view_settings.exposure = source.view_settings.exposure
    scene.view_settings.gamma = source.view_settings.gamma
    for obj in source.objects:
        copy = obj.copy()
        copy.data = obj.data.copy()
        copy.name = f'HYHead_{pet}_{RUN}_{obj.type}'
        scene.collection.objects.link(copy)
        if obj == source.camera:
            scene.camera = copy
        if obj == original:
            mesh = copy
            # Isolate material graphs as well; packed image pixels stay read-only.
            for slot in mesh.material_slots:
                material = slot.material.copy()
                slot.link = 'OBJECT'
                slot.material = material
    assert mesh.data != original.data
    # Newly linked copies have a stale matrix_world until dependency evaluation.
    # The GLB uses local Y-up; copying its stale identity rotates the rig in the
    # wrong space and can move a tail/face around an unrelated pivot.
    scene.view_layers[0].update()
    armature = bpy.data.armatures.new(name + '_Bones')
    rig = bpy.data.objects.new(name + '_Rig', armature)
    scene.collection.objects.link(rig)
    rig.matrix_world = mesh.matrix_world.copy()
    scene.view_layers[0].update()
    assert max(abs(rig.matrix_world[r][c]-mesh.matrix_world[r][c])
               for r in range(4) for c in range(4)) < 1e-6
    scene.view_layers[0].objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode='EDIT')
    root = armature.edit_bones.new('body_anchor')
    root.head = (0, .05, 0)
    root.tail = (0, .38, 0)
    head = armature.edit_bones.new('head')
    head.head = (0, .49, .10)
    head.tail = (0, .73, .10)
    head.parent = root
    bpy.ops.object.mode_set(mode='OBJECT')
    rig.show_in_front = True
    coords = np.empty(len(mesh.data.vertices) * 3, dtype=np.float32)
    mesh.data.vertices.foreach_get('co', coords)
    coords = coords.reshape(-1, 3)
    # Broad C2-continuous neck blend, rigid face and stationary lower body.
    t = np.clip((coords[:, 1] - .38) / .20, 0, 1)
    weights = t**3 * (10 - 15*t + 6*t*t)
    bins = np.rint(weights * 65536).astype(np.int32)
    head_group = mesh.vertex_groups.new(name='head')
    body_group = mesh.vertex_groups.new(name='body_anchor')
    order = np.argsort(bins)
    boundaries = np.flatnonzero(np.diff(bins[order]))+1
    for group_indices in np.split(order, boundaries):
        value = bins[group_indices[0]]
        indices = group_indices.tolist()
        w = int(value) / 65536
        if w:
            head_group.add(indices, w, 'REPLACE')
        if w < 1:
            body_group.add(indices, 1-w, 'REPLACE')
    modifier = mesh.modifiers.new('Head deformation study', 'ARMATURE')
    modifier.object = rig
    modifier.use_deform_preserve_volume = True
    points = [mesh.matrix_world @ Vector(p) for p in mesh.bound_box]
    low = Vector(tuple(min(p[i] for p in points) for i in range(3)))
    high = Vector(tuple(max(p[i] for p in points) for i in range(3)))
    center = (low + high) / 2
    size = max(high - low)
    scene.camera.location = center + Vector((0, -3, .32)) * size
    scene.camera.rotation_euler = (center-scene.camera.location).to_track_quat('-Z', 'Y').to_euler()
    scene.camera.data.ortho_scale = size * 1.3
    scene.frame_start, scene.frame_end = 1, 72
    bone = rig.pose.bones['head']
    bone.rotation_mode = 'XYZ'
    # A single attentive glance with a hold and gentle return, not perpetual sway.
    # Frame 73 duplicates frame 1 only for interpolation; export frames 1..72.
    for frame, yaw, tilt in [(1, 0, 0), (13, 0, 0), (29, 7, -3),
                             (41, 7, -3), (65, 0, 0), (73, 0, 0)]:
        bone.rotation_euler = (0, math.radians(yaw), math.radians(tilt))
        bone.keyframe_insert(data_path='rotation_euler', frame=frame)
    # Blender's automatic clamped Bezier handles avoid overshoot at holds.
    action = rig.animation_data.action
    for layer in action.layers:
        for strip in layer.strips:
            for bag in strip.channelbags:
                for curve in bag.fcurves:
                    for key in curve.keyframe_points:
                        key.interpolation = 'BEZIER'
                        key.handle_left_type = key.handle_right_type = 'AUTO_CLAMPED'
    scene.frame_set(1)
    studies[pet] = dict(scene=scene, mesh=mesh, rig=rig, original=original,
                        before=before, coords=coords)
    print(json.dumps({'pet': pet, 'vertices': len(coords), 'independent_mesh': True,
                      'bones': ['body_anchor', 'head'], 'frames': 72}))


def validate(pet):
    state = studies[pet]
    scene, mesh = state['scene'], state['mesh']
    bpy.context.window.scene = scene
    assert max(abs(state['rig'].matrix_world[r][c]-mesh.matrix_world[r][c])
               for r in range(4) for c in range(4)) < 1e-6, 'Rig/mesh coordinate mismatch'
    baseline = state['coords']
    feet = baseline[:, 1] < .12
    results = []
    for frame in range(1, 74):
        scene.frame_set(frame)
        evaluated = mesh.evaluated_get(bpy.context.evaluated_depsgraph_get())
        data = evaluated.to_mesh()
        try:
            points = np.empty(baseline.size, dtype=np.float32)
            data.vertices.foreach_get('co', points)
            delta = np.linalg.norm(points.reshape(-1, 3) - baseline, axis=1)
            foot_drift = float(delta[feet].max())
            assert foot_drift < 1e-6, (frame, foot_drift)
            if frame in (1, 65, 73):
                assert float(delta.max()) < 1e-6
            if frame in (1, 21, 29, 41, 53, 65, 73):
                results.append(dict(frame=frame, max_displacement=float(delta.max()),
                                    foot_drift=foot_drift))
        finally:
            evaluated.to_mesh_clear()
    assert geometry_digest(state['original'].data) == state['before']
    assert not state['original'].vertex_groups and not state['original'].modifiers
    assert geometry_digest(mesh.data) == state['before']
    scene.frame_set(1)
    report = {'pet': pet, 'source_geometry_unchanged': True,
              'bind_geometry_unchanged': True, 'source_unrigged': True,
              'validated_frame_count': 73,
              'sampled_frames': results, 'production_ready': False}
    (OUT / f'{pet}-validation.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report))


def render(pet, frame):
    scene = studies[pet]['scene']
    bpy.context.window.scene = scene
    scene.frame_set(frame)
    scene.render.filepath = str(OUT / f'{pet}-{frame:03d}.png')
    bpy.ops.render.render(write_still=True, scene=scene.name)


def save():
    for state in studies.values():
        state['scene'].frame_set(1)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'hunyuan-head-study.blend'),
                               copy=True, compress=True)
