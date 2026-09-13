"""Inspect the user-authorized Rodin base mesh without modifying its geometry.

Execute through Blender MCP with __file__ set. Raw imported blend is retained.
No model requests, credentials, or generated geometry live in this script.
"""
import bpy
from pathlib import Path
from mathutils import Vector

SCENE='Naitang_Rodin_Approved_9qoNnY'
MODEL='Naitang_Rodin_9qoNnY'
PREFIX='RodinNTQA_'
OUT=Path(__file__).resolve().parents[2]/'output/rodin-naitang/run-9qoNnY'


def setup():
    scene=bpy.data.scenes[SCENE]
    bpy.context.window.scene=scene
    obj=bpy.data.objects[MODEL]
    if obj.name not in scene.objects:raise RuntimeError('Model outside target scene')
    if bpy.data.objects.get(PREFIX+'Camera'):raise RuntimeError('Preview studio already exists')
    bounds=[obj.matrix_world@Vector(corner) for corner in obj.bound_box]
    target=sum(bounds,Vector())/8
    height=max(v.z for v in bounds)-min(v.z for v in bounds)
    scene.render.engine='CYCLES';scene.cycles.samples=32;scene.cycles.use_denoising=True
    scene.render.resolution_x=640;scene.render.resolution_y=640;scene.render.resolution_percentage=100
    scene.render.image_settings.file_format='PNG';scene.render.image_settings.color_mode='RGBA'
    scene.render.film_transparent=True;scene.view_settings.view_transform='AgX'
    world=bpy.data.worlds.new(PREFIX+'World');world.use_nodes=True
    bg=next(n for n in world.node_tree.nodes if n.bl_idname=='ShaderNodeBackground')
    bg.inputs['Color'].default_value=(.70,.75,.82,1);bg.inputs['Strength'].default_value=.35
    scene.world=world
    def area(name,location,power,size):
        data=bpy.data.lights.new(PREFIX+name,'AREA');data.energy=power;data.size=size
        light=bpy.data.objects.new(PREFIX+name,data);scene.collection.objects.link(light);light.location=location
        light.rotation_euler=(target-light.location).to_track_quat('-Z','Y').to_euler()
    area('Key',(-3,-4,5),450,4)
    area('Fill',(4,-1,3),240,4)
    area('Back',(0,4,4),350,4)
    data=bpy.data.cameras.new(PREFIX+'Camera');data.type='ORTHO';data.ortho_scale=height*1.35
    camera=bpy.data.objects.new(PREFIX+'Camera',data);scene.collection.objects.link(camera);scene.camera=camera
    camera['look_target']=list(target)
    print('Prepared four-view studio; source model transforms unchanged')


def render(view):
    scene=bpy.data.scenes[SCENE];bpy.context.window.scene=scene
    camera=scene.camera;target=Vector(camera['look_target'])
    offsets={'negative-y':(0,-5,1.0),'positive-y':(0,5,1.0),'positive-x':(5,0,1.0),'negative-x':(-5,0,1.0),'three-quarter':(3.4,-5,1.3)}
    camera.location=target+Vector(offsets[view])
    camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
    scene.render.filepath=str(OUT/f'rodin-{view}.png')
    bpy.ops.render.render(write_still=True,scene=SCENE)
    print(scene.render.filepath)


def save():
    scene=bpy.data.scenes[SCENE];bpy.context.window.scene=scene
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'naitang-rodin-preview.blend'))
    for obj in scene.objects:obj.select_set(False)
    obj=bpy.data.objects[MODEL];obj.select_set(True);bpy.context.view_layer.objects.active=obj
    bpy.ops.export_scene.gltf(filepath=str(OUT/'naitang-base.glb'),export_format='GLB',use_selection=True,use_active_scene=True,export_animations=False)
    print('Saved packed Blender preview and selected-model GLB')
