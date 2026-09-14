"""Run inside Blender: regression check for freshly linked Y-up meshes.

Creates only tagged test fixtures and removes exactly those fixtures afterward.
Uses the real rig builder, not a mock, and checks a rigid head vertex in motion.
"""
import math
from pathlib import Path
import runpy
import uuid

import bpy
from mathutils import Vector


def check():
    token = 'RigCoordinatesTest_' + uuid.uuid4().hex[:8]
    before_scene = bpy.context.window.scene
    before_objects = set(bpy.data.objects)
    before_scenes = set(bpy.data.scenes)
    owned = {kind: set(getattr(bpy.data, kind)) for kind in
             ('meshes', 'armatures', 'cameras', 'worlds', 'materials', 'actions')}
    try:
        source = bpy.data.scenes.new(token)
        bpy.context.window.scene = source
        source.world = bpy.data.worlds.new(token)
        mesh = bpy.data.meshes.new(token)
        mesh.from_pydata([(-.05,.1,.05),(.05,.1,.05),(0,.1,-.05),
                          (-.05,.7,.2),(.05,.7,.2),(0,.75,.15)], [], [(0,1,2),(3,4,5)])
        obj = bpy.data.objects.new(token, mesh)
        source.collection.objects.link(obj)
        obj.rotation_euler.x = math.pi/2
        camera = bpy.data.objects.new(token+'_Camera', bpy.data.cameras.new(token))
        source.collection.objects.link(camera)
        source.camera = camera
        source.view_layers[0].update()
        namespace = runpy.run_path(str(Path(__file__).with_name('rig_hunyuan_head_study.py')),
                                  init_globals={'RIG_OUTPUT': '/tmp/'+token})
        namespace['SOURCES']['naitang'] = source.name
        namespace['create']('naitang')
        state = namespace['studies']['naitang']
        rig, copy, scene = state['rig'], state['mesh'], state['scene']
        scene.frame_set(29)
        scene.view_layers[0].update()
        error = max(abs(rig.matrix_world[r][c]-copy.matrix_world[r][c])
                    for r in range(4) for c in range(4))
        assert error < 1e-6
        assert abs(rig.matrix_world[1][2]) > .99, 'Must not silently use identity'
        bone = rig.pose.bones['head']
        expected = bone.matrix @ bone.bone.matrix_local.inverted() @ Vector(mesh.vertices[3].co)
        evaluated = copy.evaluated_get(bpy.context.evaluated_depsgraph_get())
        data = evaluated.to_mesh()
        try:
            error = (data.vertices[3].co-expected).length
            assert error < 1e-6, error
        finally:
            evaluated.to_mesh_clear()
        assert not obj.modifiers and not obj.vertex_groups and copy.data != mesh
        print({'coordinate_regression_passed': True, 'rigid_vertex_error': error})
    finally:
        bpy.context.window.scene = before_scene
        # No pre-existing data may be removed, even if the assertion fails.
        for obj in set(bpy.data.objects)-before_objects:
            bpy.data.objects.remove(obj, do_unlink=True)
        for scene in set(bpy.data.scenes)-before_scenes:
            bpy.data.scenes.remove(scene)
        for kind, existing in owned.items():
            collection = getattr(bpy.data, kind)
            for item in set(collection)-existing:
                if item.users == 0:
                    collection.remove(item)


if __name__ == '__main__':
    check()
