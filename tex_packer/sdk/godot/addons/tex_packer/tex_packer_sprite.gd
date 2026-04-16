@tool
class_name TexPackerSprite
extends MeshInstance2D
## A sprite node that renders using polygon mesh from tex_packer atlas.
## Reduces GPU overdraw by only rendering non-transparent pixels.
##
## Usage:
##   1. Set atlas_path to your JSON/tpsheet file
##   2. Set sprite_name to the sprite you want to display
##   3. The node auto-creates the mesh and applies the atlas texture

@export_file("*.json", "*.tpsheet") var atlas_path: String = "":
	set(v):
		atlas_path = v
		_reload()

@export var sprite_name: String = "":
	set(v):
		sprite_name = v
		_reload()

## Flip horizontally
@export var flip_h: bool = false:
	set(v):
		flip_h = v
		scale.x = -abs(scale.x) if flip_h else abs(scale.x)

## Flip vertically
@export var flip_v: bool = false:
	set(v):
		flip_v = v
		scale.y = -abs(scale.y) if flip_v else abs(scale.y)

## Center the sprite on its origin
@export var centered: bool = true:
	set(v):
		centered = v
		_reload()

var _loader: TexPackerLoader = null

func _ready() -> void:
	_reload()

func _reload() -> void:
	if atlas_path.is_empty() or sprite_name.is_empty():
		return

	if _loader == null or _loader.atlas_texture == null:
		_loader = TexPackerLoader.load_atlas(atlas_path)

	if not _loader.frames.has(sprite_name):
		push_warning("TexPackerSprite: sprite '%s' not found in atlas" % sprite_name)
		return

	mesh = _loader.get_mesh(sprite_name)
	texture = _loader.atlas_texture

	# Center the mesh if requested
	if centered:
		var f = _loader.frames[sprite_name]
		if f.has("alias"):
			f = _loader.frames[f["alias"]]
		var frame = f["frame"]
		position_offset(-float(frame["w"]) / 2.0, -float(frame["h"]) / 2.0)

func position_offset(ox: float, oy: float) -> void:
	# Offset all mesh vertices to center the sprite
	if mesh == null or mesh.get_surface_count() == 0:
		return
	var arrays = mesh.surface_get_arrays(0)
	var verts: PackedVector2Array = arrays[Mesh.ARRAY_VERTEX]
	var new_verts = PackedVector2Array()
	for v in verts:
		new_verts.append(Vector2(v.x + ox, v.y + oy))
	arrays[Mesh.ARRAY_VERTEX] = new_verts

	var new_mesh = ArrayMesh.new()
	new_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	mesh = new_mesh

## Change which sprite to display (runtime).
func set_sprite(name: String) -> void:
	sprite_name = name

## Get the loader for advanced usage.
func get_loader() -> TexPackerLoader:
	if _loader == null:
		_loader = TexPackerLoader.load_atlas(atlas_path)
	return _loader
