@tool
extends EditorPlugin

func _enter_tree() -> void:
	add_custom_type(
		"TexPackerSprite",
		"MeshInstance2D",
		preload("tex_packer_sprite.gd"),
		preload("res://addons/tex_packer/icon.svg") if FileAccess.file_exists("res://addons/tex_packer/icon.svg") else null
	)
	add_custom_type(
		"TexPackerAnimPlayer",
		"Node",
		preload("tex_packer_anim.gd"),
		null
	)

func _exit_tree() -> void:
	remove_custom_type("TexPackerSprite")
	remove_custom_type("TexPackerAnimPlayer")
