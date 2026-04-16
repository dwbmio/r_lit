class_name TexPackerLoader
## Loads tex_packer JSON atlas data and creates meshes or AtlasTextures.
##
## Usage:
##   var atlas = TexPackerLoader.load_atlas("res://atlas.json")
##   var mesh = atlas.get_mesh("sprite_name.png")
##   var tex = atlas.get_atlas_texture("sprite_name.png")

## Parsed atlas data
var atlas_texture: Texture2D
var frames: Dictionary = {}      # name -> frame data dict
var animations: Dictionary = {}  # name -> [frame_names]
var atlas_size: Vector2i

## Load a tex_packer JSON (hash format) or .tpsheet file.
static func load_atlas(path: String) -> TexPackerLoader:
	var loader = TexPackerLoader.new()
	var file = FileAccess.open(path, FileAccess.READ)
	if not file:
		push_error("TexPackerLoader: Cannot open %s" % path)
		return loader

	var json = JSON.new()
	var err = json.parse(file.get_as_text())
	file.close()
	if err != OK:
		push_error("TexPackerLoader: JSON parse error in %s" % path)
		return loader

	var data: Dictionary = json.data
	var base_dir = path.get_base_dir()

	# Detect format
	if data.has("textures"):
		# .tpsheet format
		var tex_info = data["textures"][0]
		var image_path = base_dir.path_join(tex_info["image"])
		loader.atlas_texture = load(image_path)
		loader.atlas_size = Vector2i(tex_info["size"]["w"], tex_info["size"]["h"])
		for sprite in tex_info["sprites"]:
			loader.frames[sprite["filename"]] = sprite
	elif data.has("frames"):
		# JSON hash format
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
		push_error("TexPackerLoader: sprite '%s' not found" % sprite_name)
		return ArrayMesh.new()

	var f = frames[sprite_name]

	# If this is an alias, redirect
	if f.has("alias"):
		return get_mesh(f["alias"])

	var mesh = ArrayMesh.new()
	var arrays = []
	arrays.resize(Mesh.ARRAY_MAX)

	var has_polygon = f.has("vertices") and f.has("verticesUV") and f.has("triangles")

	if has_polygon:
		# Polygon mesh — reduced overdraw
		var verts = PackedVector2Array()
		var uvs = PackedVector2Array()
		var indices = PackedInt32Array()

		for v in f["vertices"]:
			verts.append(Vector2(v[0], v[1]))

		# Convert atlas-space UVs to normalized 0-1 range
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
		# Fallback: full quad
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

## Get a classic AtlasTexture for a sprite (no polygon mesh, just rect).
func get_atlas_texture(sprite_name: String) -> AtlasTexture:
	if not frames.has(sprite_name):
		push_error("TexPackerLoader: sprite '%s' not found" % sprite_name)
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

## Get all sprite names.
func get_sprite_names() -> Array[String]:
	return frames.keys()

## Get animation frame names.
func get_animation_frames(anim_name: String) -> Array:
	if animations.has(anim_name):
		return animations[anim_name]
	return []

## Get all animation names.
func get_animation_names() -> Array[String]:
	return animations.keys()
