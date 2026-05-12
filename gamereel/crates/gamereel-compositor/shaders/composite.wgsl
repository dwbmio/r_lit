// composite.wgsl — single-quad sprite blit with anchor + scale + rotation + opacity.
//
// Per draw: one quad (4 vertices via instance indexing of triangles).
// The host pushes one Uniforms struct per sprite:
//   * scene_size: render target dims (px)
//   * sprite_size: sprite native dims (px) before scale
//   * pos: anchor target in scene-space px
//   * scale: per-axis multiplier
//   * rotation_rad: rotation around anchor
//   * anchor: 0..1 within the sprite
//   * opacity: 0..1
//
// Output framebuffer is RGBA8 with premultiplied "src over dst" alpha.

struct Uniforms {
    scene_size:    vec2<f32>,
    sprite_size:   vec2<f32>,
    pos:           vec2<f32>,
    scale:         vec2<f32>,
    anchor:        vec2<f32>,
    rotation_rad:  f32,
    opacity:       f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var sprite_tex: texture_2d<f32>;
@group(0) @binding(2) var sprite_smp: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Two triangles forming the unit quad in [0,1]^2.
    // Naga (WGSL validator) only allows constant indexing into arrays
    // declared with `let`, so we enumerate via switch instead.
    var q: vec2<f32>;
    switch vid {
        case 0u: { q = vec2<f32>(0.0, 0.0); }
        case 1u: { q = vec2<f32>(1.0, 0.0); }
        case 2u: { q = vec2<f32>(0.0, 1.0); }
        case 3u: { q = vec2<f32>(1.0, 0.0); }
        case 4u: { q = vec2<f32>(1.0, 1.0); }
        case 5u: { q = vec2<f32>(0.0, 1.0); }
        default: { q = vec2<f32>(0.0, 0.0); }
    }

    // Sprite-local position in pixels: (q - anchor) * sprite_size * scale
    let local_px = (q - u.anchor) * u.sprite_size * u.scale;

    // Rotate around the anchor.
    let c = cos(u.rotation_rad);
    let s = sin(u.rotation_rad);
    let rot = mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
    let rotated_px = rot * local_px;

    // Translate into scene coordinates (anchor lands at u.pos).
    let scene_px = rotated_px + u.pos;

    // Map [0..scene_size] → clip space [-1..1].
    // Image / video convention: y down. Clip space: y up. Flip y.
    let ndc_x = scene_px.x / u.scene_size.x * 2.0 - 1.0;
    let ndc_y = 1.0 - scene_px.y / u.scene_size.y * 2.0;

    var out: VsOut;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = q;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_tex, sprite_smp, in.uv);
    // Apply per-sprite opacity to the alpha channel only — image convention.
    return vec4<f32>(texel.rgb, texel.a * u.opacity);
}
