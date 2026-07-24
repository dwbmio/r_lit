#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

struct ToonMaterial {
    quantize_steps: u32,
    brightness: f32,
    shadow_lift: f32,
#ifdef SIXTEEN_BYTE_ALIGNMENT
    _webgl2_padding_8b: u32,
    _webgl2_padding_12b: u32,
#endif
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> toon_material: ToonMaterial;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    let steps = max(toon_material.quantize_steps, 1u);
    if steps < 48u {
        out.color = vec4<f32>(vec4<u32>(out.color * f32(steps))) / f32(steps);
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.color.rgb = out.color.rgb * toon_material.brightness + vec3<f32>(toon_material.shadow_lift);
    return out;
}
