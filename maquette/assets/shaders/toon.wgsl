// Maquette toon (cel) shader — v0.10 D-1.D.
//
// Flat cel-shading: one hard-coded directional light, N discrete bands on
// N·L, plus an ambient floor so unlit faces don't go fully black. No
// specular, no PBR. Matches the product north star ("no 3D jargon,
// consistent look").
//
// v0.10 D-1.D added an optional `base_color_texture` (binding 1+2). The
// fragment samples it and multiplies against `base_color`, so:
//
//   * Flat mode: Rust hands `base_color_texture: None` ; Bevy's Option
//     AsBindGroup auto-fills a 1×1 white image; sample == 1.0 ; final
//     == base_color × shade. Identical to the pre-D-1.D output.
//   * Textured mode: Rust hands a real PNG handle and sets base_color
//     to white; sample is the AI-generated tile; final == tile ×
//     shade. Toon stepping still applies on top.
//
// This shader runs ONLY in the preview. Exports never reference it —
// the Export Golden Rule says geometry + vertex color + standard material
// only (see docs/handoff/COST_AWARENESS.md).

#import bevy_pbr::forward_io::VertexOutput

struct ToonParams {
    base_color:  vec4<f32>,
    // xyz = light direction (from light toward world), w = band count.
    light_dir_bands: vec4<f32>,
    // x = ambient floor (0..1), yzw unused / padding.
    ambient_pad: vec4<f32>,
}

// Bevy 0.18 moved the material bind group from index 2 to 3 (slot 2
// is now the per-object mesh storage buffer). Using the preprocessor
// directive keeps this shader resilient against future re-slotting.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ToonParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_color_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(-material.light_dir_bands.xyz);
    let ndl = max(dot(n, l), 0.0);

    let bands = max(material.light_dir_bands.w, 1.0);
    // Quantize N·L into `bands` discrete steps in (0, 1].
    let step = ceil(ndl * bands) / bands;

    let ambient = clamp(material.ambient_pad.x, 0.0, 1.0);
    let shade   = ambient + (1.0 - ambient) * step;

    // Texture sample in linear sRGB. When `base_color_texture` is
    // `None` on the Rust side, Bevy fills the slot with a 1×1
    // white image so this multiply is a no-op.
    let tex = textureSample(base_color_texture, base_color_sampler, in.uv);
    let albedo = material.base_color.rgb * tex.rgb;
    let alpha  = material.base_color.a   * tex.a;

    return vec4<f32>(albedo * shade, alpha);
}
