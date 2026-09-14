"""REJECTED visual experiment: surface eyelids on supplied Hunyuan meshes.

Run in Blender with EYELID_OUTPUT. Never changes the original head geometry or
texture pixels. Eyelids are additional fitted surfaces, not squashed eye art.
Generated eyelids are hidden by default. No animation/export/apply path is
provided: the overlays still look detached and must not become desktop assets.
"""
from pathlib import Path
import json
import math
import hashlib
import runpy
import numpy as np

import bpy
from mathutils import Vector
from mathutils.geometry import barycentric_transform
from mathutils.bvhtree import BVHTree

OUT = Path(EYELID_OUTPUT).resolve()
RUN = OUT.name.rsplit('-', 1)[-1]
studies = {}
EYES = {
    'naitang': [(281, 500, 84, 71), (552, 457, 78, 71)],
    'pudding': [(220, 379, 55, 54), (474, 323, 56, 51)],
}
LEVELS = (.0, .1, .25, .5, .75, .9, 1.)


def create(pet):
    source = bpy.data.scenes[f'HY_Secondary_{pet}_SPQ4T0']
    name = f'HY_Eyelids_{pet}_{RUN}'
    if name in bpy.data.scenes:
        raise RuntimeError('Refusing to overwrite study')
    target = bpy.data.scenes.new(name)
    bpy.context.window.scene = target
    target.world = source.world.copy()
    target.render.engine = 'CYCLES'
    target.cycles.samples = 24
    target.cycles.use_denoising = True
    target.cycles.seed = 0
    target.render.film_transparent = True
    target.render.resolution_x = target.render.resolution_y = 768
    target.render.resolution_percentage = 100
    target.render.image_settings.file_format = 'PNG'
    target.render.image_settings.color_mode = 'RGBA'
    target.render.fps = 24
    target.frame_start, target.frame_end = 1, 72
    target.view_settings.view_transform = source.view_settings.view_transform
    target.view_settings.look = source.view_settings.look
    target.view_settings.exposure = source.view_settings.exposure
    target.view_settings.gamma = source.view_settings.gamma
    copies = {}
    for obj in source.objects:
        copy = obj.copy()
        copy.data = obj.data.copy()
        copy.name = f'HYEye_{pet}_{RUN}_{obj.type}'
        if copy.animation_data and copy.animation_data.action:
            copy.animation_data.action = copy.animation_data.action.copy()
        target.collection.objects.link(copy)
        copies[obj] = copy
        if obj == source.camera:
            target.camera = copy
        if obj.type == 'MESH':
            for slot in copy.material_slots:
                material = slot.material.copy()
                slot.link = 'OBJECT'
                slot.material = material
    for copy in copies.values():
        if copy.parent in copies:
            copy.parent = copies[copy.parent]
        for modifier in copy.modifiers:
            if modifier.type == 'ARMATURE':
                modifier.object = copies[modifier.object]
    target.frame_set(1)
    target.view_layers[0].update()
    mesh = next(o for o in copies.values() if o.type == 'MESH')
    rig = next(o for o in copies.values() if o.type == 'ARMATURE')
    assert max(abs(rig.matrix_world[r][c]-mesh.matrix_world[r][c])
               for r in range(4) for c in range(4)) < 1e-6
    tree = BVHTree.FromObject(mesh, bpy.context.evaluated_depsgraph_get())
    studies[pet] = dict(scene=target, mesh=mesh, rig=rig, tree=tree,
                        original=next(o for o in source.objects if o.type == 'MESH'),
                        camera_matrix=target.camera.matrix_world.copy(),
                        camera_scale=target.camera.data.ortho_scale)
    target['production_ready'] = False
    target['study_scope'] = 'additional fitted eyelid surfaces; original eye art untouched'
    print(json.dumps({'pet': pet, 'vertices': len(mesh.data.vertices),
                      'original_shape_keys': bool(mesh.data.shape_keys),
                      'original_material_slots': len(mesh.material_slots)}))


def closeup(pet, label='anatomy'):
    state = studies[pet]
    scene = state['scene']
    bpy.context.window.scene = scene
    scene.camera.location = (.02, -3, .66)
    scene.camera.rotation_euler = (Vector((.02, -.22, .66))-scene.camera.location).to_track_quat('-Z', 'Y').to_euler()
    scene.camera.data.ortho_scale = .42
    scene.render.filepath = str(OUT/f'{pet}-{label}.png')
    bpy.ops.render.render(write_still=True, scene=scene.name)


def sample(state, x, y):
    position, normal, index, _distance = state['tree'].ray_cast((x, y, 2), (0, 0, -1))
    if position is None:
        raise ValueError('Eyelid projection missed the face')
    mesh = state['mesh'].data
    face = mesh.polygons[index]
    if len(face.vertices) != 3:
        raise ValueError('Expected imported triangle mesh')
    verts = [mesh.vertices[i].co for i in face.vertices]
    uv = [Vector((*mesh.uv_layers.active.data[i].uv, 0)) for i in face.loop_indices]
    coordinates = barycentric_transform(position, *verts, *uv)
    return position, coordinates[:2]


def eyelid_material(state):
    source = state['mesh'].material_slots[0].material
    texture = next(n.image for n in source.node_tree.nodes
                   if n.bl_idname == 'ShaderNodeTexImage' and n.label == 'BASE COLOR')
    material = bpy.data.materials.new('Fitted eyelid fur '+RUN)
    material.use_nodes = True
    tree = material.node_tree
    shader = next(n for n in tree.nodes if n.bl_idname == 'ShaderNodeBsdfPrincipled')
    colors = tree.nodes.new('ShaderNodeVertexColor')
    colors.layer_name = 'Sampled fur color'
    tree.links.new(colors.outputs['Color'], shader.inputs['Base Color'])
    shader.inputs['Roughness'].default_value = .75
    shader.inputs['Specular IOR Level'].default_value = .15
    pixels = np.empty(texture.size[0]*texture.size[1]*4, dtype=np.float32)
    texture.pixels.foreach_get(pixels)
    state['color_pixels'] = pixels.reshape(texture.size[1], texture.size[0], 4)
    rim = bpy.data.materials.new('Eyelid margin '+RUN)
    rim.use_nodes = True
    shader = next(n for n in rim.node_tree.nodes if n.bl_idname == 'ShaderNodeBsdfPrincipled')
    shader.inputs['Base Color'].default_value = (.10, .052, .027, 1)
    shader.inputs['Roughness'].default_value = .65
    return material, rim


def sampled_color(state, uv):
    pixels = state['color_pixels']
    height, width, _ = pixels.shape
    x, y = np.clip(uv[0], 0, 1)*(width-1), np.clip(uv[1], 0, 1)*(height-1)
    x0, y0 = int(x), int(y)
    x1, y1 = min(x0+1, width-1), min(y0+1, height-1)
    dx, dy = x-x0, y-y0
    rgb = ((pixels[y0,x0,:3]*(1-dx)+pixels[y0,x1,:3]*dx)*(1-dy)
           +(pixels[y1,x0,:3]*(1-dx)+pixels[y1,x1,:3]*dx)*dy)
    # Packed base-color image is sRGB; FLOAT_COLOR attributes contain linear RGB.
    linear = np.where(rgb <= .04045, rgb/12.92, ((rgb+.055)/1.055)**2.4)
    return (*linear, 1)


def build_lids(pet, preview=False):
    state = studies[pet]
    if 'lids' in state:
        raise RuntimeError('Eyelids already exist')
    scene = state['scene']
    bpy.context.window.scene = scene
    scene.frame_set(1)
    materials = eyelid_material(state)
    lids = []
    nu, nv = 96, 40
    for eye_index, (px, py, rx_px, ry_px) in enumerate(EYES[pet]):
        cx, cy = .02+(px/768-.5)*.42, .66+(.5-py/768)*.42
        rx, ry = rx_px/768*.42, ry_px/768*.42
        boundary = [sample(state, cx+rx*math.cos(a), cy+ry*math.sin(a))[0]
                    for a in np.linspace(0, 2*math.pi, 64, endpoint=False)]
        boundary = np.asarray(boundary)
        plane = np.linalg.lstsq(np.column_stack((boundary[:, :2], np.ones(64))), boundary[:, 2], rcond=None)[0]
        for upper in (True, False):
            side = 1 if upper else -1
            shapes = [[] for _ in LEVELS]
            texture_uvs = []
            vertex_colors = []
            for i in range(nu+1):
                u = -.998+1.996*i/nu
                arc = math.sqrt(1-u*u)
                x = cx+rx*u
                # Include the surrounding skin collar. A cap confined inside
                # the iris outline looks detached even when it occludes well.
                outer_y = cy+side*(ry+.030)*arc
                open_y = cy+side*ry*arc
                seam_y = cy-.65*ry*arc
                for j in range(nv+1):
                    v = 1-(1-j/nv)**2
                    rest_y = outer_y*(1-v)+open_y*v
                    _, uv = sample(state, x, rest_y)
                    texture_uvs.append(uv)
                    vertex_colors.append(sampled_color(state, uv))
                    for positions, closure in zip(shapes, LEVELS):
                        edge_y = (1-closure)*open_y+closure*seam_y
                        y = outer_y*(1-v)+edge_y*v
                        point, _ = sample(state, x, y)
                        # Open collar follows source skin; moving lid clears
                        # the eye surface. This still fails visual acceptance.
                        if closure == 0:
                            point.z += .0007*math.sin(v*math.pi/2)-.0003*(1-v)**6
                        else:
                            dome = max(0, 1-u*u-((y-cy)/ry)**2)*.003
                            fitted = plane[0]*x+plane[1]*y+plane[2]+dome
                            blend = math.sin(v*math.pi/2)**.3
                            point.z += (max(0, fitted-point.z)+.0015)*blend-.0003*(1-v)**6
                        positions.append(tuple(point))
            faces = []
            for i in range(nu):
                for j in range(nv):
                    a = i*(nv+1)+j
                    face = (a, a+1, a+nv+2, a+nv+1)
                    faces.append(face if upper else face[::-1])
            mesh = bpy.data.meshes.new(f'Lid_{pet}_{eye_index}_{upper}_{RUN}')
            mesh.from_pydata(shapes[0], [], faces)
            mesh.materials.append(materials[0])
            mesh.materials.append(materials[1])
            colors = mesh.color_attributes.new(name='Sampled fur color', type='FLOAT_COLOR', domain='POINT')
            colors.data.foreach_set('color', np.asarray(vertex_colors, dtype=np.float32).ravel())
            uv_layer = mesh.uv_layers.new(name='Source fur samples')
            for face in mesh.polygons:
                face.use_smooth = True
                face.material_index = 1 if face.index % nv >= nv-2 else 0
                for loop in face.loop_indices:
                    uv_layer.data[loop].uv = texture_uvs[mesh.loops[loop].vertex_index]
            obj = bpy.data.objects.new(mesh.name, mesh)
            scene.collection.objects.link(obj)
            obj.visible_shadow = False
            obj.hide_render = not preview
            obj.matrix_world = state['mesh'].matrix_world.copy()
            obj.shape_key_add(name='Open')
            for positions, level in zip(shapes[1:], LEVELS[1:]):
                key = obj.shape_key_add(name=f'Closure {level}')
                for vertex, position in zip(key.data, positions):
                    vertex.co = position
            group = obj.vertex_groups.new(name='head')
            group.add(list(range(len(mesh.vertices))), 1, 'REPLACE')
            modifier = obj.modifiers.new('Follow head', 'ARMATURE')
            modifier.object = state['rig']
            modifier.use_deform_preserve_volume = True
            obj['eye_index'] = eye_index
            obj['upper_lid'] = upper
            lids.append(obj)
    state['lids'] = lids
    scene.view_layers[0].update()
    for obj in lids:
        obj.hide_set(not preview, view_layer=scene.view_layers[0])
    del state['color_pixels']
    print(json.dumps({'pet': pet, 'eyelid_surfaces': len(lids),
                      'vertices_per_surface': len(lids[0].data.vertices)}))


def set_closure(pet, level):
    if not 0 <= level <= 1:
        raise ValueError('Closure must be in [0, 1]')
    for obj in studies[pet]['lids']:
        keys = obj.data.shape_keys.key_blocks
        for key in list(keys)[1:]:
            key.value = 0
        for lo, hi in zip(LEVELS, LEVELS[1:]):
            if lo <= level <= hi:
                blend = (level-lo)/(hi-lo)
                if lo > 0:
                    keys[f'Closure {lo}'].value = 1-blend
                keys[f'Closure {hi}'].value = blend
                break
    studies[pet]['scene'].view_layers[0].update()


def verify_and_reject(pet):
    state = studies[pet]
    mesh, original = state['mesh'], state['original']
    def coordinates(data):
        values = np.empty(len(data.vertices)*3, dtype=np.float32)
        data.vertices.foreach_get('co', values)
        return values
    current = coordinates(mesh.data)
    source = coordinates(original.data)
    assert mesh.data != original.data and np.array_equal(current, source)
    assert not original.data.shape_keys and not mesh.data.shape_keys
    assert mesh.data.uv_layers.active and len(mesh.data.uv_layers.active.data), 'Missing original UVs'
    for level in (0, .1, .25, .5, .75, .9, 1):
        set_closure(pet, level)
        for obj in state['lids']:
            keys = list(obj.data.shape_keys.key_blocks)[1:]
            total = sum(key.value for key in keys)
            assert 0 <= total <= 1.000001
            assert all(key.relative_key.name == 'Open' for key in keys)
            assert obj.modifiers[0].object == state['rig']
    set_closure(pet, 0)
    for obj in state['lids'] + state.get('rejected_lids', []):
        obj.hide_render = True
        obj.hide_set(True, view_layer=state['scene'].view_layers[0])
        obj['visual_acceptance'] = 'rejected'
    state['scene'].frame_set(1)
    state['scene']['visual_acceptance'] = 'rejected'
    state['scene']['production_ready'] = False
    report = {
        'pet': pet, 'source_geometry_unchanged': True,
        'coordinate_sha256': hashlib.sha256(current.tobytes()).hexdigest(),
        'shape_key_numeric_checks': 'passed', 'visual_acceptance': 'rejected',
        'production_ready': False, 'all_experimental_lids_hidden': True,
        'reason': ['detached eyelid shell appearance', 'skin-collar seams',
                   'residual source eyelid/eye-rim artifacts'],
        'next_step': 'local eyelid/socket retopology, not more overlay shells',
        'connectivity': state.get('connectivity'),
    }
    (OUT/f'{pet}-eyelid-qa.json').write_text(json.dumps(report, indent=2))
    print(json.dumps({k:v for k,v in report.items() if k != 'connectivity'}))


def inspect_connectivity(pet):
    state = studies[pet]
    mesh = state['mesh'].data
    vertices = np.empty(len(mesh.vertices)*3, dtype=np.float32)
    mesh.vertices.foreach_get('co', vertices)
    edges = np.empty(len(mesh.edges)*2, dtype=np.int32)
    mesh.edges.foreach_get('vertices', edges)
    helper = runpy.run_path(str(Path(__file__).with_name('hunyuan_connectivity.py')))
    report = helper['inspect'](vertices.reshape(-1,3), edges.reshape(-1,2))
    state['connectivity'] = report
    (OUT/f'{pet}-connectivity.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report))


def save():
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'hunyuan-eyelid-study.blend'), copy=True, compress=True)
