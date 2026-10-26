// ABOUTME: Shaders for the textured 3D view: lit and fogged map and car parts, the mood's sky gradient, and colored overlays.
// ABOUTME: Lighting follows three.js as the web viewer uses it: Lambert diffuse, plus Blinn-Phong highlights for Phong materials.

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_ambient: vec4<f32>,
    ground_ambient: vec4<f32>,
    fog_color: vec4<f32>,
    // x near, y far
    fog: vec4<f32>,
    // Zenith to horizon
    sky: array<vec4<f32>, 5>,
    // x, y: viewport size in pixels, for lines of constant screen width
    viewport: vec4<f32>,
}

struct Material {
    color: vec4<f32>,
    flags: u32,
    opacity: f32,
    pad: vec2<f32>,
}

const HAS_DIFFUSE: u32 = 1u;
const WORLD_UV: u32 = 2u;
const BLEND_BOTH: u32 = 4u;
const HAS_BLEND3: u32 = 8u;
const FLIP_Y: u32 = 16u;
const UNLIT: u32 = 32u;
const EMISSIVE: u32 = 64u;
const FIXED_OPACITY: u32 = 128u;
const PHONG: u32 = 256u;
const HAS_SPECULAR: u32 = 512u;
// three.js MeshPhongMaterial defaults: specular color 0x111111 and shininess 30
const SPECULAR_COLOR: f32 = 0.0056;
const SHININESS: f32 = 30.0;
const PI: f32 = 3.14159265;

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var diffuse_tex: texture_2d<f32>;
@group(1) @binding(2) var blend2_tex: texture_2d<f32>;
@group(1) @binding(3) var blend_i_tex: texture_2d<f32>;
@group(1) @binding(4) var blend3_tex: texture_2d<f32>;
@group(1) @binding(5) var tex_sampler: sampler;
@group(1) @binding(6) var specular_tex: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

fn has(flag: u32) -> bool {
    return (material.flags & flag) != 0u;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = model * vec4<f32>(v.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    out.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    var uv = v.uv;
    // Terrain repeats its texture every 16 world units
    if has(WORLD_UV) {
        uv = world.xz / 16.0;
    } else if has(FLIP_Y) {
        uv.y = 1.0 - uv.y;
    }
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var albedo = material.color;
    if has(HAS_DIFFUSE) {
        albedo = albedo * textureSample(diffuse_tex, tex_sampler, in.uv);
    }
    // Layered grass: a second texture by an intensity mask, then a third by its own alpha
    let blend2 = textureSample(blend2_tex, tex_sampler, in.uv);
    let blend_i = textureSample(blend_i_tex, tex_sampler, in.uv).r;
    let blend3 = textureSample(blend3_tex, tex_sampler, in.uv);
    let specular_map = textureSample(specular_tex, tex_sampler, in.uv).r;
    if has(BLEND_BOTH) {
        albedo = mix(albedo, blend2, blend_i);
    }
    if has(HAS_BLEND3) {
        albedo = mix(albedo, blend3, blend3.a);
    }
    if has(FIXED_OPACITY) {
        albedo.a = material.opacity;
    }
    if albedo.a < 0.02 {
        discard;
    }

    var color = albedo.rgb;
    if !has(UNLIT) {
        var n = normalize(in.normal);
        if !front {
            n = -n;
        }
        let hemisphere = mix(frame.ground_ambient.rgb, frame.sky_ambient.rgb, 0.5 * n.y + 0.5);
        let sun = frame.sun_color.rgb * max(dot(n, frame.sun_dir.xyz), 0.0);
        color = albedo.rgb * (hemisphere + sun) / PI;
        if has(PHONG) {
            // BRDF_BlinnPhong: Schlick Fresnel, implicit geometry term 0.25, normalized Blinn-Phong distribution
            let view = normalize(frame.camera_pos.xyz - in.world);
            let half_dir = normalize(frame.sun_dir.xyz + view);
            let dot_nh = clamp(dot(n, half_dir), 0.0, 1.0);
            let dot_vh = clamp(dot(view, half_dir), 0.0, 1.0);
            let fresnel = exp2((-5.55473 * dot_vh - 6.98316) * dot_vh);
            let f = SPECULAR_COLOR * (1.0 - fresnel) + fresnel;
            let d = (SHININESS * 0.5 + 1.0) * pow(dot_nh, SHININESS) / PI;
            var strength = 1.0;
            if has(HAS_SPECULAR) {
                strength = specular_map;
            }
            color = color + sun * f * 0.25 * d * strength;
        }
        if has(EMISSIVE) {
            color = color + albedo.rgb * 0.4;
        }
    }
    let fog = smoothstep(frame.fog.x, frame.fog.y, distance(in.world, frame.camera_pos.xyz));
    return vec4<f32>(mix(color, frame.fog_color.rgb, fog), albedo.a);
}

// The sky: a full-screen triangle colored by the view ray's elevation, from the horizon up to the zenith

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4<f32>(xy, 0.0, 1.0);
    out.ndc = xy;
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let far = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - frame.camera_pos.xyz);
    let e = max(dir.y, 0.0);
    // Elevation of each color stop, zenith first
    let stops = array<f32, 5>(1.0, 0.55, 0.28, 0.1, 0.0);
    var color = frame.sky[4].rgb;
    for (var k = 1; k < 5; k = k + 1) {
        if e >= stops[k] {
            let t = (e - stops[k]) / (stops[k - 1] - stops[k]);
            color = mix(frame.sky[k].rgb, frame.sky[k - 1].rgb, t);
            break;
        }
    }
    return vec4<f32>(color, 1.0);
}

// Colored overlays: racing-line ribbons and checkpoint posts

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var out: LineOut;
    out.clip = frame.view_proj * vec4<f32>(v.position, 1.0);
    out.color = v.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

// Racing lines: each segment is a quad widened in screen space, so lines keep their pixel width at any distance

struct ScreenLineIn {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    // x: which side of the line (-1 or 1), y: width in pixels
    @location(2) params: vec2<f32>,
    @location(3) color: vec4<f32>,
}

@vertex
fn vs_screen_line(v: ScreenLineIn) -> LineOut {
    var a = frame.view_proj * vec4<f32>(v.position, 1.0);
    var b = frame.view_proj * vec4<f32>(v.other, 1.0);
    // Cut the segment at the near plane so endpoints behind the camera don't flip
    let near = 0.05;
    if a.w < near && b.w >= near {
        a = mix(a, b, (near - a.w) / (b.w - a.w));
    } else if b.w < near && a.w >= near {
        b = mix(b, a, (near - b.w) / (a.w - b.w));
    }
    var out: LineOut;
    out.color = v.color;
    if a.w < near {
        // Entirely behind the camera
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    let half = frame.viewport.xy * 0.5;
    let sa = a.xy / a.w * half;
    let sb = b.xy / b.w * half;
    var dir = sb - sa;
    if length(dir) < 1e-4 {
        dir = vec2<f32>(1.0, 0.0);
    }
    dir = normalize(dir);
    let normal = vec2<f32>(-dir.y, dir.x);
    let offset = normal * v.params.x * v.params.y * 0.5 / half * a.w;
    out.clip = vec4<f32>(a.xy + offset, a.z, a.w);
    return out;
}
