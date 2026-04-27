"""
Blender headless script: render "TUNG" bubble-style 3D text.
Usage: blender -b -P blender_tung.py
Output: tung_render.png (transparent background)
"""
import bpy
import math

# --------------- cleanup scene ---------------
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete()
for m in list(bpy.data.materials):
    bpy.data.materials.remove(m)
for c in list(bpy.data.collections):
    if c.name != "Collection":
        bpy.data.collections.remove(c)

# --------------- settings ---------------
LETTERS = "TUNG"
# Per-letter colors matching the screenshot: pink, green, blue-purple, yellow-orange
COLORS = [
    (1.0, 0.35, 0.55, 1.0),   # T - pink/magenta
    (0.2, 0.85, 0.55, 1.0),   # U - green/teal
    (0.45, 0.4, 0.9, 1.0),    # N - blue/purple
    (1.0, 0.75, 0.15, 1.0),   # G - yellow/orange
]
FONT_SIZE = 1.0
EXTRUDE = 0.25
BEVEL_DEPTH = 0.06
BEVEL_RESOLUTION = 4
LETTER_SPACING = 1.15  # spacing multiplier

# --------------- create per-letter 3D text ---------------
all_objects = []
x_cursor = 0.0

for i, ch in enumerate(LETTERS):
    # Add text object
    bpy.ops.object.text_add(location=(0, 0, 0))
    obj = bpy.context.active_object
    obj.name = f"Letter_{ch}"
    obj.data.body = ch
    obj.data.size = FONT_SIZE
    obj.data.extrude = EXTRUDE
    obj.data.bevel_depth = BEVEL_DEPTH
    obj.data.bevel_resolution = BEVEL_RESOLUTION
    obj.data.align_x = 'CENTER'
    obj.data.align_y = 'CENTER'

    # Try to use a bold/rounded font if available
    # Blender ships with Bfont by default; we'll use it
    # On server, can install fonts and point here

    # Convert to mesh so we can assign material properly
    bpy.ops.object.convert(target='MESH')

    # Create bubble/glossy material
    mat = bpy.data.materials.new(name=f"Mat_{ch}")
    mat.use_nodes = True
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()

    # Principled BSDF for glossy bubble look
    output = nodes.new('ShaderNodeOutputMaterial')
    principled = nodes.new('ShaderNodeBsdfPrincipled')
    principled.inputs['Base Color'].default_value = COLORS[i]
    principled.inputs['Metallic'].default_value = 0.0
    principled.inputs['Roughness'].default_value = 0.15   # very glossy
    principled.inputs['IOR'].default_value = 1.45
    # Subsurface for soft translucent bubble feel
    principled.inputs['Subsurface Weight'].default_value = 0.3
    principled.inputs['Subsurface Radius'].default_value = (0.5, 0.5, 0.5)
    principled.inputs['Coat Weight'].default_value = 0.8   # clear coat
    principled.inputs['Coat Roughness'].default_value = 0.05
    # Emission for slight self-illumination (cartoon feel)
    principled.inputs['Emission Color'].default_value = COLORS[i]
    principled.inputs['Emission Strength'].default_value = 0.15

    links.new(principled.outputs['BSDF'], output.inputs['Surface'])

    output.location = (300, 0)
    principled.location = (0, 0)

    obj.data.materials.append(mat)

    # Position letter
    # Measure bounding box width
    bbox = obj.bound_box
    min_x = min(v[0] for v in bbox)
    max_x = max(v[0] for v in bbox)
    width = max_x - min_x

    obj.location.x = x_cursor + width / 2
    x_cursor += width * LETTER_SPACING

    all_objects.append(obj)

# --------------- center all letters ---------------
total_width = x_cursor
offset = total_width / 2
for obj in all_objects:
    obj.location.x -= offset

# --------------- white outline (wireframe + solidify trick) ---------------
# Add a slightly larger duplicate behind for white outline effect
for i, src in enumerate(all_objects):
    bpy.ops.object.select_all(action='DESELECT')
    src.select_set(True)
    bpy.context.view_layer.objects.active = src
    bpy.ops.object.duplicate()
    outline = bpy.context.active_object
    outline.name = f"Outline_{LETTERS[i]}"

    # Scale up slightly for outline
    outline.scale = (1.05, 1.05, 1.05)
    outline.location.z -= 0.02  # push slightly back

    # White material
    mat_w = bpy.data.materials.new(name=f"Mat_Outline_{LETTERS[i]}")
    mat_w.use_nodes = True
    n = mat_w.node_tree.nodes
    l = mat_w.node_tree.links
    n.clear()
    out_n = n.new('ShaderNodeOutputMaterial')
    emit = n.new('ShaderNodeEmission')
    emit.inputs['Color'].default_value = (1, 1, 1, 1)
    emit.inputs['Strength'].default_value = 1.0
    l.new(emit.outputs['Emission'], out_n.inputs['Surface'])
    out_n.location = (300, 0)
    emit.location = (0, 0)

    outline.data.materials.clear()
    outline.data.materials.append(mat_w)

# --------------- lighting ---------------
# Key light (warm, from top-right)
bpy.ops.object.light_add(type='AREA', location=(2, -2, 3))
key = bpy.context.active_object
key.data.energy = 150
key.data.size = 3
key.data.color = (1.0, 0.95, 0.9)
key.rotation_euler = (math.radians(45), 0, math.radians(30))

# Fill light (cool, from left)
bpy.ops.object.light_add(type='AREA', location=(-3, -1, 2))
fill = bpy.context.active_object
fill.data.energy = 80
fill.data.size = 4
fill.data.color = (0.85, 0.9, 1.0)
fill.rotation_euler = (math.radians(50), 0, math.radians(-40))

# Rim light from behind
bpy.ops.object.light_add(type='AREA', location=(0, 2, 2))
rim = bpy.context.active_object
rim.data.energy = 60
rim.data.size = 5
rim.data.color = (1.0, 1.0, 1.0)
rim.rotation_euler = (math.radians(-30), 0, 0)

# --------------- camera ---------------
bpy.ops.object.camera_add(location=(0, -4, 0.3))
cam = bpy.context.active_object
cam.rotation_euler = (math.radians(85), 0, 0)
cam.data.lens = 85
bpy.context.scene.camera = cam

# --------------- render settings ---------------
scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.device = 'CPU'       # safe fallback; GPU if available
scene.cycles.samples = 128
scene.cycles.use_denoising = True
scene.render.resolution_x = 800
scene.render.resolution_y = 400
scene.render.resolution_percentage = 100
scene.render.film_transparent = True  # transparent background
scene.render.image_settings.file_format = 'PNG'
scene.render.image_settings.color_mode = 'RGBA'
scene.render.filepath = '//tung_render.png'

# Try to use GPU if available
prefs = bpy.context.preferences.addons.get('cycles')
if prefs:
    cprefs = prefs.preferences
    # Try CUDA, then OPTIX, then Metal, fallback CPU
    for compute in ['OPTIX', 'CUDA', 'METAL', 'HIP']:
        try:
            cprefs.compute_device_type = compute
            cprefs.get_devices()
            devices = cprefs.devices
            if devices:
                for d in devices:
                    d.use = True
                scene.cycles.device = 'GPU'
                print(f"Using GPU compute: {compute}")
                break
        except:
            continue

# --------------- world (subtle gradient for ambient) ---------------
world = bpy.data.worlds.new("World")
scene.world = world
world.use_nodes = True
wn = world.node_tree.nodes
wl = world.node_tree.links
wn.clear()
bg = wn.new('ShaderNodeBackground')
bg.inputs['Color'].default_value = (0.02, 0.02, 0.04, 1.0)
bg.inputs['Strength'].default_value = 0.5
out_w = wn.new('ShaderNodeOutputWorld')
wl.new(bg.outputs['Background'], out_w.inputs['Surface'])

# --------------- render! ---------------
print("Starting render...")
bpy.ops.render.render(write_still=True)
print(f"Done! Output: {scene.render.filepath}")
