class_name TexPackerAnimPlayer
extends Node
## Animation player for TexPackerSprite — plays animation sequences from atlas data.
##
## Usage:
##   1. Add as child of a TexPackerSprite node
##   2. Set animation_name to an animation group from the atlas
##   3. Call play() to start animating

## The TexPackerSprite node to animate (auto-detected from parent if null)
@export var target: TexPackerSprite = null

## Animation name (from atlas "animations" data)
@export var animation_name: String = ""

## Frames per second
@export var fps: float = 10.0

## Loop the animation
@export var loop_anim: bool = true

## Auto-play on ready
@export var autoplay: bool = false

var _frames: Array = []
var _current_frame: int = 0
var _elapsed: float = 0.0
var _playing: bool = false

signal animation_finished

func _ready() -> void:
	if target == null and get_parent() is TexPackerSprite:
		target = get_parent() as TexPackerSprite

	if autoplay and not animation_name.is_empty():
		play(animation_name)

func _process(delta: float) -> void:
	if not _playing or _frames.is_empty() or target == null:
		return

	_elapsed += delta
	var frame_duration = 1.0 / fps

	if _elapsed >= frame_duration:
		_elapsed -= frame_duration
		_current_frame += 1

		if _current_frame >= _frames.size():
			if loop_anim:
				_current_frame = 0
			else:
				_current_frame = _frames.size() - 1
				_playing = false
				animation_finished.emit()
				return

		target.set_sprite(_frames[_current_frame])

## Play an animation by name.
func play(anim_name: String = "") -> void:
	if not anim_name.is_empty():
		animation_name = anim_name

	if target == null:
		push_error("TexPackerAnimPlayer: no target TexPackerSprite")
		return

	var loader = target.get_loader()
	if loader == null:
		push_error("TexPackerAnimPlayer: target has no loaded atlas")
		return

	_frames = loader.get_animation_frames(animation_name)
	if _frames.is_empty():
		push_warning("TexPackerAnimPlayer: animation '%s' not found or empty" % animation_name)
		return

	_current_frame = 0
	_elapsed = 0.0
	_playing = true
	target.set_sprite(_frames[0])

## Stop the animation.
func stop() -> void:
	_playing = false

## Is the animation currently playing?
func is_playing() -> bool:
	return _playing

## Get current frame index.
func get_current_frame() -> int:
	return _current_frame

## Set current frame index.
func set_frame(idx: int) -> void:
	if idx >= 0 and idx < _frames.size():
		_current_frame = idx
		if target:
			target.set_sprite(_frames[idx])
