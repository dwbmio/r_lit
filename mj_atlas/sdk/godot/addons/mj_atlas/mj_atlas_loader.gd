class_name MJAtlasLoader
## MJAtlas runtime loader — parses mj_atlas JSON hash schema into meshes / AtlasTextures.
##
## ⚠️ SCHEMA DEPENDENCY ⚠️
## This loader consumes the exact JSON schema produced by the `mj_atlas` CLI
## (r_lit/mj_atlas). Any schema change on the producer side must be mirrored here
## AND in the Rust loader (rlib/gd-main-extension/src/fac/mj_atlas.rs).
##
## Schema (hash format):
##   {
##     "frames": {
##       "<name>": {
##         "frame": { x, y, w, h },                  # packed rect in atlas
##         "rotated": bool, "trimmed": bool,
##         "spriteSourceSize": { x, y, w, h },       # original sprite rect
##         "sourceSize": { w, h },                   # original image size
##         "vertices":    [[x,y], ...]?,             # --polygon only (local px)
##         "verticesUV":  [[u,v], ...]?,             # --polygon only (atlas px)
##         "triangles":   [[i,i,i], ...]?,           # --polygon only
##         "alias": "<other_name>"?                  # duplicate pointer
##       }
##     },
##     "animations": { "<anim>": ["frame_a", ...] },
##     "meta": { "app": "mj_atlas", "image", "size": {w,h}, ... }
##   }
##
## Usage:
##   var atlas = MJAtlasLoader.load_atlas("res://atlas_balls.json")
##   var mesh = atlas.get_mesh("0.png")            # polygon mesh (MeshInstance2D)
##   var tex  = atlas.get_atlas_texture("0.png")   # AtlasTexture (TextureButton/TextureRect)

## Parsed atlas data
var atlas_texture: Texture2D
var frames: Dictionary = {}      # name -> frame data dict
var animations: Dictionary = {}  # name -> [frame_names]
var atlas_size: Vector2i

## Load an mj_atlas JSON (hash format) or .tpsheet file.
static func load_atlas(path: String) -> MJAtlasLoader:
	var loader = MJAtlasLoader.new()
	var file = FileAccess.open(path, FileAccess.READ)
	if not file:
		push_error("MJAtlasLoader: Cannot open %s" % path)
		return loader

	var json = JSON.new()
	var err = json.parse(file.get_as_text())
	file.close()
	if err != OK:
		push_error("MJAtlasLoader: JSON parse error in %s" % path)
		return loader

	var data: Dictionary = json.data
	var base_dir = path.get_base_dir()

	if data.has("textures"):
		var tex_info = data["textures"][0]
		var image_path = base_dir.path_join(tex_info["image"])
		loader.atlas_texture = load(image_path)
		loader.atlas_size = Vector2i(tex_info["size"]["w"], tex_info["size"]["h"])
		for sprite in tex_info["sprites"]:
			loader.frames[sprite["filename"]] = sprite
	elif data.has("frames"):
		var image_path = base_dir.path_join(data["meta"]["image"])
		loader.atlas_texture = load(image_path)
		var sz = data["meta"]["size"]
		loader.atlas_size = Vector2i(sz["w"], sz["h"])
		if data["frames"] is Dictionary:
			loader.frames = data["frames"]
		elif data["frames"] is Array:
			for f in data["frames"]:
				loader.frames[f["filename"]] = f

	if data.has("animations"):
		loader.animations = data["animations"]

	return loader

## Get a polygon ArrayMesh for a sprite (uses mesh data if available, falls back to quad).
## The mesh has UVs mapped to the atlas texture.
func get_mesh(sprite_name: String) -> ArrayMesh:
	if not frames.has(sprite_name):
		push_error("MJAtlasLoader: sprite '%s' not found" % sprite_name)
		return ArrayMesh.new()

	var f = frames[sprite_name]

	if f.has("alias"):
		return get_mesh(f["alias"])

	var mesh = ArrayMesh.new()
	var arrays = []
	arrays.resize(Mesh.ARRAY_MAX)

	var has_polygon = f.has("vertices") and f.has("verticesUV") and f.has("triangles")

	if has_polygon:
		var verts = PackedVector2Array()
		var uvs = PackedVector2Array()
		var indices = PackedInt32Array()

		for v in f["vertices"]:
			verts.append(Vector2(v[0], v[1]))

		for uv in f["verticesUV"]:
			uvs.append(Vector2(uv[0] / atlas_size.x, uv[1] / atlas_size.y))

		for tri in f["triangles"]:
			indices.append(tri[0])
			indices.append(tri[1])
			indices.append(tri[2])

		arrays[Mesh.ARRAY_VERTEX] = verts
		arrays[Mesh.ARRAY_TEX_UV] = uvs
		arrays[Mesh.ARRAY_INDEX] = indices
	else:
		var frame = f["frame"]
		var x = float(frame["x"])
		var y = float(frame["y"])
		var w = float(frame["w"])
		var h = float(frame["h"])

		var verts = PackedVector2Array([
			Vector2(0, 0), Vector2(w, 0), Vector2(w, h), Vector2(0, h)
		])
		var uvs = PackedVector2Array([
			Vector2(x / atlas_size.x, y / atlas_size.y),
			Vector2((x + w) / atlas_size.x, y / atlas_size.y),
			Vector2((x + w) / atlas_size.x, (y + h) / atlas_size.y),
			Vector2(x / atlas_size.x, (y + h) / atlas_size.y),
		])
		var indices = PackedInt32Array([0, 1, 2, 0, 2, 3])

		arrays[Mesh.ARRAY_VERTEX] = verts
		arrays[Mesh.ARRAY_TEX_UV] = uvs
		arrays[Mesh.ARRAY_INDEX] = indices

	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh

## Get a classic AtlasTexture for a sprite (rect only, no polygon).
## Use this for TextureButton / TextureRect cases where MeshInstance2D is not applicable.
func get_atlas_texture(sprite_name: String) -> AtlasTexture:
	if not frames.has(sprite_name):
		push_error("MJAtlasLoader: sprite '%s' not found" % sprite_name)
		return AtlasTexture.new()

	var f = frames[sprite_name]
	if f.has("alias"):
		return get_atlas_texture(f["alias"])

	var frame = f["frame"]
	var at = AtlasTexture.new()
	at.atlas = atlas_texture
	at.region = Rect2(frame["x"], frame["y"], frame["w"], frame["h"])

	if f.has("sourceSize") and f.has("spriteSourceSize"):
		var src = f["spriteSourceSize"]
		var orig = f["sourceSize"]
		at.margin = Rect2(
			src["x"], src["y"],
			orig["w"] - frame["w"],
			orig["h"] - frame["h"]
		)

	return at

func get_sprite_names() -> Array[String]:
	return frames.keys()

func get_animation_frames(anim_name: String) -> Array:
	if animations.has(anim_name):
		return animations[anim_name]
	return []

func get_animation_names() -> Array[String]:
	return animations.keys()
