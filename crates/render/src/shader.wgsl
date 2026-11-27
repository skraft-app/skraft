// Skraft viewport: shaded faces, B-rep edges, silhouettes and coloured lines.

// Everything is in camera coordinates (the CPU subtracts the camera in f64): the camera is
// at the origin looking down -Z, and only the projection is left to the GPU.
struct Globals {
    proj: mat4x4<f32>,     // reversed depth
    up: vec4<f32>,         // world +Z, in camera coordinates
    key: vec4<f32>,        // key light direction, in camera coordinates
    background: vec4<f32>, // linear RGB
    viewport: vec4<f32>,   // target size (px), half edge width (px: the faces' push too), unused
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
// Each palette entry's texture (docs/render/look.md#textures): x its layer (< 0: none), y and z
// the inverse of its width and height (mm), w its rotation (radians).
@group(0) @binding(2) var<storage, read> textures: array<vec4<f32>>;
@group(0) @binding(3) var texture_layers: texture_2d_array<f32>;
@group(0) @binding(4) var texture_sampler: sampler;
// Each palette entry's finish (docs/render/look.md#palette-and-materials): `A` at 2i, `B` at
// 2i + 1, what is under shown as A·x + B.
@group(0) @binding(5) var<storage, read> overlays: array<vec4<f32>>;
// A layer's side (texels), lib.rs's `TEXTURE_SIZE`.
const TEXTURE_SIZE: f32 = 1024.0;

const NO_MATERIAL: u32 = 0xffffffffu;
// Set in an instance's palette entry: it wins over its faces' own paint (lib.rs's `FORCED`).
const FORCED: u32 = 0x40000000u;

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
    // From the mesh's coordinates to those of its texture's projection: columns, the
    // translation in their `w`.
    @location(13) t0: vec4<f32>,
    @location(14) t1: vec4<f32>,
    @location(15) t2: vec4<f32>,
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
    // In the axes of the part it shows: a texture's projection (`textured`).
    @location(8) local: vec3<f32>,
    @location(9) local_normal: vec3<f32>,
};

@vertex
fn vs_face(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) paint: u32, inst: Instance) -> FaceOut {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let nmat = mat3x3<f32>(inst.n0.xyz, inst.n1.xyz, inst.n2.xyz);
    let world = model * vec4<f32>(pos, 1.0);
    var out: FaceOut;
    out.clip = g.proj * pushed_back(world.xyz, nmat * normal);
    out.world = world.xyz;
    out.normal = nmat * normal;
    out.tint = inst.tint;
    out.mirrored = inst.n0.w;
    out.opacity = inst.n1.w;
    out.hatch = inst.n2.w;
    out.pitch = hatch_pitch(inst.m3.xyz, inst.n2.w);
    let texture = mat3x3<f32>(inst.t0.xyz, inst.t1.xyz, inst.t2.xyz);
    out.local = texture * pos + vec3<f32>(inst.t0.w, inst.t1.w, inst.t2.w);
    out.local_normal = texture * normal;
    // The face's own colour, else its placement's; the placement's when forced (no entry left
    // under the flag is the default colour).
    let forced = inst.material != NO_MATERIAL && (inst.material & FORCED) != 0u;
    out.colour = select(select(inst.material, paint, paint != NO_MATERIAL), inst.material & ~FORCED, forced);
    return out;
}

// The colour of a front face of palette entry `i` with a texture: its image projected on the
// face in its part's axes along the axis it most faces (a box projection), its width along the
// part's length, its red axis — across it on the end grain —, at its real size, turned by its
// rotation. Its mip level from the size of a pixel on the face (distance and slant), not from
// derivatives: they would need a uniform control flow. Without a texture, `base`.
fn textured(in: FaceOut, base: vec3<f32>) -> vec3<f32> {
    let i = in.colour;
    if (i == NO_MATERIAL || i >= arrayLength(&textures)) {
        return base;
    }
    let t = textures[i];
    if (t.x < 0.0) {
        return base;
    }
    let a = abs(in.local_normal);
    var facing = 2u;
    if (a.x >= a.y && a.x >= a.z) {
        facing = 0u;
    } else if (a.y >= a.z) {
        facing = 1u;
    }
    var u_axis = 0u;
    var v_axis = 3u - facing;
    if (facing == 0u) {
        u_axis = 1u;
        v_axis = 2u;
    }
    let p = in.local;
    let flat_uv = vec2<f32>(p[u_axis] * t.y, p[v_axis] * t.z);
    let c = cos(t.w);
    let s = sin(t.w);
    let uv = vec2<f32>(c * flat_uv.x - s * flat_uv.y, s * flat_uv.x + c * flat_uv.y);
    // A pixel's footprint on the face (mm), in texels.
    let pixel = 2.0 / (g.proj[1][1] * g.viewport.y);
    var size = pixel;
    if (g.proj[3][3] == 0.0) {
        size = length(in.world) * pixel;
    }
    let slant = max(abs(dot(normalize(in.normal), normalize(-in.world))), 0.2);
    let texels = size / slant * max(t.y, t.z) * TEXTURE_SIZE;
    let lod = max(log2(max(texels, 1e-6)), 0.0);
    return textureSampleLevel(texture_layers, texture_sampler, uv, i32(t.x), lod).rgb;
}

// What is under, `x`, with palette entry `i`'s finish over it.
fn coloured(i: u32, x: vec3<f32>) -> vec3<f32> {
    if (i == NO_MATERIAL || 2u * i + 1u >= arrayLength(&overlays)) {
        return x;
    }
    return x * overlays[2u * i].rgb + overlays[2u * i + 1u].rgb;
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
// as caps (docs/render/look.md): parts are closed, so where the nearest surface left is a back face, the
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
            let cap = mix(coloured(in.colour, base) * (1.0 - g.section.y), g.edge.rgb, hatch * g.hatch.z);
            return vec4<f32>(mix(cap, in.tint.rgb, in.tint.a), 1.0);
        }
        n = -n;
        base = g.face_back.rgb;
    } else {
        base = coloured(in.colour, textured(in, base));
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
    // Its depth just behind the plane (the plane seen edge on: the back face's own), and that
    // depth's slope, pushed back as the faces are, so the cut lines draw whole over it.
    let c = g.proj * vec4<f32>(q.xyz, 1.0);
    let plane = clamp(c.z / max(c.w, 1e-6), 0.0, 1.0);
    let z = select(in.clip.z, plane - (plane - in.clip.z) * CAP_DEPTH, q.w != 0.0);
    let slope = max(abs(dpdx(z)), abs(dpdy(z)));
    if (cut(in.world) || is_front(in, facing)) {
        discard;
    }
    var out: CapOut;
    out.depth = pushed(z, slope);
    if (q.w == 0.0) {
        // The plane seen edge on: the back face as it is.
        out.colour = shade(in, facing, true, 0.0);
        return out;
    }
    let k = hatch_direction(in.hatch);
    let d = k.x * g.hatch_u.xyz + k.y * g.hatch_v.xyz;
    let rate = length(vec2<f32>(dot(d, dx), dot(d, dy)));
    out.colour = shade(in, facing, true, hatch_cover(hatch_measure(q.xyz, in.hatch), rate, in.pitch));
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

// The shortest edge segment on screen, in half widths, whose quad follows its depth past its ends.
const SHORTEST_TAPER: f32 = 4.0;

fn wide(ca_in: vec4<f32>, cb_in: vec4<f32>, corner: vec2<f32>, half: f32, follow: bool) -> vec4<f32> {
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
    if (!follow) {
        return vec4<f32>(p.xy + offset * p.w, p.z, p.w);
    }
    // An edge: depth is linear on screen, so past its end the quad goes on along the edge's own
    // depth, or it would be stretched over the whole segment and sink at its nearer end. Not for
    // a segment about as short as it is wide: seen end on, its tip would rise through what hides
    // it.
    let za = ca.z / ca.w;
    let zb = cb.z / cb.w;
    let z = select(za, zb, corner.x > 0.5);
    let past = select(0.0, (zb - za) / len * along, len > SHORTEST_TAPER * half);
    return vec4<f32>(p.xy + offset * p.w, max(z + past, min(z, 0.0)) * p.w, p.w);
}

// A quad carries the depth of its centre line, but a pixel away from it a neighbouring face
// can be nearer (both faces of a concave corner rise towards the eye; a face seen at a glance
// climbs fast): the widened part would sink into it. So the faces are pushed back from the eye
// by the depth they gain over most of an edge's half width (`pushed_back`), and lines are drawn
// at their own depth: an edge on a face draws (nearly) whole over it at any angle, and what lies
// behind a face shows only within a few pixels of the face's own slope, measured on screen —
// the same at every zoom (docs/render/display.md#depth-of-lines; SolidCraft's `FACE_BIAS`, worked out
// here: a GPU's slope bias is its own, Apple's several times WebGPU's).

// How steeply a face is seen, at most: the depth it gains per unit across the screen (the
// tangent of the angle between its normal and the ray), capped where it is seen edge on.
const STEEPEST: f32 = 50.0;

// How much of an edge's half width the faces are pushed back over: what lies behind a face
// shows where it is nearer than about (this + 1) half widths of the face's slope (the face
// recedes across the edge's quad too), and a line on a steep face keeps this much of its half
// width over it. 7/8: a concealed hinge 5 mm behind a door hidden at 12.7 m (25°), a pocket's
// corners inked (tests `embedded_edges_stay_hidden_far_away`, `concave_edges_are_not_buried`).
const FACE_PUSH: f32 = 0.875;

// The camera-space point `p` of a face of normal `n`, moved away from the eye along its ray
// (its pixel kept) by the depth the face gains over `FACE_PUSH` of an edge's half width there.
fn pushed_back(p: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    let perspective = g.proj[3][3] == 0.0;
    let ray = select(vec3<f32>(0.0, 0.0, -1.0), normalize(p), perspective);
    let c = abs(dot(normalize(n), ray));
    let steep = min(sqrt(max(1.0 - c * c, 0.0)) / max(c, 1.0 / STEEPEST), STEEPEST);
    // The size of a pixel at unit depth (perspective) or anywhere (parallel).
    let pixel = 2.0 / (g.proj[1][1] * g.viewport.y);
    let push = FACE_PUSH * g.viewport.z * pixel * steep;
    if (perspective) {
        // A share of the depth, its pixel growing with it: a point behind the eye stays behind
        // it, for the near plane to cut its triangles.
        return vec4<f32>(p * (1.0 + push), 1.0);
    }
    return vec4<f32>(p.xy, p.z - push, 1.0);
}

// A line of the tools wider than an edge is lifted towards the eye by the pixels of its half
// width the faces' push does not cover, measured at its depth. In perspective, scaling a
// camera-space point towards the eye (at the origin) keeps it on the same pixel; in parallel
// projection, moving it along the view axis does.
fn lifted(p: vec4<f32>, half: f32) -> vec4<f32> {
    let px = half - g.viewport.z;
    if (px <= 0.0) {
        return p;
    }
    // The size of a pixel at unit depth (perspective) or anywhere (parallel).
    let pixel = 2.0 / (g.proj[1][1] * g.viewport.y);
    let d = px * pixel;
    if (g.proj[3][3] == 0.0) {
        return vec4<f32>(p.xyz * max(1.0 - d, 0.5), 1.0); // perspective
    }
    return vec4<f32>(p.xy, p.z + d, 1.0); // parallel: the eye looks down -Z
}

// A depth the shader writes itself (the caps'), pushed back as the faces are: by its own slope
// over `FACE_PUSH` of an edge's half width (`pushed_back`), and the `FACE_STEPS` depth steps
// their pipelines add — a step 2⁻²³ of the depth in a float buffer, 2⁻²⁴ in a 24-bit one: the
// larger of the two, whichever the device has.
const FACE_STEPS: f32 = 8.0;

fn pushed(z: f32, slope: f32) -> f32 {
    let step = max(abs(z) * 1.1920929e-7, 1.0 / 16777216.0);
    return max(z - slope * FACE_PUSH * g.viewport.z - step * FACE_STEPS, 0.0);
}

// ---- B-rep edges and silhouettes (instanced like faces) ----------------------------------------

struct EdgeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) tint: vec4<f32>,
    // Where the line is (for the section).
    @location(1) world: vec3<f32>,
    // How far along its edge (model units, `segment.a.w` to `b.w`), for its dashes.
    @location(2) along: f32,
    // Its dashes' period there (model units, a power of two), 0 for a silhouette's segment
    // (none along it: dashed on screen); on screen, the axis it runs most along (0: x, 1: y).
    @location(3) @interpolate(flat) dash: vec2<f32>,
};

// A dashed edge's period on screen, in half widths (`g.viewport.z`), and its dash's share.
const EDGE_DASH_PERIOD: f32 = 10.0;
const EDGE_DASH_SHARE: f32 = 0.6;

// A segment of an edge (lib.rs's `Segment`): its ends (`w`: how far along its edge, model
// units; 0 at both for a silhouette's), and the outward normals of the two triangles along it
// (`w`: 1, 0 when there is none).
struct EdgeSegment {
    a: vec4<f32>,
    b: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
};

// The drawn mesh's segments: its B-rep edges', or its silhouette candidates'.
@group(1) @binding(0) var<storage, read> segments: array<EdgeSegment>;

// A corner of a segment's quad (4 corners, indexed: lib.rs's `SEGMENT_INDICES`): its end (k & 1)
// and its side (-1, then +1), half a width past each end so that joints have no notch, at the
// segment's own depth. An edge whose two faces turn away is not drawn (`all`: drawn all the
// same, for what is seen through): its closed solid hides it, however close behind its faces.
// An outline (`outline`), only where one of its triangles faces the eye and the other does not.
fn band(vi: u32, inst: Instance, outline: bool, all: bool) -> EdgeOut {
    var none: EdgeOut;
    none.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0); // degenerate: nothing drawn
    let seg = segments[vi / 4u];
    let k = vi % 4u;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let nmat = mat3x3<f32>(inst.n0.xyz, inst.n1.xyz, inst.n2.xyz);
    let pa = model * vec4<f32>(seg.a.xyz, 1.0);
    let pb = model * vec4<f32>(seg.b.xyz, 1.0);
    // Towards the eye: to the origin in perspective, +Z in parallel projection.
    let mid = (pa.xyz + pb.xyz) * 0.5;
    let eye = select(vec3<f32>(0.0, 0.0, 1.0), -mid, g.proj[3][3] == 0.0);
    let front1 = seg.n1.w != 0.0 && dot(nmat * seg.n1.xyz, eye) > 0.0;
    let front2 = seg.n2.w != 0.0 && dot(nmat * seg.n2.xyz, eye) > 0.0;
    if (outline && front1 == front2) {
        return none;
    }
    if (!all && seg.n1.w != 0.0 && seg.n2.w != 0.0 && !front1 && !front2) {
        return none;
    }
    let corner = vec2<f32>(f32(k & 1u), select(-1.0, 1.0, k >= 2u));
    var out: EdgeOut;
    out.clip = wide(g.proj * pa, g.proj * pb, corner, g.viewport.z, true);
    out.tint = inst.tint;
    out.world = select(pa.xyz, pb.xyz, corner.x > 0.5);
    // Its dashes: in model units along the edge, a power of two near the period on screen (the
    // same from one segment to the next), so they follow it as the view turns.
    out.along = select(seg.a.w, seg.b.w, corner.x > 0.5);
    let ca = g.proj * pa;
    let cb = g.proj * pb;
    let px = screen_length(ca, cb);
    let span = length(seg.b.xyz - seg.a.xyz);
    let period = select(0.0, exp2(round(log2(EDGE_DASH_PERIOD * g.viewport.z * span / px))), px > 1e-3 && span > 0.0 && seg.b.w > seg.a.w);
    let s = (cb.xy / max(cb.w, 1e-4) - ca.xy / max(ca.w, 1e-4)) * g.viewport.xy;
    out.dash = vec2<f32>(period, select(1.0, 0.0, abs(s.x) >= abs(s.y)));
    return out;
}

@vertex
fn vs_edge(@builtin(vertex_index) vi: u32, inst: Instance) -> EdgeOut {
    return band(vi, inst, false, false);
}

// The same for what is seen through (ghosts, faint edges): its edges hidden by its own solid
// drawn too.
@vertex
fn vs_edge_all(@builtin(vertex_index) vi: u32, inst: Instance) -> EdgeOut {
    return band(vi, inst, false, true);
}

// A mesh edge inside a curved surface, with its two triangles' normals: drawn only where one
// faces the eye and the other does not — the outline of the surface as seen from here.
@vertex
fn vs_silhouette(@builtin(vertex_index) vi: u32, inst: Instance) -> EdgeOut {
    return band(vi, inst, true, true);
}

@fragment
fn fs_edge(in: EdgeOut) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    return vec4<f32>(mix(g.edge.rgb, in.tint.rgb, in.tint.a), 1.0);
}

// The same, dashed (a linked body's): along the edge, or on screen for a silhouette.
@fragment
fn fs_edge_dashed(in: EdgeOut) -> @location(0) vec4<f32> {
    if (cut(in.world)) {
        discard;
    }
    var t: f32;
    if (in.dash.x > 0.0) {
        t = fract(in.along / in.dash.x);
    } else {
        let period = EDGE_DASH_PERIOD * g.viewport.z;
        t = fract(select(in.clip.x, in.clip.y, in.dash.y > 0.5) / period);
    }
    if (t > EDGE_DASH_SHARE) {
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
    // The dash pattern (dash, gap, dash, gap; all 0 for a plain line): model units when laid
    // in them (its period a power of two), px otherwise.
    @location(1) @interpolate(flat) pattern: vec4<f32>,
    // How far along the line on screen (px from `a`, interpolated on screen): for `reach`, and
    // where a pattern on screen is (with its phase).
    @location(2) @interpolate(linear) along: f32,
    // Only so far from the line's start drawn (px; a box's corners), 0 for the whole line.
    @location(3) @interpolate(flat) reach: f32,
    // Where a pattern laid on the line itself is (interpolated perspective-correct, so that its
    // dashes stay where they are on it as the view orbits): anchored, px from `a` (the pattern
    // stretched to fit); in model units, from the line's nearer end.
    @location(4) @interpolate(perspective) laid: f32,
    // How the pattern lies (`LAY_*`) and its phase: in model units, at the nearer end, high and
    // low parts; on screen, px at `a`.
    @location(5) @interpolate(flat) lay: vec3<f32>,
};

// How a dash pattern lies (`line_out`'s `dash.x`, then `LineOut::lay.z`).
const LAY_MODEL: f32 = 0.0;
const LAY_ANCHORED: f32 = 1.0;
const LAY_SCREEN: f32 = 2.0;

@vertex
fn vs_line(
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) corner: vec3<f32>,
    @location(3) color: vec4<f32>,
    @location(4) pattern: vec4<f32>,
    // How its pattern lies (`LAY_*`), then (see `line_out`): in model units, the phase at the
    // line's nearer end (high and low parts) and the line's length, negative when that end is
    // `b`; anchored, 1 (a whole dash at `b`); on screen, the phase (px) at `a`.
    @location(5) dash: vec4<f32>,
) -> LineOut {
    return line_out(a, b, corner, color, pattern, dash, 0.0);
}

// How much nearer an axis is drawn, as a share of its depth (a few depth steps in parallel
// projection): over the edges it lies on, whatever their rounding, and still behind any face in
// front of it (docs/render/display.md#depth-of-lines).
const AXIS_LIFT: f32 = 1e-5;

// One of the model's axes: a line at the true depth of its points, a hair nearer.
@vertex
fn vs_axis_line(
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) corner: vec3<f32>,
    @location(3) color: vec4<f32>,
    @location(4) pattern: vec4<f32>,
    @location(5) dash: vec4<f32>,
) -> LineOut {
    var out = line_out(a, b, corner, color, pattern, dash, 0.0);
    let ca = g.proj * lifted(vec4<f32>(a, 1.0), corner.z);
    let cb = g.proj * lifted(vec4<f32>(b, 1.0), corner.z);
    out.clip = wide(ca, cb, corner.xy, corner.z, true);
    if (g.proj[3][3] == 0.0) {
        out.clip.z *= 1.0 + AXIS_LIFT;
    } else {
        out.clip.z += FACE_STEPS / 16777216.0 * out.clip.w;
    }
    return out;
}

// A box's outline (instanced): the edges of the unit cube placed by `m0..m3` (to camera
// space), its dash `pattern` (px), `params`: half its width (px), how far from its corners its
// edges are drawn (px, 0: whole), whether the edges of its faces turned to the eye are drawn
// again over the scene, whether it is drawn wholly over the scene. 24 quads of 6 corners: each
// edge as two halves run from its ends to its middle, each anchored with a dash at its corner
// and half a dash at the middle, so a dash starts at every corner and one lies across the
// middle.
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
    // The edge's two faces: across each other axis, on the side of its corners. Written out
    // rather than looped over: DX12's FXC fails on the loop (X3511, "forced to unroll loop").
    if (edge_face_exposed(model, e.x, axis, 0u)
        || edge_face_exposed(model, e.x, axis, 1u)
        || edge_face_exposed(model, e.x, axis, 2u)) {
        return box_edge(in);
    }
    return no_line();
}

// Whether the face across axis `k` on the side of `corner` is one of those of the edge along
// `axis` (a single bit) through that corner, and turned to the eye.
fn edge_face_exposed(model: mat4x4<f32>, corner: u32, axis: u32, k: u32) -> bool {
    return (1u << k) != axis && face_exposed(model, k, (corner >> k) & 1u);
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
    return line_out(a, b, vec3<f32>(quad[in.vi % 6u], in.params.x), in.color, in.pattern, vec4<f32>(LAY_ANCHORED, 0.5, 0.0, 0.0), in.params.y);
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

// A corner of a coloured segment's quad, `a` and `b` in camera space, drawn only `reach` px
// from `a` when not 0. Its dash `pattern` lies as `dash.x` says (`LAY_*`):
// - in model units (`pattern` too, its period a power of two), laid on the line itself, so
//   that its dashes stay where they are on it as the view orbits; `dash.yz` its phase at the
//   line's nearer end (high and low parts), `dash.w` the line's length, negative when that end
//   is `b` (see `ink`);
// - anchored (`pattern` in px), laid on the line too: a dash at `a`, `dash.y` of a dash at `b`
//   (1: a line's last dash, ½: a box's half edge, its other half beyond), the pattern
//   stretched a little so that a whole number of periods fits the line on screen;
// - on screen (`pattern` in px), at `dash.y` px at `a`: dashes as long on screen everywhere,
//   held still as the view pans, sliding along the line as it orbits (the guides').
fn line_out(a: vec3<f32>, b: vec3<f32>, corner: vec3<f32>, color: vec4<f32>, pattern: vec4<f32>, dash: vec4<f32>, reach: f32) -> LineOut {
    var out: LineOut;
    let ca = g.proj * lifted(vec4<f32>(a, 1.0), corner.z);
    let cb = g.proj * lifted(vec4<f32>(b, 1.0), corner.z);
    out.clip = wide(ca, cb, corner.xy, corner.z, false);
    out.color = color;
    out.pattern = pattern;
    out.reach = reach;
    out.along = along_px(ca, cb, corner.xy, corner.z);
    out.lay = vec3<f32>(0.0, 0.0, dash.x);
    out.laid = 0.0;
    let period = dot(pattern, vec4<f32>(1.0));
    if (period <= 0.0) {
        return out;
    }
    if (dash.x == LAY_ANCHORED) {
        // Whole periods and the tail: as long as the line (its number of periods set on
        // screen, so it changes only as the line grows or shrinks there by a period).
        let len = screen_length(ca, cb);
        let tail = pattern.x * dash.y;
        let k = max(round((len - tail) / period), 0.0);
        let span = k * period + tail;
        out.pattern = select(vec4<f32>(0.0), pattern * (len / span), span > 0.0 && len > 0.0);
        // 0 at `a`, the length at `b`: the quad's half width past each end shrinks it by
        // as much, which leaves the ends inked.
        out.laid = select(0.0, len, corner.x > 0.5);
    } else if (dash.x == LAY_SCREEN) {
        out.lay.x = dash.y;
    } else {
        // From the nearer end, 0 there: small where a model unit is few pixels, so that the
        // float keeps the dashes there; the end's phase is added in `ink`, exactly.
        let at_a = min(dash.w, 0.0);
        let at_b = max(dash.w, 0.0);
        // The part in front of the eye.
        let t = in_front_t(ca, cb);
        out.laid = mix(at_a, at_b, select(t.x, t.y, corner.x > 0.5));
        out.lay = vec3<f32>(dash.yz, LAY_MODEL);
    }
    return out;
}

// Where the part of a segment in front of the eye starts and ends along it (0: `a`, 1: `b`),
// as `in_front` cuts it.
fn in_front_t(ca: vec4<f32>, cb: vec4<f32>) -> vec2<f32> {
    let near = 1e-4;
    if (ca.w < near && cb.w >= near) {
        return vec2<f32>((near - ca.w) / (cb.w - ca.w), 1.0);
    }
    if (cb.w < near && ca.w >= near) {
        return vec2<f32>(0.0, 1.0 - (near - cb.w) / (ca.w - cb.w));
    }
    return vec2<f32>(0.0, 1.0);
}

// `x` modulo `2^n`, exactly (scaled by powers of two, a float loses nothing).
fn wrap(x: f32, n: i32) -> f32 {
    return x - floor(x * ldexp(1.0, -n)) * ldexp(1.0, n);
}

// The model units a pixel spans along the line (the laid distance's change from pixel to
// pixel; taken before any branch, derivatives need every pixel of the quad).
fn laid_per_px(in: LineOut) -> f32 {
    return length(vec2<f32>(dpdx(in.laid), dpdy(in.laid)));
}

// How much of a line's fragment is inked: 0 in a gap of its dashes or past its reach, 1 on a
// dash. In model units, where the period falls under a few pixels on screen (towards a
// vanishing point), the dashes fade into a line inked as much as they ink it (their share of
// the period), rather than shimmer.
fn ink(in: LineOut, per_px: f32) -> f32 {
    if (in.reach > 0.0 && in.along > in.reach) {
        return 0.0;
    }
    let period = dot(in.pattern, vec4<f32>(1.0));
    if (period <= 0.0) {
        return 1.0;
    }
    let p = in.pattern;
    var t: f32;
    var fade = 1.0;
    if (in.lay.z == LAY_MODEL) {
        let n = i32(round(log2(period)));
        t = wrap(wrap(in.lay.x, n) + wrap(in.lay.y, n) + in.laid, n);
        if (per_px > 0.0) {
            fade = smoothstep(3.0, 6.0, period / per_px);
        }
    } else {
        let at = select(in.laid, in.lay.x + in.along, in.lay.z == LAY_SCREEN);
        t = at - floor(at / period) * period;
    }
    let on = t < p.x || (t >= p.x + p.y && t < p.x + p.y + p.z);
    let share = (p.x + p.z) / period;
    return mix(share, select(0.0, 1.0, on), fade);
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let k = ink(in, laid_per_px(in));
    if (k <= 0.0) {
        discard;
    }
    return vec4<f32>(mix(g.background.rgb, in.color.rgb, in.color.a * k), 1.0);
}

// The overlay's lines: blended over what is drawn under them (their alpha is a true opacity).
@fragment
fn fs_overlay(in: LineOut) -> @location(0) vec4<f32> {
    let k = ink(in, laid_per_px(in));
    if (k <= 0.0) {
        discard;
    }
    return vec4<f32>(in.color.rgb, in.color.a * k);
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
