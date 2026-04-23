// Maquette toon (cel) shader — v0.3 A.
//
// Flat cel-shading: one hard-coded directional light, N discrete bands on
// N·L, plus an ambient floor so unlit faces don't go fully black. No
// specular, no PBR, no textures. Matches the product north star ("no 3D
// jargon, consistent look").
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

@group(2) @binding(0) var<uniform> material: ToonParams;

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

    return vec4<f32>(material.base_color.rgb * shade, material.base_color.a);
}
