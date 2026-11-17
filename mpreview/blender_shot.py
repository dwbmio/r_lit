"""mpreview Blender side: render a GLB with the shared cam.json preset.
Usage:
  blender --background --python blender_shot.py -- model.glb out.png [cam.json] [--action Idle] [--frame -1] [--size 800x600] [--samples 64]
Compares 1:1 with the Godot mpreview viewer when cam.json matches.
"""
import bpy
import json
import math
import sys
from mathutils import Vector as V

argv = sys.argv[sys.argv.index("--") + 1:]
src, out = argv[0], argv[1]
cam_json = argv[2] if len(argv) > 2 and not argv[2].startswith("--") else "cam.json"
action_name, frame, size, samples = "", -1, "800x600", 64
i = 0
while i < len(argv):
    if argv[i] == "--action" and i + 1 < len(argv):
        action_name = argv[i + 1]
    elif argv[i] == "--frame" and i + 1 < len(argv):
        frame = int(argv[i + 1])
    elif argv[i] == "--size" and i + 1 < len(argv):
        size = argv[i + 1]
    elif argv[i] == "--samples" and i + 1 < len(argv):
        samples = int(argv[i + 1])
    i += 1

with open(cam_json) as f:
    cam = json.load(f)
pos, tgt, fov = V(cam["pos"]), V(cam["target"]), float(cam.get("fov", 40.0))

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)

if action_name:
    for o in bpy.data.objects:
        if o.type == 'ARMATURE' and action_name in bpy.data.actions:
            if not o.animation_data:
                o.animation_data_create()
            o.animation_data.action = bpy.data.actions[action_name]
            break
acts = [a for a in bpy.data.actions]
if frame >= 0:
    bpy.context.scene.frame_set(frame)
elif acts:
    a = acts[0]
    bpy.context.scene.frame_set(int((a.frame_range[0] + a.frame_range[1]) / 2))

bpy.ops.object.light_add(type='SUN', location=(float(pos.x) + 2, float(pos.y) - 2, float(pos.z) + 4))
sun = bpy.context.active_object
sun.data.energy = 3.0
bpy.ops.object.camera_add(location=(float(pos.x), float(pos.y), float(pos.z)))
cc = bpy.context.active_object
bpy.context.scene.camera = cc
cc.rotation_euler = (tgt - cc.location).to_track_quat('-Z', 'Y').to_euler()
# vertical fov -> lens (sensor fit vertical, 32mm height -> use 36mm width basis like Blender default? use fov directly)
cc.data.lens = 18.0 / math.tan(math.radians(fov) / 2.0)

sc = bpy.context.scene
sc.render.engine = 'CYCLES'
sc.cycles.device = 'CPU'
sc.cycles.samples = samples
w, h = size.split('x')
sc.render.resolution_x, sc.render.resolution_y = int(w), int(h)
sc.render.film_transparent = False
sc.render.filepath = out
if not sc.world:
    sc.world = bpy.data.worlds.new("World")
try:
    sc.world.use_nodes = True
except Exception:
    pass
try:
    bg = sc.world.node_tree.nodes.get('Background')
    if bg:
        bg.inputs[0].default_value = (0.25, 0.27, 0.3, 1.0)
        bg.inputs[1].default_value = 1.0
except Exception:
    pass
bpy.ops.render.render(write_still=True)
print("MPREVIEW-BLENDER shot:", out)
