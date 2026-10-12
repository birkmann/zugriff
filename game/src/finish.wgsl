// Weapon finishes (finish.rs): the weapon's own material, painted over through its `finish` UV map (TEXCOORD_1: the
// weapon seen from its side, in metres from its back end and its bottom). A tile repeats every so many metres; a wrap
// spans the whole weapon. The paint keeps the light and dark of the weapon's own colour map (its edges, wear and
// print show through), leaves bare metal partly bare, and brings its own metalness and roughness. Metal paints get a
// studio reflection of their own: the view has no reflection probes.
#import bevy_pbr::{
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

// x: the weapon's length (m), y: its height (m), z: a tile's size (m; 0 for a wrap), w: the original colour map's mean
// luminance (linear).
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> shape: vec4<f32>;
// x: coverage, y: how much bare metal stays bare, z: how much of the original's light and dark shows, w: metalness.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> look: vec4<f32>;
// x: roughness, y: the studio reflection's strength, z: its brightness (nits at full strength), w: glow.
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> surf: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var paint_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var paint_sampler: sampler;

fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// A photo studio seen in a reflection: a soft bright top, a dark floor, a strip light along the horizon.
fn studio(r: vec3<f32>, rough: f32) -> vec3<f32> {
    let up = r.y;
    let w = 0.12 + rough * 0.6;
    let sky = smoothstep(-0.25 - w, 0.7 + w, up);
    let strip = exp(-pow((up - 0.18) / (0.05 + rough * 0.3), 2.0)) * (1.0 - rough * 0.7);
    return vec3<f32>(0.04) + vec3<f32>(0.85, 0.88, 0.92) * sky + vec3<f32>(1.6) * strip;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var glow = vec3<f32>(0.0);
#ifdef VERTEX_UVS_B
    let q = in.uv_b;
    let tiled = shape.z > 0.0;
    let uv = select(vec2<f32>(q.x / max(shape.x, 0.01), 1.0 - q.y / max(shape.y, 0.01)), vec2<f32>(q.x, -q.y) / max(shape.z, 0.001), tiled);
    let paint = textureSample(paint_tex, paint_sampler, uv);
    let orig = pbr_input.material.base_color.rgb;
    let lum = dot(orig, vec3<f32>(0.2126, 0.7152, 0.0722));
    let detail = clamp(pow(max(lum, 1e-4) / max(shape.w, 1e-3), look.z), 0.35, 1.7);
    let metal = pbr_input.material.metallic;
    let cover = clamp(look.x * paint.a * (1.0 - metal * look.y), 0.0, 1.0);
    pbr_input.material.base_color = vec4<f32>(mix(orig, paint.rgb * detail, cover), pbr_input.material.base_color.a);
    pbr_input.material.metallic = mix(metal, look.w, cover);
    pbr_input.material.perceptual_roughness = clamp(mix(pbr_input.material.perceptual_roughness, surf.x, cover), 0.089, 1.0);
    // Glow: the paint's brightest, most saturated strokes light up a little (neon designs).
    let sat = max(paint.r, max(paint.g, paint.b)) - min(paint.r, min(paint.g, paint.b));
    glow = paint.rgb * smoothstep(0.35, 0.8, sat) * smoothstep(0.5, 0.9, max(paint.r, max(paint.g, paint.b))) * surf.w * cover;
#endif
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    if (surf.y > 0.0) {
        let nv = max(dot(pbr_input.N, pbr_input.V), 1e-4);
        let r = reflect(-pbr_input.V, pbr_input.N);
        let rough = pbr_input.material.perceptual_roughness;
        let m = pbr_input.material.metallic;
        let f0 = mix(vec3<f32>(0.04), pbr_input.material.base_color.rgb, m);
        let spec = studio(r, rough) * env_brdf(f0, rough, nv) * surf.y * surf.z;
        out.color = vec4<f32>(out.color.rgb + spec * view.exposure, out.color.a);
    }
    out.color = vec4<f32>(out.color.rgb + glow * surf.z * view.exposure * 0.25, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
