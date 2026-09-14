"""Non-destructive rear-coat material study for the user-provided Hunyuan pets.

The original meshes, image pixels, UVs and materials remain untouched. Material
overrides belong to copied objects in new scenes, not to shared mesh data.
"""
import bpy
import json
from pathlib import Path
from mathutils import Vector

OUT = Path(MATERIAL_OUTPUT).resolve()
RUN = OUT.name.rsplit('-',1)[-1]
SOURCE_SCENES = dict(globals().get('SOURCE_SCENES', {'naitang':'Hunyuan_naitang_LFO0Y1','pudding':'Hunyuan_pudding_LFO0Y1'}))
TARGET_SCENES = {pet:f'HY_Coat_{pet}_{RUN}' for pet in SOURCE_SCENES}
audit = {}


def node(tree, kind, label, x, y):
    n=tree.nodes.new(kind);n.label=label;n.name='HYCoat_'+label;n.location=(x,y)
    return n


def constant(tree, value, label):
    n=node(tree,'ShaderNodeValue',label,-1000,-1000)
    n.outputs[0].default_value=value
    return n.outputs[0]


def original_socket(tree, socket, label):
    if socket.is_linked:return socket.links[0].from_socket
    if socket.type=='RGBA':
        n=node(tree,'ShaderNodeRGB',label,-1000,-800)
        n.outputs[0].default_value=socket.default_value
        return n.outputs[0]
    return constant(tree,socket.default_value,label)


def math_node(tree, operation, a, b, label):
    n=node(tree,'ShaderNodeMath',label,-200,-1000)
    n.operation=operation
    for i,value in enumerate((a,b)):
        if isinstance(value,(int,float)):n.inputs[i].default_value=value
        else:tree.links.new(value,n.inputs[i])
    return n.outputs[0]


def range_node(tree, source, low, high, start, end, label):
    n=node(tree,'ShaderNodeMapRange',label,-600,-800)
    n.interpolation_type='SMOOTHSTEP';n.clamp=True
    tree.links.new(source,n.inputs['Value'])
    for key,value in [('From Min',low),('From Max',high),('To Min',start),('To Max',end)]:n.inputs[key].default_value=value
    return n.outputs[0]


def build_material(source, pet):
    material=source.copy();material.name=f'HY_{pet}_RearCoat_{RUN}'
    if material.node_tree==source.node_tree:raise RuntimeError('Material graph was not isolated')
    tree=material.node_tree
    shader=next(n for n in tree.nodes if n.bl_idname=='ShaderNodeBsdfPrincipled')
    base=original_socket(tree,shader.inputs['Base Color'],'Original color')
    rough=original_socket(tree,shader.inputs['Roughness'],'Original roughness')
    normal=shader.inputs['Normal'].links[0].from_socket if shader.inputs['Normal'].is_linked else None
    tex=node(tree,'ShaderNodeTexCoord','Model coordinates',-1400,0)
    xyz=node(tree,'ShaderNodeSeparateXYZ','Local glTF axes',-1200,0)
    tree.links.new(tex.outputs['Generated'],xyz.inputs[0])
    # glTF Y is height. Local +Z faces the camera after the imported X rotation.
    depth=range_node(tree,xyz.outputs['Z'],.22,.65,1,0,'Rear depth mask')
    geo=node(tree,'ShaderNodeNewGeometry','Surface orientation',-1400,-350)
    transform=node(tree,'ShaderNodeVectorTransform','Normal into object space',-1200,-350)
    transform.vector_type='NORMAL';transform.convert_from='WORLD';transform.convert_to='OBJECT'
    tree.links.new(geo.outputs['Normal'],transform.inputs[0])
    nxyz=node(tree,'ShaderNodeSeparateXYZ','Facing direction',-1000,-350)
    tree.links.new(transform.outputs[0],nxyz.inputs[0])
    facing=range_node(tree,nxyz.outputs['Z'],-.15,.25,1,0,'Protect front-facing surface')
    mask=math_node(tree,'MULTIPLY',depth,facing,'Rear-only blend')
    if pet=='naitang':
        waves=[]
        for direction,scale,distortion in [('Y',4.0,2.7),('X',3.0,1.2)]:
            wave=node(tree,'ShaderNodeTexWave','Tabby '+direction,-1000,300 if direction=='Y' else 520)
            wave.wave_type='BANDS';wave.bands_direction=direction
            wave.inputs['Scale'].default_value=scale;wave.inputs['Distortion'].default_value=distortion
            wave.inputs['Detail'].default_value=2;wave.inputs['Detail Scale'].default_value=1.8
            tree.links.new(tex.outputs['Generated'],wave.inputs['Vector'])
            waves.append(range_node(tree,wave.outputs['Fac'],.55,.83,0,1,'Soft stripe '+direction))
        head=range_node(tree,xyz.outputs['Y'],.60,.74,0,1,'Crown orientation blend')
        body_weight=math_node(tree,'SUBTRACT',1,head,'Body region')
        pattern=math_node(tree,'ADD',math_node(tree,'MULTIPLY',waves[0],body_weight,'Body bands'),math_node(tree,'MULTIPLY',waves[1],head,'Crown bands'),'Tabby pattern')
        amount=math_node(tree,'MULTIPLY',math_node(tree,'MULTIPLY',pattern,mask,'Masked tabby'),.42,'Tabby strength')
        mix=node(tree,'ShaderNodeMixRGB','Ginger stripe pigment',0,250)
        mix.blend_type='MULTIPLY';mix.inputs[2].default_value=(.48,.24,.10,1)
        tree.links.new(amount,mix.inputs[0]);tree.links.new(base,mix.inputs[1]);base=mix.outputs[0]
    tree.links.new(base,shader.inputs['Base Color'])
    stretch=node(tree,'ShaderNodeVectorMath','Directional microfibers',-1000,-600)
    stretch.operation='MULTIPLY';stretch.inputs[1].default_value=(150,22,150)
    tree.links.new(tex.outputs['Generated'],stretch.inputs[0])
    grain=node(tree,'ShaderNodeTexNoise','Fine rear coat grain',-750,-600)
    grain.inputs['Scale'].default_value=1;grain.inputs['Detail'].default_value=2;grain.inputs['Roughness'].default_value=.65
    tree.links.new(stretch.outputs[0],grain.inputs['Vector'])
    bump=node(tree,'ShaderNodeBump','Rear coat micro-normal',0,-200)
    bump.inputs['Distance'].default_value=.0015
    tree.links.new(math_node(tree,'MULTIPLY',mask,.15,'Micro-normal strength'),bump.inputs['Strength'])
    tree.links.new(grain.outputs['Fac'],bump.inputs['Height'])
    if normal:tree.links.new(normal,bump.inputs['Normal'])
    tree.links.new(bump.outputs[0],shader.inputs['Normal'])
    tree.links.new(math_node(tree,'MAXIMUM',rough,math_node(tree,'MULTIPLY',mask,.68,'Rear roughness floor'),'Retain front roughness'),shader.inputs['Roughness'])
    material['astro_material_study']='rear-only-v1'
    material['source_material']=source.name
    return material


def create(pet):
    source=bpy.data.scenes[SOURCE_SCENES[pet]]
    target_name=TARGET_SCENES[pet]
    if target_name in bpy.data.scenes:raise RuntimeError('Material study already exists')
    target=bpy.data.scenes.new(target_name)
    bpy.context.window.scene=target
    target.render.engine=source.render.engine
    target.cycles.samples=source.cycles.samples;target.cycles.use_denoising=source.cycles.use_denoising;target.cycles.seed=source.cycles.seed
    target.render.resolution_x=source.render.resolution_x;target.render.resolution_y=source.render.resolution_y;target.render.resolution_percentage=100
    target.render.film_transparent=True
    target.render.image_settings.file_format='PNG';target.render.image_settings.color_mode='RGBA'
    target.view_settings.view_transform=source.view_settings.view_transform
    target.view_settings.look=source.view_settings.look
    target.view_settings.exposure=source.view_settings.exposure;target.view_settings.gamma=source.view_settings.gamma
    target.world=source.world.copy()
    before=[]
    for obj in source.objects:
        copy=obj.copy();copy.name=f'HYCoat_{pet}_{RUN}_{obj.type}'
        if obj.type in ('CAMERA','LIGHT'):copy.data=obj.data.copy()
        target.collection.objects.link(copy)
        if obj==source.camera:target.camera=copy
        if obj.type=='MESH':
            before.append(dict(object=obj.name,mesh=obj.data.name,vertices=len(obj.data.vertices),faces=len(obj.data.polygons),materials=[slot.material.name for slot in obj.material_slots]))
            for slot in copy.material_slots:
                original=slot.material
                slot.link='OBJECT'
                slot.material=build_material(original,pet)
    target['geometry_modified']=False;target['source_scene']=source.name
    audit[pet]=before
    print(json.dumps({'scene':target.name,'source_unchanged':before,'geometry_shared_readonly':True}))


def apply_tabby_texture():
    scene=bpy.data.scenes[TARGET_SCENES['naitang']]
    obj=next(o for o in scene.objects if o.type=='MESH')
    material=obj.material_slots[0].material;tree=material.node_tree
    if tree.nodes.get('HYCoat_Natural tabby fur'):raise RuntimeError('Tabby texture already applied')
    shader=next(n for n in tree.nodes if n.bl_idname=='ShaderNodeBsdfPrincipled')
    rejected=tree.nodes['HYCoat_Ginger stripe pigment']
    original=rejected.inputs[1].links[0].from_socket
    # Extend the rear mask to the back of the head. The independent facing mask
    # still excludes forward-facing cheeks, eyes and muzzle.
    tree.nodes['HYCoat_Rear depth mask'].inputs['From Max'].default_value=.85
    mask=tree.nodes['HYCoat_Rear-only blend'].outputs[0]
    xyz=tree.nodes['HYCoat_Local glTF axes']
    coords=node(tree,'ShaderNodeCombineXYZ','Back fur projection',-700,850)
    tree.links.new(math_node(tree,'ADD',xyz.outputs['X'],-.07,'Spine alignment'),coords.inputs['X'])
    tree.links.new(xyz.outputs['Y'],coords.inputs['Y'])
    color_image=bpy.data.images.load(str(OUT/'tabby-fur-albedo.png'),check_existing=False)
    color_image.name=f'HY_Naitang_Albedo_{RUN}';color_image.colorspace_settings.name='sRGB';color_image.pack()
    texture=node(tree,'ShaderNodeTexImage','Natural tabby fur',-430,850)
    texture.image=color_image;texture.extension='EXTEND'
    tree.links.new(coords.outputs[0],texture.inputs['Vector'])
    blend=node(tree,'ShaderNodeMixRGB','Natural rear coat',-50,650)
    blend.blend_type='MIX'
    tree.links.new(math_node(tree,'MULTIPLY',mask,.55,'Natural coat strength'),blend.inputs[0])
    tree.links.new(original,blend.inputs[1]);tree.links.new(texture.outputs['Color'],blend.inputs[2])
    tree.links.new(blend.outputs[0],shader.inputs['Base Color'])
    detail_image=bpy.data.images.load(str(OUT/'tabby-fur-detail.png'),check_existing=False)
    detail_image.name=f'HY_Naitang_FurDetail_{RUN}';detail_image.colorspace_settings.name='Non-Color';detail_image.pack()
    detail=node(tree,'ShaderNodeTexImage','Rear fur microdetail',-430,450)
    detail.image=detail_image;detail.extension='EXTEND';tree.links.new(coords.outputs[0],detail.inputs['Vector'])
    bump=tree.nodes['HYCoat_Rear coat micro-normal']
    tree.links.new(detail.outputs['Color'],bump.inputs['Height'])
    bump.inputs['Distance'].default_value=.0007
    material['astro_material_study']='rear-image-texture-v2'
    print('Applied packed natural tabby texture; regular procedural stripes disconnected')


def fix_side_projection():
    scene=bpy.data.scenes[TARGET_SCENES['naitang']]
    material=next(o for o in scene.objects if o.type=='MESH').material_slots[0].material
    tree=material.node_tree
    if tree.nodes.get('HYCoat_Side fur projection'):raise RuntimeError('Side projection already applied')
    xyz=tree.nodes['HYCoat_Local glTF axes'];normal=tree.nodes['HYCoat_Facing direction']
    nx=math_node(tree,'ABSOLUTE',normal.outputs['X'],0,'Side normal amount')
    nz=math_node(tree,'ABSOLUTE',normal.outputs['Z'],0,'Back normal amount')
    total=math_node(tree,'ADD',math_node(tree,'ADD',nx,nz,'Horizontal normal total'),.0001,'Projection denominator')
    weight=math_node(tree,'DIVIDE',nx,total,'Side projection weight')
    sign=range_node(tree,normal.outputs['X'],-.1,.1,-1,1,'Side orientation')
    side=node(tree,'ShaderNodeCombineXYZ','Side fur projection',-650,1050)
    span=math_node(tree,'MULTIPLY',math_node(tree,'MULTIPLY',xyz.outputs['Z'],sign,'Spine outward direction'),.5,'Half fur span')
    tree.links.new(math_node(tree,'ADD',span,.5,'Side texture U'),side.inputs['X']);tree.links.new(xyz.outputs['Y'],side.inputs['Y'])
    for source_name, label, destination in [
        ('Natural tabby fur','Side tabby fur',tree.nodes['HYCoat_Natural rear coat'].inputs[2]),
        ('Rear fur microdetail','Side fur microdetail',tree.nodes['HYCoat_Rear coat micro-normal'].inputs['Height']),
    ]:
        back=tree.nodes['HYCoat_'+source_name]
        texture=node(tree,'ShaderNodeTexImage',label,-400,1050)
        texture.image=back.image;texture.extension='EXTEND';tree.links.new(side.outputs[0],texture.inputs['Vector'])
        blend=node(tree,'ShaderNodeMixRGB',label+' blend',0,1000)
        tree.links.new(weight,blend.inputs[0]);tree.links.new(back.outputs['Color'],blend.inputs[1]);tree.links.new(texture.outputs['Color'],blend.inputs[2])
        tree.links.new(blend.outputs[0],destination)
    material['astro_material_study']='rear-and-side-image-texture-v3'
    print('Added normal-weighted side/back projections to prevent flank stretching')


def view(pet, name):
    target=bpy.data.scenes[TARGET_SCENES[pet]];bpy.context.window.scene=target
    obj=next(o for o in target.objects if o.type=='MESH')
    points=[obj.matrix_world@Vector(p) for p in obj.bound_box]
    low=Vector(tuple(min(p[i] for p in points) for i in range(3)));high=Vector(tuple(max(p[i] for p in points) for i in range(3)))
    center=(low+high)/2;size=max(high-low)
    direction={'front':(0,-3,.32),'three-quarter':(1.55,-3,.45),'side':(3,0,.32),'back':(0,3,.32)}[name]
    target.camera.location=center+Vector(direction)*size
    target.camera.rotation_euler=(center-target.camera.location).to_track_quat('-Z','Y').to_euler()
    return target


def render(pet, name):
    scene=view(pet,name)
    scene.render.filepath=str(OUT/f'{pet}-{name}-coat.png')
    bpy.ops.render.render(write_still=True,scene=scene.name)
    print('Rendered '+scene.render.filepath)


def verify_and_save():
    if set(audit)!=set(SOURCE_SCENES):raise RuntimeError('Missing source audit; refusing to claim preservation')
    for pet,entries in audit.items():
        for before in entries:
            obj=bpy.data.objects[before['object']]
            after=dict(object=obj.name,mesh=obj.data.name,vertices=len(obj.data.vertices),faces=len(obj.data.polygons),materials=[slot.material.name for slot in obj.material_slots])
            if after!=before:raise RuntimeError('Original model/material binding changed')
        source_meshes=[o for o in bpy.data.scenes[SOURCE_SCENES[pet]].objects if o.type=='MESH']
        targets=[o for o in bpy.data.scenes[TARGET_SCENES[pet]].objects if o.type=='MESH']
        for target in targets:
            source=next((o for o in source_meshes if o.data==target.data),None)
            if source is None:raise RuntimeError('Study unexpectedly changed mesh data')
            for original,edited in zip(source.material_slots,target.material_slots):
                if edited.link!='OBJECT' or original.material==edited.material or original.material.node_tree==edited.material.node_tree:
                    raise RuntimeError('Material overrides are not isolated from the original')
    (OUT/'source-preservation.json').write_text(json.dumps(audit,indent=2)+'\n')
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'hunyuan-material-study.blend'),copy=True,compress=True)
    print('Original model/material bindings verified; working copy saved')
