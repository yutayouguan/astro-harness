"""Import user GLBs into isolated Blender review scenes, without mesh edits.

Run stages via Blender MCP. Supply __file__ and REVIEW_OUTPUT in the namespace.
This tool never decimates, welds, applies transforms, or overwrites a source GLB.
"""
import bpy
import json
from pathlib import Path
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = Path(REVIEW_OUTPUT).resolve()
RUN_ID = OUTPUT.name.rsplit('-', 1)[-1]
SOURCES = {'naitang': '奶糖.glb', 'pudding': '布丁.glb'}
SCENES = {pet: f'Hunyuan_{pet}_{RUN_ID}' for pet in SOURCES}
records = {}


def activate(pet):
    scene = bpy.data.scenes[SCENES[pet]]
    bpy.context.window.scene = scene
    return scene


def import_model(pet):
    scene_name = SCENES[pet]
    if scene_name in bpy.data.scenes:
        raise RuntimeError('Review scene already exists; refusing to replace it')
    path = (ROOT / 'designs/3d' / SOURCES[pet]).resolve()
    if path.parent != (ROOT / 'designs/3d').resolve():
        raise RuntimeError('Source escapes the requested model directory')
    scene = bpy.data.scenes.new(scene_name)
    bpy.context.window.scene = scene
    scene['astro_review_source'] = str(path)
    scene['astro_review_geometry_modified'] = False
    bpy.ops.import_scene.gltf(filepath=str(path), import_pack_images=True, merge_vertices=False)
    objects = list(scene.objects)
    meshes = [obj for obj in objects if obj.type == 'MESH']
    if not meshes:
        raise RuntimeError('GLB contains no mesh')
    bounds = [obj.matrix_world @ Vector(point) for obj in meshes for point in obj.bound_box]
    minimum = Vector(tuple(min(point[a] for point in bounds) for a in range(3)))
    maximum = Vector(tuple(max(point[a] for point in bounds) for a in range(3)))
    details = []
    for index, obj in enumerate(meshes):
        obj.name = f'HY_{pet}_{RUN_ID}_Source_{index}'
        data = obj.data
        images = {}
        material_details = []
        for material in data.materials:
            if not material or not material.node_tree:
                continue
            image_nodes = [node for node in material.node_tree.nodes if node.bl_idname == 'ShaderNodeTexImage' and node.image]
            for node in image_nodes:
                image = node.image
                images[image.name] = dict(name=image.name, size=list(image.size), channels=image.channels,
                                         packed=bool(image.packed_file), colorSpace=image.colorspace_settings.name)
            material_details.append(dict(name=material.name, textures=[node.image.name for node in image_nodes]))
        details.append(dict(name=obj.name, vertices=len(data.vertices), faces=len(data.polygons),
                            triangles=sum(len(p.vertices)-2 for p in data.polygons),
                            uvLayers=[layer.name for layer in data.uv_layers],
                            shapeKeys=len(data.shape_keys.key_blocks) if data.shape_keys else 0,
                            armatureModifiers=sum(mod.type == 'ARMATURE' for mod in obj.modifiers),
                            materials=material_details, images=list(images.values()),
                            matrixWorld=[list(row) for row in obj.matrix_world]))
    record = dict(source=str(path), scene=scene_name, objects=len(objects),
                  armatures=sum(obj.type == 'ARMATURE' for obj in objects),
                  boundsMin=list(minimum), boundsMax=list(maximum), dimensions=list(maximum-minimum), meshes=details)
    records[pet] = record
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / f'{pet}-inspection.json').write_text(json.dumps(record, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps(record, ensure_ascii=False))


def studio(pet):
    scene = activate(pet)
    if scene.camera:
        raise RuntimeError('Review studio already exists; refusing to add duplicate lights')
    record = records[pet]
    low, high = Vector(record['boundsMin']), Vector(record['boundsMax'])
    center = (low + high) / 2
    size = max(high-low)
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 20
    scene.cycles.use_denoising = True
    scene.render.film_transparent = True
    scene.render.resolution_x = 640
    scene.render.resolution_y = 640
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGBA'
    scene.view_settings.view_transform = 'AgX'
    world = bpy.data.worlds.new(f'HY_{pet}_{RUN_ID}_World')
    world.use_nodes = True
    background = next(node for node in world.node_tree.nodes if node.bl_idname == 'ShaderNodeBackground')
    background.inputs['Color'].default_value = (.6,.65,.72,1)
    background.inputs['Strength'].default_value = .35
    scene.world = world
    for name, direction, power, color in [
        ('Key',(-1.5,-2,2.5),250,(1,.93,.85)),
        ('Fill',(1.8,-1,1.3),110,(.85,.91,1)),
        ('Rim',(.6,1.8,2),200,(1,.92,.84)),
    ]:
        data = bpy.data.lights.new(f'HY_{pet}_{RUN_ID}_{name}','AREA')
        data.energy = power * size * size
        data.shape = 'DISK'
        data.size = size * 1.6
        obj = bpy.data.objects.new(data.name,data)
        scene.collection.objects.link(obj)
        obj.location = center + Vector(direction)*size
        obj.rotation_euler = (center-obj.location).to_track_quat('-Z','Y').to_euler()
    data = bpy.data.cameras.new(f'HY_{pet}_{RUN_ID}_Camera')
    data.type = 'ORTHO'
    data.ortho_scale = size * 1.3
    camera = bpy.data.objects.new(data.name,data)
    scene.collection.objects.link(camera)
    scene.camera = camera
    set_view(pet,'front')
    print('Added a review-only camera and studio lights; source materials untouched')


def set_view(pet, view):
    scene = activate(pet)
    record = records[pet]
    low, high = Vector(record['boundsMin']), Vector(record['boundsMax'])
    center = (low+high)/2
    size = max(high-low)
    direction = {'front':(0,-3,.32),'three-quarter':(1.55,-3,.45),'side':(3,0,.32),'back':(0,3,.32)}[view]
    scene.camera.location = center + Vector(direction)*size
    scene.camera.rotation_euler = (center-scene.camera.location).to_track_quat('-Z','Y').to_euler()


def render(pet, view='front'):
    scene = activate(pet)
    set_view(pet,view)
    scene.render.filepath = str(OUTPUT / f'{pet}-{view}.png')
    bpy.ops.render.render(write_still=True,scene=scene.name)
    print('Rendered '+scene.render.filepath)


def save_copy():
    path = OUTPUT / 'hunyuan-pets-review.blend'
    bpy.ops.wm.save_as_mainfile(filepath=str(path),copy=True,compress=True)
    print('Saved editable review copy: '+str(path))
