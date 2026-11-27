//! The stretch (docs/tools/stretch.md) on random shapes, placements and stretches, each result held
//! to invariants that need no expected value: a stretch either refuses ([`StretchError`]) or writes
//! a solid the kernel reads back, whose length along the axis grew by the moves, whose volume moved
//! the way the moves did, which meshes like its volume, which still cuts where the original cut, and
//! which the opposite stretch brings back to the original.
//!
//! The numbers are drawn, not round: a sum that is exact on `300 + 100` can be one ulp off on a
//! user's `-395.61269526791114`. Every case is replayable from its seed:
//! `STRETCH_FUZZ_SEED=<seed>` runs that one case, `STRETCH_FUZZ_CASES=<n>` runs more of them, and
//! `STRETCH_FUZZ_DUMP=<dir>` writes each case's stretch job (`<seed>.json`, as the journal's
//! failures hold it) to make a fixture of it.

use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use glam::{DAffine3, DMat3, DQuat, DVec2, DVec3};
use skraft_kernel::names::{Blend, NodeOp, TreeBody, TreeNode};
use skraft_kernel::profile::{self, Corner};
use skraft_kernel::{BooleanOp, Job, Output, Primitive, Solid, Stretch, StretchError, Task, run};

/// Cases of a plain `cargo test`; a few seconds.
const CASES: u64 = 120;
const FIRST_SEED: u64 = 0x5EED_0000;

/// SplitMix64: small, seedable, the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `lo..hi`.
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, p: f64) -> bool {
        self.f(0.0, 1.0) < p
    }
}

/// Volume enclosed by the display mesh.
fn mesh_volume(s: &Solid) -> f64 {
    let m = s.mesh().unwrap();
    let p = |i: u32| DVec3::from_array(m.positions[i as usize].map(f64::from)) + m.origin;
    m.indices.chunks(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum::<f64>() / 6.0
}

fn rectangle(l: f64, w: f64) -> Vec<Corner> {
    profile::polygon(&[DVec2::ZERO, DVec2::new(l, 0.0), DVec2::new(l, w), DVec2::new(0.0, w)])
}

/// The board `l × w × t` with some of its top edges blended as the app's body node does.
fn blended(l: f64, w: f64, t: f64, blend: Blend, size: f64, picks: Vec<[f64; 3]>) -> Option<Solid> {
    let mut p = rectangle(l, w);
    profile::settle_ids(&mut p);
    let b = Primitive::Prism { origin: DVec3::ZERO, profile: p, height: t };
    let solid = Arc::new(b.build().ok()?);
    let leaf = TreeBody {
        id: 1,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Leaf { solid: 0, recipe: Some(b) },
    };
    let job =
        Job { inputs: vec![solid], tasks: vec![Task::Node { children: vec![leaf], op: NodeOp::blend_of(blend, size, Vec::new()), picks }] };
    match run(&job).ok()?.pop() {
        Some(Output::Node { solid, refusal: None, failed, .. }) if failed.is_empty() => Some(solid),
        _ => None,
    }
}

/// A random shape in its own frame, from the origin, and what it is.
fn shape(rng: &mut Rng) -> (String, Solid) {
    let (l, w, t) = (rng.f(40.0, 700.0), rng.f(30.0, 400.0), rng.f(6.0, 60.0));
    let board = || Solid::new_box(DVec3::ZERO, DVec3::new(l, w, t)).unwrap();
    let what = format!("{l} × {w} × {t}");
    match rng.pick(10) {
        0 => (format!("board {what}"), board()),
        1 => {
            let r = rng.f(1.0, 0.45 * l.min(w));
            (
                format!("board {what}, corners filleted {r}"),
                Solid::new_prism(DVec3::ZERO, &profile::with_fillets(&rectangle(l, w), r), t).unwrap(),
            )
        }
        2 => {
            // One side an arc, bulging out or in.
            let bulge = rng.f(0.05, 0.6) * if rng.chance(0.5) { 1.0 } else { -1.0 };
            let mut p = rectangle(l, w);
            p[1] = Corner::arc(p[1].p, bulge);
            match Solid::new_prism(DVec3::ZERO, &p, t) {
                Ok(s) => (format!("board {what}, an arc side of bulge {bulge}"), s),
                Err(_) => (format!("board {what}"), board()), // an arc deeper than the board
            }
        }
        3 => {
            let r = rng.f(1.0, 0.2 * l.min(w));
            let at = DVec3::new(rng.f(r + 1.0, l - r - 1.0), rng.f(r + 1.0, w - r - 1.0), -5.0);
            let hole = Solid::new_cylinder(at, r, t + 10.0).unwrap();
            (format!("board {what}, drilled Ø{} at {at}", 2.0 * r), board().boolean(&hole, BooleanOp::Difference).unwrap())
        }
        4 => {
            // A blind bore along the length, from the near end.
            let r = rng.f(1.0, 0.4 * t.min(w));
            let depth = rng.f(0.1, 0.9) * l;
            let bore = Solid::new_cylinder(DVec3::ZERO, r, depth + 5.0)
                .unwrap()
                .transformed(&(DAffine3::from_translation(DVec3::new(-5.0, w / 2.0, t / 2.0)) * DAffine3::from_rotation_y(FRAC_PI_2)))
                .unwrap();
            (format!("board {what}, bored Ø{} over {depth}", 2.0 * r), board().boolean(&bore, BooleanOp::Difference).unwrap())
        }
        5 => {
            // A concave arc carved by a large cylinder across one long side.
            let r = rng.f(0.2, 0.8) * l;
            let x = rng.f(0.2, 0.8) * l;
            let y = w + r - rng.f(0.1, 0.6) * w;
            let cylinder = Solid::new_cylinder(DVec3::new(x, y, -5.0), r, t + 10.0).unwrap();
            match board().boolean(&cylinder, BooleanOp::Difference) {
                Ok(s) if s.pieces() == 1 => (format!("board {what}, carved by R{r} at ({x}, {y})"), s),
                _ => (format!("board {what}"), board()),
            }
        }
        6 => {
            let (r, h) = (rng.f(2.0, 200.0), rng.f(5.0, 700.0));
            (format!("cylinder R{r} × {h}"), Solid::new_cylinder(DVec3::ZERO, r, h).unwrap())
        }
        7 => {
            let r = rng.f(2.0, 200.0);
            (format!("sphere R{r}"), Solid::new_sphere(DVec3::ZERO, r).unwrap())
        }
        k => {
            let blend = if k == 8 { Blend::Round } else { Blend::Chamfer };
            let size = rng.f(0.5, 0.45 * t.min(w));
            let all = [[l / 2.0, 0.0, t], [l, w / 2.0, t], [l / 2.0, w, t], [0.0, w / 2.0, t]];
            let picks: Vec<[f64; 3]> = all.iter().copied().filter(|_| rng.chance(0.6)).collect();
            let picks = if picks.is_empty() { vec![all[0]] } else { picks };
            match blended(l, w, t, blend, size, picks.clone()) {
                Some(s) => (format!("board {what}, {blend:?} {size} on {} top edges", picks.len()), s),
                None => (format!("board {what}"), board()),
            }
        }
    }
}

/// A random placement: square turns (the kernel's rounding in them), any turn, a mirror.
fn placement(rng: &mut Rng) -> DAffine3 {
    let turn = match rng.pick(4) {
        0 => DQuat::IDENTITY,
        1 => DQuat::from_rotation_z(FRAC_PI_2 * rng.pick(4) as f64) * DQuat::from_rotation_x(FRAC_PI_2 * rng.pick(4) as f64),
        2 => DQuat::from_rotation_z(rng.f(-PI, PI)),
        _ => DQuat::from_rotation_z(rng.f(-PI, PI)) * DQuat::from_rotation_x(rng.f(-PI, PI)),
    };
    let mirror = if rng.chance(0.25) { DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)) } else { DMat3::IDENTITY };
    let shift = DVec3::new(rng.f(-800.0, 800.0), rng.f(-800.0, 800.0), rng.f(-800.0, 800.0));
    DAffine3::from_mat3_translation(DMat3::from_quat(turn) * mirror, shift)
}

/// Known: a curved group is scaled about its extent along the axis, taken from its net's hull,
/// looser than a sphere where the axis is not along its net's: its ends do not land where their
/// sections move them, and the way back, scaling about another hull, is not the inverse
/// (docs/REVIEW.md).
const HULL_PIVOTS: &str = "scale pivots from the net's hull";

/// Known, SolidCore's: a sphere through a symmetric reflection comes out inside out
/// (`tests/solidcore_quirks.rs`).
const SPHERE_INSIDE_OUT: &str = "reflected sphere inside out";

/// What one case did.
enum Outcome {
    Stretched,
    /// A failure of a known family, not fixed yet: counted and shown, not failed.
    Known(&'static str),
    Refused(String),
    Failed(String),
}

/// The room the tool keeps between the matter of two sections a compression brings together (mm),
/// as its `move_range` (crates/tools/src/tools/stretch.rs): `apply` itself does not check it.
const KEEP: f64 = 1.0;

/// One case: a shape stretched along one of its own axes (now and then any axis), as its placed
/// record or in its own frame (an occurrence's definition), checked. Everything is measured in
/// the frame the stretch ran in, the result read back as is: the kernel's transform has quirks of
/// its own (a turn one ulp off square refuses some conics).
fn case(seed: u64) -> (String, Outcome) {
    let mut rng = Rng(seed);
    let (what, base) = shape(&mut rng);
    let place = placement(&mut rng);
    let own_axis = rng.pick(3);
    let aligned = rng.chance(0.85);
    let mut axis = place.matrix3 * DVec3::AXES[own_axis];
    if !aligned {
        axis = DVec3::new(rng.f(-1.0, 1.0), rng.f(-1.0, 1.0), rng.f(-1.0, 1.0));
    }
    if rng.chance(0.3) {
        // The noise of axes read from a turned frame (`(0, −1, 2.2e-16)`).
        axis += DVec3::new(rng.f(-1.0, 1.0), rng.f(-1.0, 1.0), rng.f(-1.0, 1.0)) * 4e-16;
    }
    let in_own_frame = rng.chance(0.5);
    let placed = base.transformed(&place).unwrap();
    let target = if in_own_frame { base.clone() } else { placed.clone() };
    let frame = |s: Stretch| if in_own_frame { s.in_frame(&place) } else { s };

    let Some((lo, hi)) = Stretch::new(axis, vec![0.0], vec![0.0, 0.0]).unwrap().extent(&placed) else {
        return (what, Outcome::Failed("no extent".into()));
    };
    let len = hi - lo;
    // Cuts inside; moves increasing (lengthening: compression is then the way back), or one
    // compression within the room the tool leaves.
    let lengthen = rng.chance(0.75);
    let count = if lengthen && rng.chance(0.2) { 2 } else { 1 };
    let mut cuts: Vec<f64> = (0..count).map(|_| lo + len * rng.f(0.1, 0.9)).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| *a - *b < 1.0);
    let mut moves = vec![if rng.chance(0.6) { 0.0 } else { rng.f(-300.0, 300.0) }];
    if lengthen {
        for _ in &cuts {
            moves.push(moves.last().unwrap() + rng.f(0.0, 1.5) * len);
        }
    } else {
        let spans = frame(Stretch::new(axis, cuts.clone(), vec![0.0, 0.0]).unwrap()).section_spans(&target);
        let room = match spans[..] {
            [Some((_, a_hi)), Some((b_lo, _))] => b_lo - a_hi - KEEP,
            _ => 0.6 * len,
        };
        if room <= 0.0 {
            return (what, Outcome::Refused("no room to compress".into()));
        }
        moves.push(moves[0] - rng.f(0.0, 1.0) * room);
    }
    // What the ends of the matter move apart, once its true ends are known (the cuts are drawn
    // within a conservative extent, and may miss the matter).
    let mut growth = moves.last().unwrap() - moves[0];
    let stretch = frame(Stretch::new(axis, cuts.clone(), moves.clone()).unwrap());
    let what = format!(
        "{what}; placed {place:?}{}; along {axis:?}; cuts {cuts:?}, moves {moves:?}",
        if in_own_frame { ", stretched in its own frame" } else { "" }
    );
    let fail = |why: String| Outcome::Failed(why);
    if let Ok(dir) = std::env::var("STRETCH_FUZZ_DUMP") {
        let job = Job { inputs: vec![Arc::new(target.clone())], tasks: vec![Task::Stretch { solid: 0, stretch: stretch.clone() }] };
        std::fs::write(format!("{dir}/{seed:#x}.json"), serde_json::to_string(&job).unwrap()).unwrap();
    }

    let out = match stretch.apply(&target) {
        Ok(s) => s,
        Err(e @ StretchError::Unreadable(_)) => return (what, fail(format!("stretch: {e}"))),
        Err(e) => return (what, Outcome::Refused(format!("{e:?}"))),
    };
    // Read back by the kernel, as the document keeps it.
    let out = match out.transformed(&DAffine3::IDENTITY) {
        Ok(s) => s,
        Err(e) => return (what, fail(format!("read back: {e}"))),
    };
    let v0 = target.volume().unwrap();
    if v0 < 0.0 {
        return (what, Outcome::Known(SPHERE_INSIDE_OUT));
    }
    let v = match out.volume() {
        Ok(v) => v,
        Err(e) => return (what, fail(format!("volume: {e}"))),
    };
    if v <= 0.0 || (growth > 0.0 && v < v0 * (1.0 - 1e-9)) || (growth < 0.0 && v > v0 * (1.0 + 1e-9)) {
        return (what, fail(format!("volume {v0} → {v} for a growth of {growth}")));
    }
    match out.mesh() {
        Ok(_) => {
            let m = mesh_volume(&out);
            if (m - v).abs() > 2e-2 * v {
                return (what, fail(format!("mesh volume {m} ≠ volume {v}")));
            }
        }
        Err(e) => return (what, fail(format!("mesh: {e}"))),
    }
    // Longer by the growth, as the kernel's exact box measures it, when the axis is one of the
    // frame's (the stretch's own extents are conservative on NURBS, which the kernel rewrites).
    let (t0, t1) = (target.bounds().unwrap(), out.bounds().unwrap());
    if let Some(k) = (0..3).find(|&k| stretch.axis()[k].abs() > 1.0 - 1e-12) {
        let (e0, e1) = (stretch.axis()[k] * t0.0[k], stretch.axis()[k] * t0.1[k]);
        growth = stretch.move_at(e0.max(e1)) - stretch.move_at(e0.min(e1));
        let (l0, l1) = ((t0.1 - t0.0)[k], (t1.1 - t1.0)[k]);
        if (l1 - l0 - growth).abs() > 1e-6 * (1.0 + l0) {
            return (what, fail(format!("length {l0} → {l1}, not grown by {growth}")));
        }
    }
    // Still an operand: cut by a box over its lower half, where the original also cuts.
    let half = |(lo, hi): (DVec3, DVec3)| {
        Solid::new_box(lo - DVec3::splat(5.0), (hi - lo) * DVec3::new(1.0, 1.0, 0.5) + DVec3::new(10.0, 10.0, 5.0)).unwrap()
    };
    if let Err(e) = out.boolean(&half(t1), BooleanOp::Difference) {
        if target.boolean(&half(t0), BooleanOp::Difference).is_ok() {
            return (what, fail(format!("boolean: {e}")));
        }
    }
    // The way back: the moves reversed, each cut where it crosses again what it crossed. In what it
    // lengthened, but not anywhere: a curved group scaled by it is crossed again only around the
    // image of the cut, the faces of the group lying past it otherwise only translated (ADR 0058).
    // Tried at the middle first, then along; one way back that gives the original is enough.
    if lengthen {
        let back_moves: Vec<f64> = moves.iter().map(|m| -m).collect();
        let mut first = None;
        for f in std::iter::once(0.5).chain((1..16).map(|i| i as f64 / 16.0)) {
            let back_cuts: Vec<f64> = cuts.iter().enumerate().map(|(i, c)| c + moves[i] + f * (moves[i + 1] - moves[i])).collect();
            let Ok(back) = Stretch::new(axis, back_cuts, back_moves.clone()) else { continue };
            match frame(back).apply(&out) {
                Ok(s) => {
                    let s = match s.transformed(&DAffine3::IDENTITY) {
                        Ok(s) => s,
                        Err(e) => return (what, fail(format!("way back at {f}, read back: {e}"))),
                    };
                    let v2 = s.volume().unwrap();
                    let t2 = s.bounds().unwrap();
                    let off = (t2.0 - t0.0).abs().max((t2.1 - t0.1).abs()).max_element();
                    if (v2 - v0).abs() <= 1e-6 * v0 && off <= 1e-6 * (1.0 + len) {
                        return (what, Outcome::Stretched);
                    }
                    first.get_or_insert(format!("way back: volume {v0} → {v2}, bounds off by {off}"));
                }
                Err(e @ StretchError::Unreadable(_)) => return (what, fail(format!("way back at {f}: {e}"))),
                Err(_) => {}
            }
        }
        if let Some(why) = first {
            let outcome = if what.starts_with("sphere") { Outcome::Known(HULL_PIVOTS) } else { fail(why) };
            return (what, outcome);
        }
    }
    (what, Outcome::Stretched)
}

fn run_cases(seeds: impl Iterator<Item = u64>) {
    let (mut stretched, mut refused, mut known, mut failed) =
        (0, std::collections::BTreeMap::<String, usize>::new(), std::collections::BTreeMap::<&str, usize>::new(), Vec::new());
    for seed in seeds {
        let (what, outcome) = std::panic::catch_unwind(|| case(seed)).unwrap_or_else(|p| {
            let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
            (String::new(), Outcome::Failed(format!("panic: {}", why.unwrap_or_default())))
        });
        match outcome {
            Outcome::Stretched => stretched += 1,
            Outcome::Known(why) => *known.entry(why).or_default() += 1,
            Outcome::Refused(why) => *refused.entry(why.split('(').next().unwrap_or_default().to_string()).or_default() += 1,
            Outcome::Failed(why) => failed.push(format!("seed {seed:#x}: {why}\n    {what}")),
        }
    }
    println!("stretched {stretched}, refused {refused:?}, known {known:?}, failed {}", failed.len());
    assert!(failed.is_empty(), "{} failed (replay one with STRETCH_FUZZ_SEED):\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn random_stretches_hold_their_invariants() {
    if let Some(seed) = std::env::var("STRETCH_FUZZ_SEED").ok().and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()) {
        return run_cases(std::iter::once(seed));
    }
    let cases = std::env::var("STRETCH_FUZZ_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(CASES);
    run_cases(FIRST_SEED..FIRST_SEED + cases);
}
