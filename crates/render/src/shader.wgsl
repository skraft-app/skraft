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
    section: vec4<f32>,    // x: section on; y: how much darker caps are; z: ghosts' opacity; w: faint edges' opacity
    hatch_u: vec4<f32>,    // the plane's basis for the caps' hatches: dot(xyz, p) + w measures a point p of the plane (mm)
    hatch_v: vec4<f32>,
    hatch: vec4<f32>,      // x: spacing (px); y: half width (px); z: opacity
};

@group(0) @binding(0) var<uniform> g: Globals;
// The colours faces and placements refer to (linear RGB).
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;

const NO_MATERIAL: u32 = 0xffffffffu;

// One placement of a part's mesh: its transform to the camera (model-view), the inverse
// transpose of its linear part for normals (n0.w: -1 when the placement mirrors, which turns
// the triangles' winding over; n1.w: a ghost's opacity; n2.w: its caps' hatch, an index in `hatch_direction`), a
// tint (rgb mixed in by a: 0 = untouched) and its palette entry.
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
    // A ghost's opacity over the ghosts'.
    @location(5) @interpolate(flat) opacity: f32,
    // Its section caps' hatch (an index in `hatch_direction`), and their pitch (as `hatch_measure`
    // measures across them).
    @location(6) @interpolate(flat) hatch: f32,
    @location(7) @interpolate(flat) pitch: f32,
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
    out.opacity = inst.n1.w;
    out.hatch = inst.n2.w;
    out.pitch = hatch_pitch(inst.m3.xyz, inst.n2.w);
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

// Without a section.
@fragment
fn fs_face(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    return shade(in, facing, false, 0.0);
}

// With a section: what is cut away is discarded. The backs seen through the cut are painted
// as caps (§7 quater): parts are closed, so where the nearest surface left is a back face, the
// eye looks into the part through the plane. Where the stencil then marks a cap, `fs_cap`
// paints it again, hatched, just behind the plane; these stay where it does not (the eye inside
// a part, parts overlapping).
@fragment
fn fs_face_cut(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    return shade(in, facing, true, 0.0);
}

// The directions across a cap's hatches, in the plane's basis (`g.hatch_u`, `g.hatch_v`):
// whole numbers, so that the anchor, snapped to 4 m in that basis, leaves the pattern as it is.
// The lines run at 45° and 135° of `u`, then at about 27°, 63°, 117° and 153° (a part's
// chosen by its definition, `hatch_of`).
fn hatch_direction(hatch: f32) -> vec2<f32> {
    var hatches = array<vec2<f32>, 6>(
        vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, -2.0),
        vec2<f32>(2.0, -1.0), vec2<f32>(2.0, 1.0), vec2<f32>(1.0, 2.0),
    );
    return hatches[min(u32(hatch), 5u)];
}

// Where the camera-space point `q` of the plane is across the hatches `hatch` (mm, scaled by
// the length of their direction).
fn hatch_measure(q: vec3<f32>, hatch: f32) -> f32 {
    let k = hatch_direction(hatch);
    return k.x * (dot(g.hatch_u.xyz, q) + g.hatch_u.w) + k.y * (dot(g.hatch_v.xyz, q) + g.hatch_v.w);
}

// The pitch of a placement's cap hatches (as `hatch_measure` measures across them): the power of
// two that puts them 1 to 2 spacings apart (`g.hatch.x`) where its centre `o` (camera
// coordinates) falls on the plane, the plane's slant there counted. One pitch for the whole cap:
// a pattern drawn on the plane, closing up with the perspective, every other line coming or
// going at once when the zoom crosses a level — not a pitch per pixel, which would end half
// the lines across a cap seen at a slant.
fn hatch_pitch(o: vec3<f32>, hatch: f32) -> f32 {
    if (g.section.x < 0.5) {
        return 0.0;
    }
    let n = g.clip.xyz;
    let c = o - n * (dot(n, o) + g.clip.w);
    // The size of a pixel at unit depth (perspective) or anywhere (parallel).
    let pixel = 2.0 / (g.proj[1][1] * g.viewport.y);
    var size = pixel;
    var facing = abs(n.z);
    if (g.proj[3][3] == 0.0) {
        let dist = max(length(c), 1e-6);
        size = dist * pixel;
        facing = abs(dot(n, c)) / dist;
    }
    // Across the hatches, the measure grows by the length of their direction a millimetre; the
    // slant stretches a pixel's footprint along one direction (its geometric mean taken).
    let rate = length(hatch_direction(hatch)) * size / sqrt(max(facing, 0.05));
    return exp2(ceil(log2(g.hatch.x * rate)));
}

// How much of a pixel a cap's hatches cover (0–1): `t` measures the pixel's point of the plane
// across them (mm), `rate` how much it changes from one pixel to the next, `pitch` the
// placement's (`hatch_pitch`). All in one ink; where they close up too much to be told apart
// (far off, at a slant), their mean tone instead of a shimmer.
fn hatch_cover(t: f32, rate: f32, pitch: f32) -> f32 {
    if (!(rate > 0.0) || !(pitch > 0.0)) {
        return 0.0;
    }
    let width = 2.0 * g.hatch.y + 1.0;
    let spacing = pitch / rate;
    let tone = min(2.0 * g.hatch.y / spacing, 1.0);
    return mix(hatch_line(t, pitch, rate), tone, smoothstep(3.0 * width, 1.5 * width, spacing));
}

// How much of a pixel the nearest line at a multiple of `pitch` covers (antialiased).
fn hatch_line(t: f32, pitch: f32, rate: f32) -> f32 {
    let k = t / pitch;
    let px = abs(k - round(k)) * pitch / rate;
    return clamp(g.hatch.y + 0.5 - px, 0.0, 1.0);
}

// A ghost's faces (an open part's body, every part in X-ray): its fronts only, shaded,
// blended at the ghosts' opacity raised by its own (a machining's, a little more opaque than a
// blank); cut by the section like the rest.
@fragment
fn fs_face_ghost(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    if (cut(in.world) || !is_front(in, facing)) {
        discard;
    }
    return vec4<f32>(shade(in, facing, false, 0.0).rgb, min(g.section.z + in.opacity, 1.0));
}

// A ghost highlighted: its fronts washed with the tint, at a part of the tint's weight — over
// the ghost's own faces, see-through still.
@fragment
fn fs_face_ghost_mark(in: FaceOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    if (cut(in.world) || !is_front(in, facing)) {
        discard;
    }
    return vec4<f32>(in.tint.rgb, in.tint.a * 0.6);
}

// Does the fragment show the outside of its part? (A mirrored placement turns the winding
// over.)
fn is_front(in: FaceOut, facing: bool) -> bool {
    return facing != (in.mirrored < 0.0);
}

// `hatch`: how much a cap's hatches cover the pixel.
fn shade(in: FaceOut, facing: bool, caps: bool, hatch: f32) -> vec4<f32> {
    var n = normalize(in.normal);
    let front = is_front(in, facing);
    var base = front_colour(in.colour);
    if (!front) {
        if (caps) {
            // A section cap: the part's colour, flat and darker, hatched in the edges' ink.
            let cap = mix(base * (1.0 - g.section.y), g.edge.rgb, hatch * g.hatch.z);
            return vec4<f32>(mix(cap, in.tint.rgb, in.tint.a), 1.0);
        }
        n = -n;
        base = g.face_back.rgb;
    }
    let view = normalize(-in.world);
    let head = max(dot(n, view), 0.0);
    let sky = 0.5 + 0.5 * dot(n, g.up.xyz);
    let light = 0.30 + 0.45 * head + 0.15 * max(dot(n, g.key.xyz), 0.0) + 0.10 * sky;
    let shaded = base * light;
    return vec4<f32>(mix(shaded, in.tint.rgb, in.tint.a), 1.0);
}

// Second pass over the faces, without depth test nor colour: each fragment left flips the
// stencil. Parts are closed, so an odd count means the ray meets the plane inside a part —
// there, the plane is a cap.
@fragment
fn fs_cap_mark(in: FaceOut) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    return vec4<f32>(0.0);
}

// ---- section caps (the back faces again, where the stencil marks a cap) -----------------------

// How far behind the plane a cap is drawn, as a part of the way to its back face: the order of
// the back faces is kept (the nearest, the cut part's, wins), any front face behind the plane
// loses — even one lying on the back face (another part in contact, a dowel in its hole, the
// two meshes crossing by their chords) — and so do the part's own far edges.
const CAP_DEPTH: f32 = 1.0 / 64.0;

struct CapOut {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// A back face seen through the cut, where the stencil count is odd: the cap, hatched, anchored
// on the plane, drawn just behind it.
@fragment
fn fs_cap(in: FaceOut, @builtin(front_facing) facing: bool) -> CapOut {
    // The point of the plane this pixel sees, and how far it moves from one pixel to the next:
    // derivatives, so taken before any branch.
    let q = on_plane(in.world);
    let dx = dpdx(q.xyz);
    let dy = dpdy(q.xyz);
    if (cut(in.world) || is_front(in, facing)) {
        discard;
    }
    var out: CapOut;
    if (q.w == 0.0) {
        // The plane seen edge on: the back face as it is.
        out.colour = shade(in, facing, true, 0.0);
        out.depth = in.clip.z;
        return out;
    }
    let k = hatch_direction(in.hatch);
    let d = k.x * g.hatch_u.xyz + k.y * g.hatch_v.xyz;
    let rate = length(vec2<f32>(dot(d, dx), dot(d, dy)));
    out.colour = shade(in, facing, true, hatch_cover(hatch_measure(q.xyz, in.hatch), rate, in.pitch));
    let c = g.proj * vec4<f32>(q.xyz, 1.0);
    let plane = clamp(c.z / c.w, 0.0, 1.0);
    out.depth = plane - (plane - in.clip.z) * CAP_DEPTH;
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
// A segment in clip space.
struct Segment {
    a: vec4<f32>,
    b: vec4<f32>,
};

// The part of a segment in front of the eye (clip space is linear along it); at least one end
// must be.
fn in_front(ca_in: vec4<f32>, cb_in: vec4<f32>) -> Segment {
    let near = 1e-4;
    var ca = ca_in;
    var cb = cb_in;
    if (ca.w < near) {
        ca = mix(ca, cb, (near - ca.w) / (cb.w - ca.w));
    } else if (cb.w < near) {
        cb = mix(cb, ca, (near - cb.w) / (ca.w - cb.w));
    }
    return Segment(ca, cb);
}

// The segment's length on screen (pixels), its part in front of the eye.
fn screen_length(ca_in: vec4<f32>, cb_in: vec4<f32>) -> f32 {
    if (ca_in.w < 1e-4 && cb_in.w < 1e-4) {
        return 0.0;
    }
    let s = in_front(ca_in, cb_in);
    let size = g.viewport.xy;
    return length(s.b.xy / s.b.w * size * 0.5 - s.a.xy / s.a.w * size * 0.5);
}

// How far along the segment its quad's corner is, on screen (pixels from `a`): the quad runs
// half a width past each end.
fn along_px(ca_in: vec4<f32>, cb_in: vec4<f32>, corner: vec2<f32>, half: f32) -> f32 {
    if (ca_in.w < 1e-4 && cb_in.w < 1e-4) {
        return 0.0;
    }
    return select(0.0, screen_length(ca_in, cb_in), corner.x > 0.5) + (corner.x * 2.0 - 1.0) * half;
}

fn wide(ca_in: vec4<f32>, cb_in: vec4<f32>, corner: vec2<f32>, half: f32) -> vec4<f32> {
    if (ca_in.w < 1e-4 && cb_in.w < 1e-4) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0); // degenerate: nothing drawn
    }
    let s = in_front(ca_in, cb_in);
    let ca = s.a;
    let cb = s.b;
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

// The same, see-through (what the eraser would take).
@fragment
fn fs_edge_faint(in: EdgeOut) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    return vec4<f32>(mix(g.edge.rgb, in.tint.rgb, in.tint.a), g.section.w);
}

// ---- world-space coloured lines (grid, axes, tool previews) -----------------------------------

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // The dash pattern (px: dash, gap, dash, gap; all 0 for a plain line), and where the
    // pattern is here (px along the line on screen, interpolated on screen).
    @location(1) @interpolate(flat) pattern: vec4<f32>,
    @location(2) @interpolate(linear) along: f32,
    // Only so far from the line's start drawn (px; a box's corners), 0 for the whole line.
    @location(3) @interpolate(flat) reach: f32,
};

@vertex
fn vs_line(
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) corner: vec3<f32>,
    @location(3) color: vec4<f32>,
    @location(4) pattern: vec4<f32>,
    @location(5) dash: vec2<f32>,
) -> LineOut {
    return line_out(a, b, corner, color, pattern, dash.x, dash.y > 0.5, 0.0);
}

// A box's outline (instanced): the edges of the unit cube placed by `m0..m3` (to camera
// space), its dash `pattern` (px), `params`: half its width (px), how far from its corners its
// edges are drawn (px, 0: whole), whether the edges of its faces turned to the eye are drawn
// again over the scene, whether it is drawn wholly over the scene. 24 quads of 6 corners: each
// edge as two halves run from its ends to its middle, so a dash starts at every corner.
struct BoxIn {
    @builtin(vertex_index) vi: u32,
    @location(0) m0: vec4<f32>,
    @location(1) m1: vec4<f32>,
    @location(2) m2: vec4<f32>,
    @location(3) m3: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) pattern: vec4<f32>,
    @location(6) params: vec4<f32>,
};

// In the scene: every edge, depth tested; none for a box drawn over the scene.
@vertex
fn vs_box(in: BoxIn) -> LineOut {
    if (in.params.w > 0.5) {
        return no_line();
    }
    return box_edge(in);
}

// Over the scene: a box drawn over it, wholly; one with its faces turned to the eye drawn
// over it (OpenCutList's `shell_on_top`), those faces' edges; others, nothing.
@vertex
fn vs_box_front(in: BoxIn) -> LineOut {
    if (in.params.w > 0.5) {
        return box_edge(in);
    }
    if (in.params.z < 0.5) {
        return no_line();
    }
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    let e = box_edge_corners(in.vi);
    let axis = e.x ^ e.y;
    // The edge's two faces: across each other axis, on the side of its corners.
    for (var k = 0u; k < 3u; k++) {
        if ((1u << k) != axis && face_exposed(model, k, (e.x >> k) & 1u)) {
            return box_edge(in);
        }
    }
    return no_line();
}

fn no_line() -> LineOut {
    var none: LineOut;
    none.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0); // degenerate: nothing drawn
    return none;
}

// The cube's corners at the ends of the half edge drawn by vertex `vi`: its corner, then the
// other end of its edge.
fn box_edge_corners(vi: u32) -> vec2<u32> {
    // The cube's edges by their corners: corner i is at the bits of i (x: 1, y: 2, z: 4).
    var edges = array<vec2<u32>, 12>(
        vec2<u32>(0u, 1u), vec2<u32>(0u, 2u), vec2<u32>(0u, 4u), vec2<u32>(1u, 3u),
        vec2<u32>(1u, 5u), vec2<u32>(2u, 3u), vec2<u32>(2u, 6u), vec2<u32>(3u, 7u),
        vec2<u32>(4u, 5u), vec2<u32>(4u, 6u), vec2<u32>(5u, 7u), vec2<u32>(6u, 7u),
    );
    let half_edge = vi / 6u;
    let e = edges[half_edge / 2u];
    if ((half_edge & 1u) == 1u) {
        return vec2<u32>(e.y, e.x);
    }
    return e;
}

fn box_edge(in: BoxIn) -> LineOut {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let e = box_edge_corners(in.vi);
    let pa = cube_corner(e.x);
    let pb = (pa + cube_corner(e.y)) * 0.5;
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    let a = (model * vec4<f32>(pa, 1.0)).xyz;
    let b = (model * vec4<f32>(pb, 1.0)).xyz;
    return line_out(a, b, vec3<f32>(quad[in.vi % 6u], in.params.x), in.color, in.pattern, 0.0, false, in.params.y);
}

fn cube_corner(i: u32) -> vec3<f32> {
    return vec3<f32>(f32(i & 1u), f32((i >> 1u) & 1u), f32((i >> 2u) & 1u));
}

// Whether the face of the unit cube across axis `k` on side `side` (0 or 1), placed by
// `model` (to camera space), is turned to the eye: its outward normal against the ray through
// its centre; seen edge on, not (OpenCutList's `exposed_path_indexes`).
fn face_exposed(model: mat4x4<f32>, k: u32, side: u32) -> bool {
    var c = vec3<f32>(0.5, 0.5, 0.5);
    c[k] = f32(side);
    let centre = (model * vec4<f32>(c, 1.0)).xyz;
    var cols = array<vec3<f32>, 3>(model[0].xyz, model[1].xyz, model[2].xyz);
    var n = cross(cols[(k + 1u) % 3u], cols[(k + 2u) % 3u]);
    // Towards +k (a mirroring placement turns the cross product over), then outwards.
    let along = cols[k];
    if (dot(along, along) > 1e-12 && dot(n, along) < 0.0) {
        n = -n;
    }
    if (side == 0u) {
        n = -n;
    }
    // Towards the eye: to the origin in perspective, +Z in parallel projection.
    let eye = select(vec3<f32>(0.0, 0.0, 1.0), -centre, g.proj[3][3] == 0.0);
    let nl = length(n);
    let el = length(eye);
    return nl > 0.0 && el > 0.0 && dot(n, eye) > 1e-9 * nl * el;
}

// A corner of a coloured segment's quad, `a` and `b` in camera space; its dash `pattern`
// (px) at `phase` px at `a`, or, `anchored`, stretched a little so that a dash starts and ends
// it on screen; drawn only `reach` px from `a` when not 0.
fn line_out(a: vec3<f32>, b: vec3<f32>, corner: vec3<f32>, color: vec4<f32>, pattern: vec4<f32>, phase: f32, anchored: bool, reach: f32) -> LineOut {
    var out: LineOut;
    let ca = g.proj * pulled(vec4<f32>(a, 1.0), corner.z);
    let cb = g.proj * pulled(vec4<f32>(b, 1.0), corner.z);
    out.clip = wide(ca, cb, corner.xy, corner.z);
    out.color = color;
    var p = pattern;
    var at = phase;
    let period = dot(pattern, vec4<f32>(1.0));
    if (anchored && period > 0.0) {
        // Whole periods and a last dash: as long as the line.
        let len = screen_length(ca, cb);
        let k = max(round((len - pattern.x) / period), 0.0);
        let span = k * period + pattern.x;
        p = select(vec4<f32>(0.0), pattern * (len / span), span > 0.0 && len > 0.0);
        at = 0.0;
    }
    out.pattern = p;
    out.along = at + along_px(ca, cb, corner.xy, corner.z);
    out.reach = reach;
    return out;
}

// Whether a line's fragment falls in a gap of its dashes, or past its reach.
fn gap(in: LineOut) -> bool {
    if (in.reach > 0.0 && in.along > in.reach) {
        return true;
    }
    let period = dot(in.pattern, vec4<f32>(1.0));
    if (period <= 0.0) {
        return false;
    }
    let t = in.along - floor(in.along / period) * period;
    let p = in.pattern;
    return !(t < p.x || (t >= p.x + p.y && t < p.x + p.y + p.z));
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    if (gap(in)) {
        discard;
    }
    return vec4<f32>(mix(g.background.rgb, in.color.rgb, in.color.a), 1.0);
}

// The overlay's lines: blended over what is drawn under them (their alpha is a true opacity).
@fragment
fn fs_overlay(in: LineOut) -> @location(0) vec4<f32> {
    if (gap(in)) {
        discard;
    }
    return in.color;
}

// ---- translucent fills (a tool's planes: in the scene, depth tested, or over it; blended) -----

struct FillOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_fill(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> FillOut {
    var out: FillOut;
    out.clip = g.proj * vec4<f32>(pos, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    return in.color;
}
