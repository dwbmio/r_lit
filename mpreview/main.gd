extends Control
## mpreview: single-model interactive previewer.
## Usage:
##   mpreview --model <path.glb> [--anim Idle] [--rot-x 0] [--cam cam.json]
##            [--shot out.png] [--shot-frames 90] [--compare blender.png] [--phone 405x720]
## Keys: Space play/pause, S screenshot, C compare toggle, Q quit, R reset view.

var _yaw: Node3D
var _pitch: Node3D
var _cam: Camera3D
var _viewport: SubViewport
var _model_root: Node3D
var _player: AnimationPlayer
var _rest_pose := {}
var _anims: PackedStringArray = []
var _info_label: Label
var _status_label: Label
var _loop_on := true
var _anim_list: ItemList
var _play_btn: Button
var _speed: HSlider
var _compare_rect: TextureRect
var _compare_title: Label
var _ref_tex: Texture2D
var _shot_tex: Texture2D
var _showing_ref := true
var _compare_path := ""
var _shot_path := ""
var _shot_frames := 90
var _frame := 0
var _auto_shot := false
var _model_path := ""
var _rot_x := 0.0

const DEFAULT_CAM := {"pos": [2.2, -2.8, 1.6], "target": [0.0, 0.0, 0.9], "fov": 40.0}

func _ready() -> void:
	_parse_args()
	_build_ui()
	_load_cam()
	if _model_path != "":
		_load_model(_model_path)
	if _compare_path != "":
		_set_compare(_compare_path)
	_auto_shot = _shot_path != ""

func _parse_args() -> void:
	var args := OS.get_cmdline_user_args()
	var i := 0
	while i < args.size():
		match args[i]:
			"--model":
				i += 1
				if i < args.size():
					_model_path = args[i]
			"--anim":
				i += 1
				if i < args.size():
					_anim_path_default = args[i]
			"--rot-x":
				i += 1
				if i < args.size():
					_rot_x = float(args[i])
			"--cam":
				i += 1
				if i < args.size():
					_cam_path = args[i]
			"--shot":
				i += 1
				if i < args.size():
					_shot_path = args[i]
			"--shot-frames":
				i += 1
				if i < args.size():
					_shot_frames = int(args[i])
			"--compare":
				i += 1
				if i < args.size():
					_compare_path = args[i]
			"--phone":
				i += 1
				if i < args.size():
					_phone_size = args[i]
			"--yaw-deg":
				i += 1
				if i < args.size():
					_orbit_yaw_deg = float(args[i])
			"--pitch-deg":
				i += 1
				if i < args.size():
					_orbit_pitch_deg = float(args[i])
		i += 1

var _anim_path_default := ""
var _cam_path := ""
var _phone_size := "405x720"
var _orbit_yaw_deg := 0.0
var _orbit_pitch_deg := 0.0

func _build_ui() -> void:
	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)
	margin.set_anchors_preset(Control.PRESET_FULL_RECT)
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 12)
	margin.add_child(hbox)
	# --- left: phone frame ---
	var phone := _parse_phone()
	var bezel := PanelContainer.new()
	var sb := StyleBoxFlat.new()
	sb.bg_color = Color(0.08, 0.08, 0.1)
	sb.set_corner_radius_all(28)
	sb.content_margin_left = 10
	sb.content_margin_right = 10
	sb.content_margin_top = 34
	sb.content_margin_bottom = 10
	bezel.add_theme_stylebox_override("panel", sb)
	hbox.add_child(bezel)
	var svc := SubViewportContainer.new()
	svc.custom_minimum_size = phone
	svc.stretch = true
	bezel.add_child(svc)
	_viewport = SubViewport.new()
	_viewport.size = phone
	_viewport.own_world_3d = true
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	svc.add_child(_viewport)
	svc.gui_input.connect(_on_view_input)
	_build_world()
	# --- right: panel ---
	var panel := VBoxContainer.new()
	panel.custom_minimum_size = Vector2(300, 0)
	panel.add_theme_constant_override("separation", 8)
	hbox.add_child(panel)
	_info_label = Label.new()
	_info_label.text = "mpreview: pass --model <file.glb>"
	_info_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	panel.add_child(_info_label)
	panel.add_child(HSeparator.new())
	panel.add_child(_mk_label("Animations (double-click to play):"))
	_anim_list = ItemList.new()
	_anim_list.custom_minimum_size = Vector2(300, 260)
	_anim_list.item_activated.connect(_on_anim_activated)
	panel.add_child(_anim_list)
	_play_btn = Button.new()
	_play_btn.text = "Pause"
	_play_btn.pressed.connect(_toggle_play)
	panel.add_child(_play_btn)
	var loop_btn := Button.new()
	loop_btn.text = "Loop: ON"
	loop_btn.pressed.connect(_toggle_loop.bind(loop_btn))
	panel.add_child(loop_btn)
	_status_label = _mk_label("anim: -")
	panel.add_child(_status_label)
	panel.add_child(_mk_label("Speed:"))
	_speed = HSlider.new()
	_speed.min_value = 0.1
	_speed.max_value = 2.0
	_speed.step = 0.05
	_speed.value = 1.0
	_speed.value_changed.connect(func(v: float) -> void:
		if _player:
			_player.speed_scale = v)
	panel.add_child(_speed)
	var shot_btn := Button.new()
	shot_btn.text = "Screenshot (S)"
	shot_btn.pressed.connect(func() -> void: take_shot("mpreview_shot.png"))
	panel.add_child(shot_btn)
	var cmp_btn := Button.new()
	cmp_btn.text = "Toggle compare (C)"
	cmp_btn.pressed.connect(_toggle_compare)
	panel.add_child(cmp_btn)
	var hint := _mk_label("Drag: orbit | Wheel/+/-: zoom | Space: play | C: compare | R: reset view | Q: quit")
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	panel.add_child(hint)
	# --- far right: compare pane (expands to fill leftover window space) ---
	var cmp_panel := PanelContainer.new()
	cmp_panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var csb := StyleBoxFlat.new()
	csb.bg_color = Color(0.1, 0.1, 0.12)
	csb.set_corner_radius_all(12)
	cmp_panel.add_theme_stylebox_override("panel", csb)
	hbox.add_child(cmp_panel)
	var cmp_vbox := VBoxContainer.new()
	cmp_vbox.add_theme_constant_override("separation", 6)
	cmp_panel.add_child(cmp_vbox)
	_compare_title = _mk_label("Compare: (no reference — C toggles Godot shot)")
	cmp_vbox.add_child(_compare_title)
	_compare_rect = TextureRect.new()
	_compare_rect.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_compare_rect.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_compare_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_compare_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	cmp_vbox.add_child(_compare_rect)
	var cmp_row := HBoxContainer.new()
	cmp_row.add_theme_constant_override("separation", 6)
	cmp_vbox.add_child(cmp_row)
	var load_btn := Button.new()
	load_btn.text = "Load ref…"
	load_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	load_btn.pressed.connect(_on_load_ref_pressed)
	cmp_row.add_child(load_btn)
	var swap_btn := Button.new()
	swap_btn.text = "Godot/Ref (C)"
	swap_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	swap_btn.pressed.connect(_toggle_compare)
	cmp_row.add_child(swap_btn)

func _mk_label(t: String) -> Label:
	var l := Label.new()
	l.text = t
	return l

func _parse_phone() -> Vector2i:
	var p := _phone_size.split("x")
	if p.size() == 2 and p[0].is_valid_int() and p[1].is_valid_int():
		return Vector2i(int(p[0]), int(p[1]))
	return Vector2i(405, 720)

func _build_world() -> void:
	var world := World3D.new()
	_viewport.world_3d = world
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.25, 0.27, 0.3)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.75, 0.78, 0.85)
	env.ambient_light_energy = 0.7
	var we := WorldEnvironment.new()
	we.environment = env
	_viewport.add_child(we)
	var sun := DirectionalLight3D.new()
	sun.rotation = Vector3(-0.9, -0.6, 0.0)
	sun.light_energy = 1.2
	sun.shadow_enabled = true
	_viewport.add_child(sun)
	_yaw = Node3D.new()
	_viewport.add_child(_yaw)
	_pitch = Node3D.new()
	_yaw.add_child(_pitch)
	_cam = Camera3D.new()
	_pitch.add_child(_cam)
	_model_root = Node3D.new()
	_model_root.name = "ModelRoot"
	_viewport.add_child(_model_root)

func _load_cam() -> void:
	var cfg: Dictionary = DEFAULT_CAM.duplicate()
	if _cam_path != "" and FileAccess.file_exists(_cam_path):
		var txt := FileAccess.get_file_as_string(_cam_path)
		var j: Variant = JSON.parse_string(txt)
		if j is Dictionary:
			for k in j:
				cfg[k] = j[k]
	var pos_b := Vector3(cfg["pos"][0], cfg["pos"][1], cfg["pos"][2])
	var tgt_b := Vector3(cfg["target"][0], cfg["target"][1], cfg["target"][2])
	# cam.json is Blender-space (z-up); convert to Godot (y-up): (x,y,z)->(x,z,-y)
	var pos := Vector3(pos_b.x, pos_b.z, -pos_b.y)
	var tgt := Vector3(tgt_b.x, tgt_b.z, -tgt_b.y)
	_cam.fov = float(cfg.get("fov", 40.0))
	_reset_view_to(pos, tgt)

func _reset_view_to(pos: Vector3, tgt: Vector3) -> void:
	var off: Vector3 = pos - tgt
	var dist := off.length()
	_yaw.position = tgt
	_yaw.rotation = Vector3.ZERO
	_pitch.rotation = Vector3.ZERO
	_yaw.rotation.y = atan2(off.x, off.z)
	_pitch.rotation.x = -asin(clampf(off.y / dist, -1.0, 1.0))
	_cam.position = Vector3(0, 0, dist)
	_yaw.rotation.y += deg_to_rad(_orbit_yaw_deg)
	_pitch.rotation.x += deg_to_rad(_orbit_pitch_deg)

func _load_model(path: String) -> void:
	for c in _model_root.get_children():
		_model_root.remove_child(c)
		c.queue_free()
	_player = null
	_anims = PackedStringArray()
	_anim_list.clear()
	var abs_path := ProjectSettings.globalize_path(path) if path.begins_with("res://") else path
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	var err := doc.append_from_file(abs_path, state)
	if err != OK:
		_info_label.text = "load failed: %s (err %d)" % [path, err]
		return
	var scene: Node = doc.generate_scene(state)
	_model_root.add_child(scene)
	scene.rotation_degrees.x = _rot_x
	var meshes: Array = []
	var skels: Array = []
	_collect(scene, meshes, skels)
	_player = _find(scene, "AnimationPlayer") as AnimationPlayer
	if _player:
		_snap_rest()
		_anims = _player.get_animation_list()
		for a in _anims:
			_anim_list.add_item(a)
		var want := _anim_path_default if _anim_path_default != "" else (_anims[0] if _anims.size() > 0 else "")
		if want != "" and _player.has_animation(want):
			_player.play(want)
		_apply_loop()
	_info_label.text = _stat_text(path, scene)

func _stat_text(path: String, scene: Node) -> String:
	var meshes: Array = []
	var skels: Array = []
	_collect(scene, meshes, skels)
	var tris := 0
	var mats := {}
	for mi in meshes:
		var m: ArrayMesh = (mi as MeshInstance3D).mesh
		if m == null:
			continue
		for s in range(m.get_surface_count()):
			var arr: Array = m.surface_get_arrays(s)
			if arr[Mesh.ARRAY_INDEX] != null:
				tris += (arr[Mesh.ARRAY_INDEX] as PackedInt32Array).size() / 3
			elif arr[Mesh.ARRAY_VERTEX] != null:
				tris += (arr[Mesh.ARRAY_VERTEX] as PackedVector3Array).size() / 3
			var sm: Material = m.surface_get_material(s)
			if sm:
				mats[sm.resource_name if sm.resource_name != "" else sm.get_class()] = true
	var bones := 0
	for sk in skels:
		bones += (sk as Skeleton3D).get_bone_count()
	var kb := 0.0
	if FileAccess.file_exists(path):
		kb = FileAccess.get_size(path) / 1024.0
	var h := 0.0
	if skels.size() > 0:
		var sk := skels[0] as Skeleton3D
		var top := -INF
		var bot := INF
		for bi in range(sk.get_bone_count()):
			var g: Transform3D = sk.global_transform * sk.get_bone_global_rest(bi)
			top = maxf(top, g.origin.y)
			bot = minf(bot, g.origin.y)
		h = top - bot
	else:
		var box := AABB()
		var first := true
		for mi in meshes:
			var b: AABB = (mi as VisualInstance3D).get_aabb()
			if first:
				box = b
				first = false
			else:
				box = box.merge(b)
		h = box.size.y
	var lines := []
	lines.append("file: %s" % path.get_file())
	lines.append("size: %.1f KB" % kb)
	lines.append("meshes: %d  tris: %d" % [meshes.size(), tris])
	lines.append("bones: %d  materials: %d" % [bones, mats.size()])
	lines.append("animations: %d" % _anims.size())
	lines.append("height: %.2f m" % h)
	return "\n".join(lines)

func _collect(n: Node, meshes: Array, skels: Array) -> void:
	if n is MeshInstance3D:
		meshes.append(n)
	elif n is Skeleton3D:
		skels.append(n)
	for c in n.get_children():
		_collect(c, meshes, skels)

func _find(n: Node, cls: String) -> Node:
	if n.get_class() == cls:
		return n
	for c in n.get_children():
		var r := _find(c, cls)
		if r:
			return r
	return null

func _snap_rest() -> void:
	_rest_pose.clear()
	var sk := _find_skeleton()
	if sk == null:
		return
	for bi in range(sk.get_bone_count()):
		_rest_pose[bi] = [sk.get_bone_pose_position(bi), sk.get_bone_pose_rotation(bi), sk.get_bone_pose_scale(bi)]

func _reset_pose() -> void:
	var sk := _find_skeleton()
	if sk == null or _rest_pose.is_empty():
		return
	if _player:
		_player.stop()
	for bi in _rest_pose:
		sk.set_bone_pose_position(bi, _rest_pose[bi][0])
		sk.set_bone_pose_rotation(bi, _rest_pose[bi][1])
		sk.set_bone_pose_scale(bi, _rest_pose[bi][2])

func _find_skeleton() -> Skeleton3D:
	return _find(_model_root, "Skeleton3D") as Skeleton3D

func _on_anim_activated(idx: int) -> void:
	if _player and idx < _anims.size():
		_log_switch(_anims[idx])
		_reset_pose()
		_player.play(_anims[idx])
		_play_btn.text = "Pause"

func _log_switch(name: String) -> void:
	var f := FileAccess.open("user://mpreview_switches.log", FileAccess.WRITE_READ)
	if f:
		f.seek_end()
		var cur: String = _player.current_animation if _player else "-"
		var pos: float = _player.current_animation_position if _player else 0.0
		f.store_line("%s switch %s@%.2f -> %s" % [Time.get_datetime_string_from_system(), cur, pos, name])
		f.close()

func _toggle_play() -> void:
	if _player == null:
		return
	if _player.is_playing():
		_player.pause()
		_play_btn.text = "Play"
	else:
		_player.play()
		_play_btn.text = "Pause"

func _toggle_loop(btn: Button) -> void:
	_loop_on = not _loop_on
	if _player:
		for a in _anims:
			var anim: Animation = _player.get_animation(a)
			if anim:
				anim.loop_mode = Animation.LOOP_LINEAR if _loop_on else Animation.LOOP_NONE
	btn.text = "Loop: ON" if _loop_on else "Loop: OFF"

func _apply_loop() -> void:
	if _player:
		for a in _anims:
			var anim: Animation = _player.get_animation(a)
			if anim:
				anim.loop_mode = Animation.LOOP_LINEAR if _loop_on else Animation.LOOP_NONE

func _toggle_compare() -> void:
	if _shot_tex and _showing_ref:
		_compare_rect.texture = _shot_tex
		_showing_ref = false
		_compare_title.text = "Compare: Godot shot (C toggles)"
	elif _ref_tex:
		_compare_rect.texture = _ref_tex
		_showing_ref = true
		_compare_title.text = "Compare: Blender ref (C toggles)"

func _set_compare(path: String) -> void:
	var img := Image.load_from_file(path)
	if img == null:
		return
	_ref_tex = ImageTexture.create_from_image(img)
	_compare_rect.texture = _ref_tex
	_showing_ref = true
	_compare_title.text = "Compare: Blender ref (%s)" % path.get_file()

func _on_load_ref_pressed() -> void:
	var dlg := FileDialog.new()
	dlg.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	dlg.add_filter("*.png", "PNG images")
	dlg.file_selected.connect(_set_compare)
	add_child(dlg)
	dlg.popup_centered(Vector2i(600, 400))

func take_shot(path: String) -> void:
	await RenderingServer.frame_post_draw
	var img: Image = _viewport.get_texture().get_image()
	var err := img.save_png(path)
	_shot_tex = ImageTexture.create_from_image(img)
	if not _showing_ref or _compare_rect.texture == null:
		_compare_rect.texture = _shot_tex
		_showing_ref = false
		_compare_title.text = "Compare: Godot shot (C toggles)"
	print("MPREVIEW shot: %s err=%d size=%s" % [path, err, str(img.get_size())])

func _on_view_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion and (event as InputEventMouseMotion).button_mask & MOUSE_BUTTON_MASK_LEFT:
		var rel: Vector2 = (event as InputEventMouseMotion).relative
		_yaw.rotation.y -= rel.x * 0.01
		_pitch.rotation.x = clampf(_pitch.rotation.x - rel.y * 0.01, -1.2, 1.2)
	elif event is InputEventMouseButton and (event as InputEventMouseButton).pressed:
		_zoom_step((event as InputEventMouseButton).button_index)

func _zoom_step(button_index: MouseButton, factor := 0.0) -> void:
	var d: float = _cam.position.z
	if button_index == MOUSE_BUTTON_WHEEL_UP or factor < 0.0:
		d = clampf(d * 0.85, 0.5, 15.0)
	elif button_index == MOUSE_BUTTON_WHEEL_DOWN or factor > 0.0:
		d = clampf(d * 1.18, 0.5, 15.0)
	else:
		return
	_cam.position = Vector3(0, 0, d)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and (event as InputEventKey).pressed and not (event as InputEventKey).echo:
		match (event as InputEventKey).keycode:
			KEY_SPACE:
				_toggle_play()
			KEY_S:
				take_shot("mpreview_shot.png")
			KEY_C:
				_toggle_compare()
			KEY_R:
				_load_cam()
			KEY_EQUAL, KEY_PLUS, KEY_KP_ADD:
				_zoom_step(MOUSE_BUTTON_WHEEL_UP)
			KEY_MINUS, KEY_KP_SUBTRACT:
				_zoom_step(MOUSE_BUTTON_WHEEL_DOWN)
			KEY_Q:
				get_tree().quit()

func _process(_delta: float) -> void:
	if _player and _status_label:
		var an := _player.current_animation
		var t := _player.current_animation_position
		var ln := 0.0
		if an != "" and _player.has_animation(an):
			ln = _player.get_animation(an).length
		_status_label.text = "now: %s  %.2f / %.2f s%s" % [an if an != "" else "-", t, ln, "" if _player.is_playing() else " (paused)"]
	if not _auto_shot:
		return
	_frame += 1
	if _frame >= _shot_frames:
		_auto_shot = false
		take_shot(_shot_path)
		await get_tree().create_timer(0.5).timeout
		get_tree().quit()
