@tool
extends EditorPlugin
## MJAtlas Godot addon — registers custom types for atlas-backed sprites.
##
## DEPENDS: mj_atlas CLI JSON hash schema
##   { frames: { name: { frame, vertices, verticesUV, triangles, alias? } },
##     animations: { name: [frame_name] },
##     meta: { image, size } }
##
## Produced by: r_lit/mj_atlas `pack --polygon` → atlas.png + atlas.json
## Schema change MUST be mirrored in mj_atlas_loader.gd (and the Rust loader).

func _enter_tree() -> void:
	add_custom_type(
		"MJAtlasSprite",
		"MeshInstance2D",
		preload("mj_atlas_sprite.gd"),
		preload("res://addons/mj_atlas/icon.svg") if FileAccess.file_exists("res://addons/mj_atlas/icon.svg") else null
	)
	add_custom_type(
		"MJAtlasAnimPlayer",
		"Node",
		preload("mj_atlas_anim.gd"),
		null
	)

func _exit_tree() -> void:
	remove_custom_type("MJAtlasSprite")
	remove_custom_type("MJAtlasAnimPlayer")
