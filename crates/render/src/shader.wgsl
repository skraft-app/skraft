// SolidSketch viewport: shaded faces, B-rep edges and coloured lines.

// Everything is in camera coordinates (the CPU subtracts the camera in f64): the camera is
// at the origin looking down -Z, and only the projection is left to the GPU.
struct Globals {
    proj: mat4x4<f32>,     // reversed depth
    up: vec4<f32>,         // world +Z, in camera coordinates
    key: vec4<f32>,        // key light direction, in camera coordinates
    background: vec4<f32>, // linear RGB
    viewport: vec4<f32>,   // target size (px), half edge width (px), unused
};

@group(0) @binding(0) var<uniform> g: Globals;

// One placement of a part's mesh: its transform to the camera (model-view), the inverse
// transpose of its linear part for normals (n0.w: -1 when the placement mirrors, which turns
// the triangles' winding over), and a tint (rgb mixed in by a: 0 = untouched).
struct Instance {
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
    @location(8) n0: vec4<f32>,
    @location(9) n1: vec4<f32>,
    @location(10) n2: vec4<f32>,
    @location(11) tint: vec4<f32>,
};

// ---- faces ------------------------------------------------------------------------------------

struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>, // camera coordinates
    @location(1) normal: vec3<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) mirrored: f32,
};

@vertex
fn vs_face(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, inst: Instance) -> FaceOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let nmat = mat3x3<f32>(inst.n0.xyz, inst.n1.xyz, inst.n2.xyz);
    let world = model * vec4<f32>(pos, 1.0);
    var out: FaceOut;
    out.clip = g.proj * world;
    out.world = world.xyz;
    out.normal = nmat * normal;
    out.tint = inst.tint;
    out.mirrored = inst.n0.w;
    return out;
}

@fragment
fn fs_face(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    // A mirrored placement turns the winding over: its outside faces the other way.
    let front = facing != (in.mirrored < 0.0);
    // SketchUp's convention: warm white front faces, blue-grey back faces.
    var base = vec3<f32>(0.92, 0.91, 0.87);
    if (!front) {
        n = -n;
        base = vec3<f32>(0.55, 0.60, 0.72);
    }
    let view = normalize(-in.world);
    let head = max(dot(n, view), 0.0);
    let sky = 0.5 + 0.5 * dot(n, g.up.xyz);
    let light = 0.30 + 0.45 * head + 0.15 * max(dot(n, g.key.xyz), 0.0) + 0.10 * sky;
    let shaded = base * light;
    return vec4<f32>(mix(shaded, in.tint.rgb, in.tint.a), 1.0);
}

// ---- wide lines ---------------------------------------------------------------------------------

// Segments are drawn as quads (two triangles) widened on screen, so their width is set in
// pixels: hardware lines are one pixel wide, half a point on a high-density display.
// `ca`, `cb`: the ends in clip space; `corner.x`: which end (0 = a, 1 = b); `corner.y`: which
// side (±1); `half`: half the width, in pixels.
fn wide(ca_in: vec4<f32>, cb_in: vec4<f32>, corner: vec2<f32>, half: f32) -> vec4<f32> {
    // Keep the segment in front of the eye (clip space is linear along it).
    let near = 1e-4;
    var ca = ca_in;
    var cb = cb_in;
    if (ca.w < near && cb.w < near) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0); // degenerate: nothing drawn
    }
    if (ca.w < near) {
        ca = mix(ca, cb, (near - ca.w) / (cb.w - ca.w));
    } else if (cb.w < near) {
        cb = mix(cb, ca, (near - cb.w) / (ca.w - cb.w));
    }
    let size = g.viewport.xy;
    let sa = ca.xy / ca.w * size * 0.5;
    let sb = cb.xy / cb.w * size * 0.5;
    var dir = sb - sa;
    let len = length(dir);
    dir = select(vec2<f32>(1.0, 0.0), dir / len, len > 1e-6);
    let normal = vec2<f32>(-dir.y, dir.x);
    // Widen sideways, and lengthen by half the width so joints have no notch.
    let along = (corner.x * 2.0 - 1.0) * half;
    let offset = (normal * corner.y * half + dir * along) / (size * 0.5);
    var p = select(ca, cb, corner.x > 0.5);
    p = vec4<f32>(p.xy + offset * p.w, p.z, p.w);
    return p;
}

// A quad carries the depth of its centre line, but a pixel away from it a neighbouring face
// can be nearer (both faces of a concave corner rise towards the eye; a face seen at a
// glance climbs fast): the widened part would sink into it. So lines are pulled towards the
// eye by `PULL` times their half width, measured at their own depth. In perspective, scaling
// a camera-space point towards the eye (at the origin) keeps it on the same pixel; in
// parallel projection, moving it along the view axis does.
const PULL: f32 = 3.0;

fn pulled(p: vec4<f32>, half: f32) -> vec4<f32> {
    // The size of a pixel at unit depth (perspective) or anywhere (parallel).
    let pixel = 2.0 / (g.proj[1][1] * g.viewport.y);
    let d = PULL * half * pixel;
    if (g.proj[3][3] == 0.0) {
        return vec4<f32>(p.xyz * max(1.0 - d, 0.5), 1.0); // perspective
    }
    return vec4<f32>(p.xy, p.z + d, 1.0); // parallel: the eye looks down -Z
}

// ---- B-rep edges (instanced like faces) -------------------------------------------------------

struct EdgeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) tint: vec4<f32>,
};

@vertex
fn vs_edge(@location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) corner: vec2<f32>, inst: Instance) -> EdgeOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    var out: EdgeOut;
    let half = g.viewport.z;
    let ca = g.proj * pulled(model * vec4<f32>(a, 1.0), half);
    let cb = g.proj * pulled(model * vec4<f32>(b, 1.0), half);
    out.clip = wide(ca, cb, corner, half);
    out.tint = inst.tint;
    return out;
}

@fragment
fn fs_edge(in: EdgeOut) -> @location(0) vec4<f32> {
    let ink = vec3<f32>(0.02, 0.02, 0.03);
    return vec4<f32>(mix(ink, in.tint.rgb, in.tint.a), 1.0);
}

// ---- world-space coloured lines (grid, axes, tool previews) -----------------------------------

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) corner: vec3<f32>, @location(3) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    let ca = g.proj * pulled(vec4<f32>(a, 1.0), corner.z);
    let cb = g.proj * pulled(vec4<f32>(b, 1.0), corner.z);
    out.clip = wide(ca, cb, corner.xy, corner.z);
    out.color = color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(mix(g.background.rgb, in.color.rgb, in.color.a), 1.0);
}
