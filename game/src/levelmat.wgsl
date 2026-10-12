// The level's surfaces (levelmat.rs): the standard PBR material with the lightmap, plus two things that hide how a
// texture repeats. A second look at the colour at another scale and offset, blended in by a patchy world-space noise,
// breaks the tile grid on organic sets (concrete, plaster, asphalt, grass). A slow brightness and tint variation
// across the world makes long walls and wide ground uneven, as real ones are. Patterned sets (bricks, tiles, planks)
// only get the variation: a second sample would smear their joints.
#import bevy_pbr::{
    mesh_view_bindings::{view, globals},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing, calculate_tbn_mikktspace, apply_normal_mapping},
    forward_io::{VertexOutput, FragmentOutput},
}

// x: the second sample's scale, y: how much of it, z: the variation's strength (w unused).
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> detail: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var detail_color: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var detail_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var sky_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var sky_sampler: sampler;
// x: the sky's nits at a texel of 1, y: the baked light of a spot seeing the open sky, z: texture occlusion strength.
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> reflect_params: vec4<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(106) var set_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var set_orm: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var over_color: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var over_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var over_orm: texture_2d<f32>;
// x: overlay coverage, y: overlay repeats per metre, z: parallax depth (UV), w: 1 for a ground set.
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var<uniform> ground: vec4<f32>;
// The material's colour factor (linear), for the ground path.
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var<uniform> tint: vec4<f32>;

// The split-sum environment BRDF, fitted (Karis, "Physically Based Shading on Mobile").
fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Value noise, 0..1, smooth.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash12(i);
    let b = hash12(i + vec2<f32>(1.0, 0.0));
    let c = hash12(i + vec2<f32>(0.0, 1.0));
    let d = hash12(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Two-component (BC5) normal → tangent-space normal.
fn unpack_nt(c: vec4<f32>) -> vec3<f32> {
    let xy = c.rg * 2.0 - 1.0;
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
}

fn rot2(v: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

struct Ground {
    color: vec3<f32>,
    nt: vec3<f32>,      // tangent-space normal (the face's own tangent frame)
    rough: f32,
    ao: f32,
}

// A ground set, its repeat hidden and weathered:
// - the set twice, the second turned a quarter and shifted, their mix picked by a slow world noise and resolved by
//   height (the higher stone wins), so no joint is ever smeared;
// - parallax: the UVs walk along the view ray over the height a few steps, so setts stand proud of their joints;
// - the overlay (dust, mud) laid in world space at its own scale and angle, filling the joints first and spreading
//   over the stones in patches;
// - a slow darkening and tint over tens of metres.
fn ground_surface(in: VertexOutput, q: vec2<f32>, V: vec3<f32>) -> Ground {
    let uv = in.uv;
    // Parallax: the view ray in the face's tangent frame.
    let N = normalize(in.world_normal);
    let T = in.world_tangent.xyz;
    let B = in.world_tangent.w * cross(N, T);
    let vt = vec3<f32>(dot(V, T), dot(V, B), max(dot(V, N), 0.2));
    let pstep = -vt.xy / vt.z * ground.z;
    // The second sample's frame: a quarter turn (UVs and tangent normals) and a shift.
    let w = smoothstep(0.35, 0.65, vnoise(q * 0.13 + 3.0) * 0.7 + vnoise(q * 0.41) * 0.3);
    var uva = uv;
    var uvb = rot2(uv, 1.5708) + vec2<f32>(0.37, 0.61);
    let stepb = rot2(pstep, 1.5708);
    if (ground.z > 0.0) {
        // Three offset steps (offset limiting): each moves the UV by the height still left to climb.
        for (var i = 0; i < 3; i++) {
            let ha = textureSample(set_orm, detail_sampler, uva).b;
            let hb = textureSample(set_orm, detail_sampler, uvb).b;
            uva = uv + pstep * (ha - 0.5);
            uvb = rot2(uv, 1.5708) + vec2<f32>(0.37, 0.61) + stepb * (hb - 0.5);
        }
    }
    let oa = textureSample(set_orm, detail_sampler, uva);
    let ob = textureSample(set_orm, detail_sampler, uvb);
    // Height blend of the two samples.
    let ha = oa.b + (1.0 - w);
    let hb = ob.b + w;
    let top = max(ha, hb) - 0.15;
    let ba = max(ha - top, 0.0);
    let bb = max(hb - top, 0.0);
    let kb = bb / max(ba + bb, 1e-4);
    let ca = textureSample(detail_color, detail_sampler, uva).rgb;
    let cb = textureSample(detail_color, detail_sampler, uvb).rgb;
    let na = unpack_nt(textureSample(set_normal, detail_sampler, uva));
    var nb = unpack_nt(textureSample(set_normal, detail_sampler, uvb));
    // (turn the second normal back by the quarter its UVs were turned)
    nb = vec3<f32>(rot2(nb.xy, -1.5708), nb.z);
    var g: Ground;
    g.color = mix(ca, cb, kb);
    g.nt = normalize(mix(na, nb, kb));
    g.rough = mix(oa.g, ob.g, kb);
    g.ao = mix(oa.r, ob.r, kb);
    let h = mix(oa.b, ob.b, kb);

    // The overlay, in world space (on horizontal ground), turned 30° so its grid never lines up with the set's.
    // (sampled in uniform control flow: WebGPU rejects implicit-derivative samples behind a per-pixel branch; the
    // horizontal-only rule is applied to the blend weight instead)
    if (ground.x > 0.0) {
        let wuv = rot2(q, 0.52) * ground.y;
        let dc = textureSample(over_color, detail_sampler, wuv).rgb;
        let dn = unpack_nt(textureSample(over_normal, detail_sampler, wuv));
        let dorm = textureSample(over_orm, detail_sampler, wuv);
        // Where: patches (two octaves of noise), plus more of it in the low parts (joints first).
        let patches = vnoise(q * 0.21 + 7.0) * 0.6 + vnoise(q * 0.73 + 1.0) * 0.4;
        let cover = ground.x * (0.35 + 1.3 * smoothstep(0.35, 0.75, patches));
        let k = clamp((cover + dorm.b * 0.25 - h) * 4.0, 0.0, 1.0) * step(0.6, N.y);
        // (dry and dusty: a little lighter and less saturated than the photo's damp soil)
        // (the overlay's tint is the base's: patches of another asphalt or concrete match their surroundings; dust is
        // lifted, being the dry top layer)
        var dust = mix(vec3<f32>(dot(dc, vec3<f32>(0.3, 0.55, 0.15))), dc, 0.4) * tint.rgb;
        if (tint.w > 0.5) {
            dust = mix(vec3<f32>(dot(dc, vec3<f32>(0.3, 0.55, 0.15))), dc, 0.6) * vec3<f32>(1.3, 1.28, 1.25);
        }
        g.color = mix(g.color, dust, k);
        // The overlay's normal, from world (u along the turned x, v along the turned z) into the face's frame.
        let tu = vec3<f32>(cos(0.52), 0.0, sin(0.52));
        let tv = vec3<f32>(-sin(0.52), 0.0, cos(0.52));
        let nw = normalize(dn.x * tu + dn.y * tv + dn.z * vec3<f32>(0.0, 1.0, 0.0));
        let nt_over = normalize(vec3<f32>(dot(nw, normalize(T)), dot(nw, normalize(B)), dot(nw, N)));
        g.nt = normalize(mix(g.nt, nt_over, k));
        g.rough = mix(g.rough, dorm.g, k);
        g.ao = mix(g.ao, dorm.r, k);
    }
    // The set's occlusion is half the story here (the parallax and the overlay carry the rest): at full strength,
    // times the lightmap, the joints and the whole ground went dark.
    g.ao = mix(1.0, g.ao, 0.55);
    g.rough = max(g.rough, 0.6);
    // Slow variation: darker and lighter stretches, a little warmer or cooler.
    let v = vnoise(q * 0.07) * 0.6 + vnoise(q * 0.23 + 5.0) * 0.4 - 0.5;
    let t = vnoise(q * 0.04 + 11.0) - 0.5;
    g.color *= (1.0 + v * 0.45) * vec3<f32>(1.0 + t * 0.12, 1.0, 1.0 - t * 0.12);
    return g;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let wp = in.world_position.xyz;
    // Water: small wind ripples (a few travelling waves and noise) tilt the normal; the sky's reflection does the rest.
    if (detail.w > 0.5) {
        let t = globals.time;
        let p = wp.xz;
        // Broad, irregular harbour ripples; fade fine detail before it aliases on the horizon.
        let footprint = max(length(dpdx(p)), length(dpdy(p)));
        let fine = 1.0 - smoothstep(0.12, 0.8, footprint);
        let phase = vnoise(p * 0.13) * 3.0;
        var g = vec2<f32>(0.0);
        g += vec2<f32>(0.8, 0.6) * cos(dot(p, vec2<f32>(0.8, 0.6)) * 1.6 + phase + t * 0.8) * 0.04;
        g += vec2<f32>(-0.5, 0.86) * cos(dot(p, vec2<f32>(-0.5, 0.86)) * 2.8 + t * 1.2) * 0.024 * fine;
        g += vec2<f32>(0.2, -0.98) * cos(dot(p, vec2<f32>(0.2, -0.98)) * 4.1 + t * 1.6) * 0.014 * fine;
        let e = 0.07;
        let q = p * 2.3 + vec2<f32>(t * 0.21, t * 0.13);
        g += vec2<f32>(vnoise(q + vec2<f32>(e, 0.0)) - vnoise(q - vec2<f32>(e, 0.0)), vnoise(q + vec2<f32>(0.0, e)) - vnoise(q - vec2<f32>(0.0, e))) * 0.18 * fine;
        pbr_input.N = normalize(vec3<f32>(-g.x * 1.2, 1.0, -g.y * 1.2));
    }
    let n = abs(normalize(in.world_normal));
    // World coordinates across the face's plane, for noise that runs on across neighbouring faces.
    var q = wp.xz;
    if (n.y < max(n.x, n.z)) {
        q = select(wp.xy, wp.zy, n.x > n.z);
    }
    var rgb = pbr_input.material.base_color.rgb;

#ifdef VERTEX_UVS_A
#ifdef VERTEX_TANGENTS
    if (ground.w > 0.5) {
        let g = ground_surface(in, q, pbr_input.V);
        // (the material's base colour factor tints the set, as the standard path does)
        rgb = g.color * tint.rgb;
        let TBN = calculate_tbn_mikktspace(normalize(in.world_normal), in.world_tangent);
        pbr_input.N = normalize(TBN[0] * g.nt.x + TBN[1] * g.nt.y + TBN[2] * g.nt.z);
        pbr_input.material.perceptual_roughness = clamp(g.rough, 0.089, 1.0);
        pbr_input.material.metallic = 0.0;
        pbr_input.diffuse_occlusion = vec3<f32>(g.ao);
    }
#endif
    if (detail.y > 0.0 && ground.w < 0.5) {
        let uv2 = in.uv * detail.x + vec2<f32>(0.37, 0.61);
        let second = textureSample(detail_color, detail_sampler, uv2).rgb;
        let m = smoothstep(0.3, 0.7, vnoise(q * 0.21 + 17.0) * 0.65 + vnoise(q * 0.53) * 0.35) * detail.y;
        rgb = mix(rgb, second, m);
    }
#endif

    // Slow variation: brightness, and a little towards warm or cool.
    let v = vnoise(q * 0.09) * 0.6 + vnoise(q * 0.31 + 5.0) * 0.4 - 0.5;
    let t = vnoise(q * 0.05 + 11.0) - 0.5;
    if (ground.w < 0.5) {
        rgb *= 1.0 + v * 2.0 * detail.z;
        rgb *= vec3<f32>(1.0 + t * detail.z, 1.0, 1.0 - t * detail.z);
    }

    pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), pbr_input.material.base_color.a);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    // The texture's own occlusion (mortar joints, gaps between setts) darkens the baked light too: Bevy applies it to
    // ambient and environment light only, so on lightmapped surfaces the crevices were as bright as the faces.
    let ao = mix(1.0, pbr_input.diffuse_occlusion.x, reflect_params.z);
    pbr_input.lightmap_light *= ao;
    // How much sky this spot sees, from its baked light: full in the open, none deep in a room.
    let baked = dot(pbr_input.lightmap_light, vec3<f32>(0.2126, 0.7152, 0.0722));
    let sky_seen = smoothstep(0.0, 1.0, baked / reflect_params.y);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    // The sky's reflection (the level has no reflection probes): glossy glass and metal mirror it, rough plaster and
    // stone only take a sheen at grazing angles, as in daylight.
    if (reflect_params.x > 0.0) {
        let nv = max(dot(pbr_input.N, pbr_input.V), 1e-4);
        let r = reflect(-pbr_input.V, pbr_input.N);
        let rough = pbr_input.material.perceptual_roughness;
        let metal = pbr_input.material.metallic;
        let f0 = mix(0.16 * pbr_input.material.reflectance * pbr_input.material.reflectance, pbr_input.material.base_color.rgb, metal);
        let lod = rough * 8.0;
        let env = textureSampleLevel(sky_cube, sky_sampler, r * vec3<f32>(1.0, 1.0, -1.0), lod).rgb * reflect_params.x;
        // What reflects below the horizon is the ground and the walls, darker than the painted ground of the sky.
        let horizon = mix(0.35, 1.0, smoothstep(-0.25, 0.1, r.y));
        let spec = env * env_brdf(f0, rough, nv) * sky_seen * ao * horizon;
        out.color = vec4<f32>(out.color.rgb + spec * view.exposure, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
