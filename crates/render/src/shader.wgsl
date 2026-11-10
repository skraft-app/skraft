// SolidSketch viewport: shaded faces, B-rep edges, silhouettes and coloured lines.

// Everything is in camera coordinates (the CPU subtracts the camera in f64): the camera is
// at the origin looking down -Z, and only the projection is left to the GPU.
struct Globals {
    proj: mat4x4<f32>,     // reversed depth
    up: vec4<f32>,         // world +Z, in camera coordinates
    key: vec4<f32>,        // key light direction, in camera coordinates
    background: vec4<f32>, // linear RGB
    viewport: vec4<f32>,   // target size (px), half edge width (px), unused
    face_front: vec4<f32>, // a face without a colour
    face_back: vec4<f32>,  // the back of every face
    edge: vec4<f32>,       // edges and silhouettes
    clip: vec4<f32>,       // the section plane: dot(xyz, p) + w > 0 is cut away
    section: vec4<f32>,    // x: section on; y: how much darker caps are
};

@group(0) @binding(0) var<uniform> g: Globals;
// The colours faces and placements refer to (linear RGB).
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;

const NO_MATERIAL: u32 = 0xffffffffu;

// One placement of a part's mesh: its transform to the camera (model-view), the inverse
// transpose of its linear part for normals (n0.w: -1 when the placement mirrors, which turns
// the triangles' winding over), a tint (rgb mixed in by a: 0 = untouched) and its palette
// entry.
struct Instance {
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
    @location(8) n0: vec4<f32>,
    @location(9) n1: vec4<f32>,
    @location(10) n2: vec4<f32>,
    @location(11) tint: vec4<f32>,
    @location(12) material: u32,
};

// Is the camera-space point `p` cut away by the section?
fn cut(p: vec3<f32>) -> bool {
    return g.section.x > 0.5 && dot(g.clip.xyz, p) + g.clip.w > 0.0;
}

// ---- faces ------------------------------------------------------------------------------------

struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>, // camera coordinates
    @location(1) normal: vec3<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) mirrored: f32,
    @location(4) @interpolate(flat) colour: u32,
};

@vertex
fn vs_face(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) paint: u32, inst: Instance) -> FaceOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let nmat = mat3x3<f32>(inst.n0.xyz, inst.n1.xyz, inst.n2.xyz);
    let world = model * vec4<f32>(pos, 1.0);
    var out: FaceOut;
    out.clip = g.proj * world;
    out.world = world.xyz;
    out.normal = nmat * normal;
    out.tint = inst.tint;
    out.mirrored = inst.n0.w;
    // The face's own colour, else its placement's.
    out.colour = select(inst.material, paint, paint != NO_MATERIAL);
    return out;
}

fn front_colour(i: u32) -> vec3<f32> {
    if (i != NO_MATERIAL && i < arrayLength(&palette)) {
        return palette[i].rgb;
    }
    return g.face_front.rgb;
}

struct FaceFrag {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Without a section: the hardware depth (and its bias, which lets edges win).
@fragment
fn fs_face(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    return shade(in, facing, in.clip.z, false).colour;
}

// With a section: cut, and caps written at the plane's depth. Writing the depth drops the
// bias, so this pipeline is only used while a section is on.
@fragment
fn fs_face_cut(in: FaceOut, @builtin(front_facing) facing: bool) -> FaceFrag {
    if (cut(in.world)) {
        discard;
    }
    return shade(in, facing, in.clip.z, true);
}

fn shade(in: FaceOut, facing: bool, depth: f32, caps: bool) -> FaceFrag {
    var out: FaceFrag;
    out.depth = depth;
    var n = normalize(in.normal);
    // A mirrored placement turns the winding over: its outside faces the other way.
    let front = facing != (in.mirrored < 0.0);
    var base = front_colour(in.colour);
    if (!front) {
        if (caps) {
            // A section cap (§7 quater): parts are closed, so through the cut only their back
            // shows. Paint it as the plane: the part's colour, flat and darker, at the depth
            // of the plane along this pixel's ray.
            let p = on_plane(in.world);
            if (p.w > 0.0) {
                let c = g.proj * vec4<f32>(p.xyz, 1.0);
                out.depth = c.z / c.w;
                let cap = base * (1.0 - g.section.y);
                out.colour = vec4<f32>(mix(cap, in.tint.rgb, in.tint.a), 1.0);
                return out;
            }
        }
        n = -n;
        base = g.face_back.rgb;
    }
    let view = normalize(-in.world);
    let head = max(dot(n, view), 0.0);
    let sky = 0.5 + 0.5 * dot(n, g.up.xyz);
    let light = 0.30 + 0.45 * head + 0.15 * max(dot(n, g.key.xyz), 0.0) + 0.10 * sky;
    let shaded = base * light;
    out.colour = vec4<f32>(mix(shaded, in.tint.rgb, in.tint.a), 1.0);
    return out;
}

// Where the ray through the camera-space point `p` meets the section plane (w: 1), or w: 0
// when it does not (the plane seen edge on, or behind the eye).
fn on_plane(p: vec3<f32>) -> vec4<f32> {
    let n = g.clip.xyz;
    if (g.proj[3][3] == 0.0) {
        // Perspective: the ray from the eye (the origin) through p.
        let d = dot(n, p);
        if (abs(d) < 1e-12) {
            return vec4<f32>(0.0);
        }
        let t = -g.clip.w / d;
        if (t <= 0.0) {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(p * t, 1.0);
    }
    // Parallel: the ray along -Z through p.
    if (abs(n.z) < 1e-6) {
        return vec4<f32>(0.0);
    }
    let z = -(g.clip.w + n.x * p.x + n.y * p.y) / n.z;
    return vec4<f32>(p.x, p.y, z, 1.0);
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

// ---- B-rep edges and silhouettes (instanced like faces) ----------------------------------------

struct EdgeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) tint: vec4<f32>,
    // Where the line is, before it is pulled towards the eye (for the section).
    @location(1) world: vec3<f32>,
};

fn edge_out(a: vec3<f32>, b: vec3<f32>, corner: vec2<f32>, inst: Instance) -> EdgeOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    var out: EdgeOut;
    let half = g.viewport.z;
    let wa = model * vec4<f32>(a, 1.0);
    let wb = model * vec4<f32>(b, 1.0);
    let ca = g.proj * pulled(wa, half);
    let cb = g.proj * pulled(wb, half);
    out.clip = wide(ca, cb, corner, half);
    out.tint = inst.tint;
    out.world = select(wa.xyz, wb.xyz, corner.x > 0.5);
    return out;
}

@vertex
fn vs_edge(@location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) corner: vec2<f32>, inst: Instance) -> EdgeOut {
    return edge_out(a, b, corner, inst);
}

// A mesh edge inside a curved surface, with its two triangles' normals: drawn only where one
// faces the eye and the other does not — the outline of the surface as seen from here.
@vertex
fn vs_silhouette(
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) corner: vec2<f32>,
    @location(3) n1: vec3<f32>,
    @location(13) n2: vec3<f32>,
    inst: Instance,
) -> EdgeOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let nmat = mat3x3<f32>(inst.n0.xyz, inst.n1.xyz, inst.n2.xyz);
    let mid = (model * vec4<f32>((a + b) * 0.5, 1.0)).xyz;
    // Towards the eye: to the origin in perspective, +Z in parallel projection.
    let eye = select(vec3<f32>(0.0, 0.0, 1.0), -mid, g.proj[3][3] == 0.0);
    let s1 = dot(nmat * n1, eye);
    let s2 = dot(nmat * n2, eye);
    if (s1 * s2 > 0.0) {
        var none: EdgeOut;
        none.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0); // degenerate: nothing drawn
        return none;
    }
    return edge_out(a, b, corner, inst);
}

@fragment
fn fs_edge(in: EdgeOut) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    return vec4<f32>(mix(g.edge.rgb, in.tint.rgb, in.tint.a), 1.0);
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
