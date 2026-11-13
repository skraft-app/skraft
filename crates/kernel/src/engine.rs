//! The kernel itself (feature `engine`): SolidCore calls on records, [`run`] for a whole
//! [`Job`], and the services that run jobs inline or on a thread.

use glam::{DAffine3, DVec2, DVec3};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::primitive::api_primitives as prim;
use solidcore_record::brep::Brep as Record;

use solidcore::kernel::CancelToken;
use solidcore_ops::cancel;

use crate::job::{Display, EARLIER, Job, JobId, JobResult, KernelService, Output, Progress, Quality, Task};
use crate::{BooleanOp, Error, Mesh, Primitive, Result, Snaps, Solid, Stretch};

impl Primitive {
    pub fn build(&self) -> Result<Solid> {
        match *self {
            Primitive::Prism { origin, ref profile, height } => Solid::new_prism(origin, profile, height),
            Primitive::Box { origin, size } => Solid::new_box(origin, size),
            Primitive::Cylinder { origin, radius, height } => Solid::new_cylinder(origin, radius, height),
            Primitive::Sphere { center, radius } => Solid::new_sphere(center, radius),
            Primitive::Cone { origin, base_radius, top_radius, height } => Solid::new_cone(origin, base_radius, top_radius, height),
            Primitive::Torus { center, major_radius, minor_radius } => Solid::new_torus(center, major_radius, minor_radius),
        }
    }
}

impl Solid {
    /// A box with one corner at `origin` and the given extents along X, Y, Z.
    pub fn new_box(origin: DVec3, size: DVec3) -> Result<Self> {
        create("box", |k| prim::create_box(k, p3(origin), size.x, size.y, size.z))
    }

    /// A cylinder standing on the XY plane at `origin`, along +Z.
    pub fn new_cylinder(origin: DVec3, radius: f64, height: f64) -> Result<Self> {
        create("cylinder", |k| prim::create_cylinder(k, p3(origin), radius, height))
    }

    /// A sphere centred at `center`.
    pub fn new_sphere(center: DVec3, radius: f64) -> Result<Self> {
        create("sphere", |k| prim::create_sphere(k, p3(center), radius))
    }

    /// A cone (or truncated cone) standing on the XY plane at `origin`, along +Z.
    pub fn new_cone(origin: DVec3, base_radius: f64, top_radius: f64, height: f64) -> Result<Self> {
        create("cone", |k| prim::create_cone(k, p3(origin), base_radius, top_radius, height))
    }

    /// The outline `profile` (corners in the XY plane through `origin`, relative to it, each
    /// with the bulge of its side) extruded along +Z by `height`: SolidCore's planar region of
    /// lines and arcs swept between two levels. Sides of no length and straight corners are
    /// dropped; an outline that is not simple is refused.
    pub fn new_prism(origin: DVec3, profile: &[crate::profile::Corner], height: f64) -> Result<Self> {
        use crate::profile::{self as pr, Side};
        use solidcore::prism::profile::{Loop2, Region2, Seg2};
        let p = pr::cleaned(profile);
        if !pr::is_simple(&p) || !(height.is_finite() && height > 0.0) {
            return Err(Error::Kernel { call: "prism".into(), message: "profile not a simple outline, or no height".into() });
        }
        // The outer loop counter-clockwise.
        let p = pr::counter_clockwise(&p);
        let pt = |v: DVec2| solidcore::Point2::new(v.x, v.y);
        let segs = pr::sides(&p)
            .into_iter()
            .map(|s| match s {
                Side::Line { a, b } => Seg2::Line { a: pt(a), b: pt(b) },
                Side::Arc { c, r, start, sweep, .. } => Seg2::Arc { center: pt(c), radius: r, start, sweep },
            })
            .collect();
        let region = Region2 { outer: Loop2 { segs }, holes: Vec::new() };
        let frame = solidcore::eval::Frame { origin: origin.to_array(), x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
        create("prism", |k| solidcore::prism::extrude::extrude_region(k, &frame, &region, 0.0, height))
    }

    /// A torus centred at `center`, around +Z.
    pub fn new_torus(center: DVec3, major_radius: f64, minor_radius: f64) -> Result<Self> {
        create("torus", |k| prim::create_torus(k, p3(center), major_radius, minor_radius))
    }

    /// This solid stretched (docs/ARCHITECTURE.md §6 sexies): its record rewritten, then read
    /// back by the kernel, which checks it.
    pub fn stretched(&self, stretch: &Stretch) -> Result<Self> {
        let record = stretch.apply(self)?.record().clone();
        guarded("stretch", || {
            let mut k = kernel();
            let mut body = to_kernel(&mut k, &record, "stretch")?;
            to_record(&mut k, &mut body, "stretch")
        })
    }

    /// This solid moved by the rigid (or uniformly scaled) transform `t`.
    pub fn transformed(&self, t: &DAffine3) -> Result<Self> {
        let m = row_major(t);
        guarded("transform", || {
            let mut k = kernel();
            let mut body = to_kernel(&mut k, &self.record, "transform")?;
            solidcore::topology::brep::mirror::transform_matrix(&mut k, &mut body, &m, false).map_err(|e| kernel_err("transform", e))?;
            to_record(&mut k, &mut body, "transform")
        })
    }

    /// `self op other`, both in the same coordinate system.
    pub fn boolean(&self, other: &Solid, op: BooleanOp) -> Result<Self> {
        let call = op.call();
        guarded(call, || {
            use solidcore::boolean::api_brep as b;
            let mut k = kernel();
            let a = to_kernel(&mut k, &self.record, call)?;
            let o = to_kernel(&mut k, &other.record, call)?;
            let r = match op {
                BooleanOp::Union => b::boolean_union(&mut k, a, o),
                BooleanOp::Difference => b::boolean_difference(&mut k, a, o),
                BooleanOp::Intersection => b::boolean_intersection(&mut k, a, o),
            };
            let mut body = r.map_err(|e| kernel_err(call, e))?;
            // Nothing left (a tool swallowing the target): said so, before the conversion
            // refuses a body without shells.
            if solidcore::topology::brep::p1::num_faces(&body) == 0 {
                return Err(Error::Empty(call.into()));
            }
            let out = to_record(&mut k, &mut body, call)?;
            if out.record.faces.is_empty() {
                return Err(Error::Empty(call.into()));
            }
            Ok(out)
        })
    }

    /// The solid's exact bounding box, in its own coordinates (the extremes of its faces, not
    /// only of its vertices — a cylinder has a vertex per circle): SolidCore's tight box, its
    /// loose one (which always holds the solid, larger once turned) should the tight one fail.
    pub fn bounds(&self) -> Result<(DVec3, DVec3)> {
        guarded("bounds", || {
            use solidcore::query::api_queries::brep_bounding_box;
            let mut k = kernel();
            let b = to_kernel(&mut k, &self.record, "bounds")?;
            let corners = |e: solidcore::geom::extent3d::Extent3d| {
                (DVec3::new(e.min_at(0), e.min_at(1), e.min_at(2)), DVec3::new(e.max_at(0), e.max_at(1), e.max_at(2)))
            };
            let loose = brep_bounding_box(&k, &b, false).map_err(|e| kernel_err("bounds", e))?;
            if !loose.is_defined() {
                return Err(Error::Empty("bounds".into()));
            }
            Ok(brep_bounding_box(&k, &b, true).ok().filter(|t| t.is_defined()).map_or_else(|| corners(loose), corners))
        })
    }

    /// A display mesh of the solid, in its own coordinates.
    pub fn mesh(&self) -> Result<Mesh> {
        self.display().map(|d| d.mesh)
    }

    /// The display mesh and the snap features (what tools infer on), in the solid's own
    /// coordinates: one tessellation for both.
    pub fn display(&self) -> Result<Display> {
        self.display_at(Quality::Final)
    }

    /// [`Solid::display`] at `quality`: a preview is meshed at a hundredth of the body
    /// (`solidcore_tess::preview`), in a fraction of the time.
    pub fn display_at(&self, quality: Quality) -> Result<Display> {
        let bounds = self.bounds().ok();
        let size = bounds.or_else(|| self.vertex_bounds()).map(|(lo, hi)| (hi - lo).length()).filter(|s| *s > 1e-6).unwrap_or(100.0);
        guarded("mesh", || {
            let tol = match quality {
                Quality::Preview => solidcore_tess::preview::tolerance(size, 1),
                Quality::Final => solidcore_tess::MeshTolerance::display_of(size),
            };
            let (mut plan, stats) = solidcore_tess::plan(&self.record, &tol, size).map_err(|e| Error::Mesh(e.to_string()))?;
            // Faces the planner refined to its bound without reaching the tolerance (SolidCore
            // on some splines, docs/REVIEW.md "Work: SolidCore"): tens of thousands of
            // triangles each, no closer for them. The body planned again at the sag they did
            // reach, the facet angle still keeping small radii round.
            let reached = stats
                .over_tolerance
                .iter()
                .filter(|o| o.caps.iter().any(|c| matches!(c, solidcore_tess::Cap::Refine)))
                .map(|o| o.sag)
                .fold(0.0, f64::max);
            if quality == Quality::Final
                && reached > tol.chord
                && let Ok((again, _)) = solidcore_tess::plan(&self.record, &solidcore_tess::MeshTolerance::display(reached * 1.25), size)
            {
                plan = again;
            }
            let (m, _refused) = plan.evaluate();
            let visible: Vec<bool> = (0..m.edges.len()).map(|i| edge_drawn(&self.record, &m, i)).collect();
            let polylines: Vec<Vec<DVec3>> = m.edges.iter().map(|e| e.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect()).collect();
            Ok(Display { mesh: Mesh::from_tess(&m, &visible), snaps: Snaps::build(&self.record, &polylines, &visible), bounds })
        })
    }
}

// ---- jobs -----------------------------------------------------------------------------------

/// The solid a task input names: one of the job's inputs, or what an earlier task produced.
fn input<'a>(job: &'a Job, out: &'a [Output], i: u32) -> Result<&'a Solid> {
    let found = if i & EARLIER != 0 {
        match out.get((i & !EARLIER) as usize) {
            Some(Output::Solid { solid, .. } | Output::Recipe { solid, .. }) => Some(solid),
            _ => None,
        }
    } else {
        job.inputs.get(i as usize).map(|s| &**s)
    };
    found.ok_or_else(|| Error::Convert { call: "job".into(), message: format!("no input {i:#x}") })
}

/// What a job runs with: where its progress goes, and the token that stops it.
pub struct Context<'a> {
    pub report: &'a (dyn Fn(Progress) + Sync),
    pub cancel: Option<CancelToken>,
}

impl Context<'_> {
    /// Nothing reported, nothing to stop.
    pub fn none() -> Context<'static> {
        Context { report: &|_| {}, cancel: None }
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(CancelToken::is_cancelled)
    }
}

/// Runs every task of `job` in order. A task producing a solid that fails fails the job.
pub fn run(job: &Job) -> JobResult {
    run_with(job, &Context::none())
}

/// [`run`], reporting its progress to `cx` and stopped by its token: every SolidCore call the
/// job makes, on this thread and on those it starts ([`parallel_map`]), polls it; a job
/// cancelled at any point fails with [`Error::Cancelled`].
pub fn run_with(job: &Job, cx: &Context) -> JobResult {
    let out = match &cx.cancel {
        Some(token) => cancel::within(token, || run_tasks(job, cx)),
        None => run_tasks(job, cx),
    };
    if cx.cancelled() { Err(Error::Cancelled) } else { out }
}

fn run_tasks(job: &Job, cx: &Context) -> JobResult {
    let solid = |s: Solid| {
        let display = s.display().map_err(|e| e.to_string());
        Output::Solid { solid: s, display }
    };
    let mut out = Vec::with_capacity(job.tasks.len());
    for task in &job.tasks {
        if cx.cancelled() {
            return Err(Error::Cancelled);
        }
        let input = |i: u32| input(job, &out, i);
        let done = match task {
            Task::Primitive { primitive, placement } => {
                let mut s = primitive.build()?;
                if !placement.abs_diff_eq(DAffine3::IDENTITY, 1e-12) {
                    s = s.transformed(placement)?;
                }
                solid(s)
            }
            Task::Transform { solid: i, transform } => solid(input(*i)?.transformed(transform)?),
            Task::Boolean { target, tool, tool_transform, op } => {
                let tool = input(*tool)?;
                let r = match tool_transform {
                    Some(t) => input(*target)?.boolean(&tool.transformed(t)?, *op)?,
                    None => input(*target)?.boolean(tool, *op)?,
                };
                solid(r)
            }
            Task::Recipe { blank, tools } => {
                let mut shape = input(*blank)?.clone();
                let mut failed = Vec::new();
                for (k, (tool, placement)) in tools.iter().enumerate() {
                    let tool = input(*tool)?;
                    let cut = if placement.abs_diff_eq(DAffine3::IDENTITY, 1e-12) {
                        shape.boolean(tool, BooleanOp::Difference)
                    } else {
                        tool.transformed(placement).and_then(|t| shape.boolean(&t, BooleanOp::Difference))
                    };
                    match cut {
                        Ok(s) => shape = s,
                        Err(_) => failed.push(k as u32),
                    }
                }
                let display = shape.display().map_err(|e| e.to_string());
                Output::Recipe { solid: shape, display, failed }
            }
            Task::Stretch { solid: i, stretch } => solid(input(*i)?.stretched(stretch)?),
            Task::Display { solid: i, quality } => Output::Display(input(*i)?.display_at(*quality).map_err(|e| e.to_string())),
            Task::ImportStep { bytes } => Output::Step(Box::new(crate::step::read::import(bytes, cx.report)?)),
            Task::Panic { message } => guarded("panic", || -> Result<Output> { panic!("{message}") })?,
        };
        out.push(done);
    }
    Ok(out)
}

/// Runs each job as it is submitted: results are ready at the next [`KernelService::poll`].
/// For tests, and wherever blocking is fine.
#[derive(Default)]
pub struct Inline {
    ready: Vec<(JobId, JobResult)>,
}

impl KernelService for Inline {
    fn submit(&mut self, id: JobId, job: Job) {
        self.ready.push((id, run(&job)));
    }

    fn poll(&mut self) -> Vec<(JobId, JobResult)> {
        std::mem::take(&mut self.ready)
    }
}

/// `f` over `items`, on every core where threads exist (each with [`crate::KERNEL_STACK`]), in
/// order; one after the other in the browser.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
        if threads > 1 {
            // The job's cancel token, over the calls of every thread.
            let token = cancel::current();
            let next = AtomicUsize::new(0);
            let mut done: Vec<(usize, R)> = std::thread::scope(|s| {
                let workers: Vec<_> = (0..threads)
                    .filter_map(|_| {
                        std::thread::Builder::new()
                            .stack_size(crate::KERNEL_STACK)
                            .spawn_scoped(s, || {
                                let work = || {
                                    let mut mine = Vec::new();
                                    loop {
                                        let i = next.fetch_add(1, Ordering::Relaxed);
                                        let Some(item) = items.get(i) else { break };
                                        mine.push((i, f(item)));
                                    }
                                    mine
                                };
                                match &token {
                                    Some(t) => cancel::within(t, work),
                                    None => work(),
                                }
                            })
                            .ok()
                    })
                    .collect();
                workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
            });
            // A worker that could not start or panicked left items: done here.
            let mut have = vec![false; items.len()];
            for (i, _) in &done {
                have[*i] = true;
            }
            for (i, item) in items.iter().enumerate().filter(|(i, _)| !have[*i]) {
                done.push((i, f(item)));
            }
            done.sort_unstable_by_key(|(i, _)| *i);
            return done.into_iter().map(|(_, r)| r).collect();
        }
    }
    items.iter().map(f).collect()
}

/// Runs jobs on a pool of threads, one per core (the desktop app): an operation's job before
/// the meshes asked in the background (a job of `Display` tasks only), each kind in the order
/// submitted. Results come back as each job ends.
#[cfg(not(target_arch = "wasm32"))]
pub struct Pool {
    shared: std::sync::Arc<PoolShared>,
    results: std::sync::mpsc::Receiver<(JobId, JobResult)>,
    /// To answer a job cancelled before it started.
    sender: std::sync::mpsc::Sender<(JobId, JobResult)>,
    waker: Option<crate::Waker>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PoolQueues {
    urgent: std::collections::VecDeque<(JobId, Job)>,
    background: std::collections::VecDeque<(JobId, Job)>,
    /// The jobs running, with their cancel tokens.
    running: std::collections::HashMap<JobId, CancelToken>,
    /// The latest progress of each job, since the service last took it.
    progress: std::collections::HashMap<JobId, Progress>,
    closed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl PoolQueues {
    /// The next job to run (an urgent one first), now running.
    fn next(&mut self) -> Option<(JobId, Job, CancelToken)> {
        let (id, job) = self.urgent.pop_front().or_else(|| self.background.pop_front())?;
        let token = CancelToken::new();
        self.running.insert(id, token.clone());
        Some((id, job, token))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PoolShared {
    queues: std::sync::Mutex<PoolQueues>,
    work: std::sync::Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
impl PoolShared {
    fn lock(&self) -> std::sync::MutexGuard<'_, PoolQueues> {
        // A thread panicking while it holds the lock leaves plain data: still usable.
        self.queues.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Pool {
    /// A thread per core.
    pub fn new(waker: Option<crate::Waker>) -> Self {
        Pool::with_threads(std::thread::available_parallelism().map_or(1, |n| n.get()), waker)
    }

    /// `threads` threads, each with SolidCore's stack ([`crate::KERNEL_STACK`]: it recurses
    /// legitimately, and a stack overflow aborts, it is no panic). None: nothing runs (tests).
    pub fn with_threads(threads: usize, waker: Option<crate::Waker>) -> Self {
        let shared = std::sync::Arc::new(PoolShared::default());
        let (sender, results) = std::sync::mpsc::channel();
        for n in 0..threads {
            let (shared, sender, waker) = (shared.clone(), sender.clone(), waker.clone());
            std::thread::Builder::new()
                .name(format!("kernel {n}"))
                .stack_size(crate::KERNEL_STACK)
                .spawn(move || pool_thread(&shared, &sender, waker.as_ref()))
                .expect("a kernel thread starts");
        }
        Pool { shared, results, sender, waker }
    }

    fn wake(&self) {
        if let Some(w) = &self.waker {
            w();
        }
    }
}

/// A pool thread: takes the next job (an urgent one first) until the pool is dropped.
#[cfg(not(target_arch = "wasm32"))]
fn pool_thread(shared: &PoolShared, sender: &std::sync::mpsc::Sender<(JobId, JobResult)>, waker: Option<&crate::Waker>) {
    let wake = || {
        if let Some(w) = waker {
            w();
        }
    };
    loop {
        let (id, job, token) = {
            let mut q = shared.lock();
            loop {
                if q.closed {
                    return;
                }
                if let Some((id, job, token)) = q.next() {
                    break (id, job, token);
                }
                q = shared.work.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let report = |p: Progress| {
            shared.lock().progress.insert(id, p);
            wake();
        };
        let result = run_with(&job, &Context { report: &report, cancel: Some(token) });
        {
            let mut q = shared.lock();
            q.running.remove(&id);
            q.progress.remove(&id);
        }
        if sender.send((id, result)).is_err() {
            return;
        }
        wake();
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl KernelService for Pool {
    fn submit(&mut self, id: JobId, job: Job) {
        let background = !job.tasks.is_empty() && job.tasks.iter().all(|t| matches!(t, Task::Display { .. }));
        let mut q = self.shared.lock();
        if background { q.background.push_back((id, job)) } else { q.urgent.push_back((id, job)) }
        drop(q);
        self.shared.work.notify_one();
    }

    fn poll(&mut self) -> Vec<(JobId, JobResult)> {
        self.results.try_iter().collect()
    }

    fn progress(&mut self) -> Vec<(JobId, Progress)> {
        self.shared.lock().progress.drain().collect()
    }

    fn cancel(&mut self, id: JobId) {
        let mut q = self.shared.lock();
        let queued = |d: &mut std::collections::VecDeque<(JobId, Job)>| d.iter().position(|(j, _)| *j == id).and_then(|i| d.remove(i));
        if queued(&mut q.urgent).or_else(|| queued(&mut q.background)).is_some() {
            drop(q);
            let _ = self.sender.send((id, Err(Error::Cancelled)));
            self.wake();
        } else if let Some(token) = q.running.get(&id) {
            token.cancel();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Pool {
    fn drop(&mut self) {
        // Every job left is stopped; the threads end with the job they run.
        let mut q = self.shared.lock();
        q.closed = true;
        q.urgent.clear();
        q.background.clear();
        for token in q.running.values() {
            token.cancel();
        }
        drop(q);
        self.shared.work.notify_all();
    }
}

// ---- helpers --------------------------------------------------------------------------------

/// Does the viewport draw edge `i` of `record`'s tessellation `m`? Not a seam: the mesher
/// leaves `seams` empty, so they are told from the record.
fn edge_drawn(record: &Record, m: &solidcore_tess::Mesh, i: usize) -> bool {
    m.shaded_edge_visible(i) && !m.seams.get(i).copied().unwrap_or(false) && !is_seam(record, i)
}

/// Is edge `i` a seam: where a face meets itself (a cylinder's side), or between two pieces
/// of one surface?
fn is_seam(record: &Record, i: usize) -> bool {
    let Some(e) = record.edges.get(i) else { return false };
    let [a, b] = match e.uses.as_slice() {
        [a, b] => [a.face, b.face],
        _ => return false,
    };
    if a == b {
        return true;
    }
    match (record.faces.get(a as usize), record.faces.get(b as usize)) {
        (Some(fa), Some(fb)) => fa.surface == fb.surface,
        _ => false,
    }
}

impl Mesh {
    /// `drawn[i]`: is edge `i` drawn.
    fn from_tess(m: &solidcore_tess::Mesh, drawn: &[bool]) -> Self {
        let d = |p: &solidcore_record::Vec3| DVec3::new(p.x, p.y, p.z);
        let origin = m
            .positions
            .iter()
            .map(d)
            .fold(None, |b: Option<(DVec3, DVec3)>, p| Some(b.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p)))))
            .map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        let rel = |p: &solidcore_record::Vec3| (d(p) - origin).as_vec3().to_array();
        let dir = |p: &solidcore_record::Vec3| d(p).as_vec3().to_array();
        let (edges, edge_ids) = m
            .edges
            .iter()
            .enumerate()
            .filter(|(i, _)| drawn.get(*i).copied().unwrap_or(false))
            .map(|(i, e)| (e.iter().map(rel).collect::<Vec<_>>(), i as u32))
            .unzip();
        Mesh {
            origin,
            positions: m.positions.iter().map(rel).collect(),
            normals: m.normals.iter().map(dir).collect(),
            indices: m.triangles.iter().flatten().copied().collect(),
            tri_face: m.tri_face.clone(),
            edges,
            edge_ids,
        }
    }
}

/// A fresh SolidCore kernel, stopped by the cancel token of the job running on this thread.
pub(crate) fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    if let Some(token) = cancel::current() {
        k.set_cancel_token(token);
    }
    k
}

fn p3(v: DVec3) -> solidcore::Point3 {
    solidcore::Point3::new(v.x, v.y, v.z)
}

/// SolidCore's matrix convention: row vectors, linear part in rows 0..3, translation in row 3.
fn row_major(t: &DAffine3) -> [[f64; 4]; 4] {
    let (x, y, z, w) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis, t.translation);
    [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [w.x, w.y, w.z, 1.0]]
}

fn kernel_err(call: &str, e: solidcore::SolidError) -> Error {
    if e.is_cancelled() {
        return Error::Cancelled;
    }
    Error::Kernel { call: call.into(), message: e.to_string() }
}

pub(crate) fn to_kernel(k: &mut Kernel, r: &Record, call: &str) -> Result<solidcore::Brep> {
    solidcore_convert::brep::to_kernel_body(k, r, true).map_err(|e| Error::Convert { call: call.into(), message: e.to_string() })
}

pub(crate) fn to_record(k: &mut Kernel, b: &mut solidcore::Brep, call: &str) -> Result<Solid> {
    solidcore_convert::brep::to_record(k, b)
        .map(|(mut record, _order)| {
            square_frames(&mut record);
            Solid::from_record(record)
        })
        .map_err(|e| Error::Convert { call: call.into(), message: e.to_string() })
}

/// Makes every frame of `r` square again. A rotation about a slanted axis leaves a plane's or
/// a frame's axes off square by ~1e-15 (`tests/solidcore_quirks.rs`), while SolidCore reads a
/// record back only within ~3e-16
/// (`surface::from_parameters`): the next operation on the solid would be refused ("the
/// plane's axes are not square"). Moves nothing measurable: the corrections are at rounding.
fn square_frames(r: &mut Record) {
    use solidcore_record::brep::{Curve, Frame, Surface};
    fn v(a: [f64; 3]) -> DVec3 {
        DVec3::from_array(a)
    }
    /// `b` made square to `a`, its length kept.
    fn square_to(a: DVec3, b: DVec3) -> DVec3 {
        let n = a.normalize_or_zero();
        let s = b - n * n.dot(b);
        if s == DVec3::ZERO { b } else { s.normalize() * b.length() }
    }
    fn frame(f: &mut Frame) {
        let x = v(f.x_axis).normalize_or_zero();
        if x == DVec3::ZERO {
            return;
        }
        f.x_axis = x.to_array();
        f.y_axis = square_to(x, v(f.y_axis)).normalize_or_zero().to_array();
    }
    fn curve(c: &mut Curve) {
        if let Curve::Circle { frame: f, .. } | Curve::Ellipse { frame: f, .. } = c {
            frame(f);
        }
    }
    for face in &mut r.faces {
        match &mut face.surface {
            Surface::Plane { u_axis, v_axis, .. } => *v_axis = square_to(v(*u_axis), v(*v_axis)).to_array(),
            Surface::Cylinder { frame: f, .. }
            | Surface::Cone { frame: f, .. }
            | Surface::Sphere { frame: f, .. }
            | Surface::Torus { frame: f, .. } => frame(f),
            Surface::PlacedRevolution { profile, axis, .. } => {
                *axis = v(*axis).normalize_or_zero().to_array();
                curve(profile);
            }
            _ => {}
        }
    }
    for edge in &mut r.edges {
        curve(&mut edge.curve);
    }
}

fn create(call: &'static str, f: impl FnOnce(&mut Kernel) -> solidcore::Result<solidcore::Brep>) -> Result<Solid> {
    guarded(call, || {
        let mut k = kernel();
        let mut body = f(&mut k).map_err(|e| kernel_err(call, e))?;
        to_record(&mut k, &mut body, call)
    })
}

/// Runs `f`, turning a kernel panic into an error where unwinding is available.
pub(crate) fn guarded<T>(call: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let message = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_owned());
            Err(Error::Panic { call: call.into(), message })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{SnapCurve, decode, encode};

    #[test]
    fn box_meshes() {
        let b = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0)).unwrap();
        assert_eq!(b.topology_counts(), (6, 12, 8));
        let m = b.mesh().unwrap();
        // The mesher refines planar faces too: count nothing but what the model relies on.
        assert_eq!(m.indices.len(), m.tri_face.len() * 3);
        let mut faces: Vec<u32> = m.tri_face.clone();
        faces.sort_unstable();
        faces.dedup();
        assert_eq!(faces, (0..6).collect::<Vec<_>>());
        assert_eq!(m.edges.len(), 12);
        assert_eq!(m.normals.len(), m.positions.len());
        for n in &m.normals {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-4);
        }
        assert!((m.origin - DVec3::new(5.0, 10.0, 15.0)).length() < 1e-9);
    }

    #[test]
    fn far_mesh_keeps_precision() {
        // 100 km from the origin, a 0.01 mm step must survive: positions are relative.
        let at = DVec3::new(1e8, 0.0, 0.0);
        let m = Solid::new_box(at, DVec3::new(0.01, 1.0, 1.0)).unwrap().mesh().unwrap();
        let xs: Vec<f32> = m.positions.iter().map(|p| p[0]).collect();
        let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        assert!(((hi - lo) - 0.01).abs() < 1e-6, "{lo} {hi}");
    }

    #[test]
    fn primitives_mesh() {
        for s in [
            Solid::new_cylinder(DVec3::ZERO, 5.0, 10.0),
            Solid::new_sphere(DVec3::ZERO, 5.0),
            Solid::new_cone(DVec3::ZERO, 5.0, 2.0, 10.0),
            Solid::new_torus(DVec3::ZERO, 10.0, 3.0),
        ] {
            let m = s.unwrap().mesh().unwrap();
            assert!(m.indices.len() > 30);
        }
    }

    #[test]
    fn boolean_difference_and_transform() {
        let a = Solid::new_box(DVec3::ZERO, DVec3::splat(20.0)).unwrap();
        let c = Solid::new_cylinder(DVec3::new(10.0, 10.0, -5.0), 4.0, 30.0).unwrap();
        let d = a.boolean(&c, BooleanOp::Difference).unwrap();
        assert!(d.topology_counts().0 >= 7, "{:?}", d.topology_counts());
        let u = a.boolean(&Solid::new_box(DVec3::splat(10.0), DVec3::splat(20.0)).unwrap(), BooleanOp::Union);
        assert!(u.unwrap().topology_counts().0 >= 9);
        let moved = a.transformed(&DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0))).unwrap();
        let (lo, _) = moved.vertex_bounds().unwrap();
        assert!((lo.x - 100.0).abs() < 1e-9);
        d.mesh().unwrap();
    }

    #[test]
    fn box_snaps() {
        let b = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0)).unwrap();
        let Display { mesh: m, snaps: s, .. } = b.display().unwrap();
        assert_eq!(m.edge_ids.len(), m.edges.len());
        assert_eq!(s.vertices.len(), 8);
        assert!(s.vertices.contains(&DVec3::new(10.0, 20.0, 30.0)));
        assert_eq!(s.edges.len(), 12);
        assert!(s.edges.iter().all(|e| matches!(e.curve, SnapCurve::Segment { .. })));
        let mids: Vec<DVec3> = s.edges.iter().filter_map(|e| e.curve.midpoint()).collect();
        assert!(mids.contains(&DVec3::new(5.0, 0.0, 0.0)), "{mids:?}");
        assert_eq!(s.planes.len(), 6);
        assert!(s.planes.iter().all(Option::is_some));
    }

    #[test]
    fn cylinder_snaps() {
        let c = Solid::new_cylinder(DVec3::new(1.0, 2.0, 3.0), 5.0, 10.0).unwrap();
        let s = c.display().unwrap().snaps;
        // Two circles; the side's seam is not drawn, and a circle's vertex is no endpoint.
        assert!(s.vertices.is_empty(), "{:?} {:?}", s.vertices, s.edges);
        assert_eq!(c.mesh().unwrap().edges.len(), 2, "the side's seam is not drawn");
        assert_eq!(s.edges.len(), 2, "{:?}", s.edges);
        let mut centers: Vec<DVec3> = s.edges.iter().filter_map(|e| e.curve.center()).collect();
        centers.sort_by(|a, b| a.z.total_cmp(&b.z));
        assert!((centers[0] - DVec3::new(1.0, 2.0, 3.0)).length() < 1e-9, "{centers:?}");
        assert!((centers[1] - DVec3::new(1.0, 2.0, 13.0)).length() < 1e-9, "{centers:?}");
        assert!(s.edges.iter().all(|e| e.curve.is_closed_arc()));
        assert_eq!(s.planes.iter().filter(|p| p.is_some()).count(), 2);
    }

    #[test]
    fn half_disc_arc_snaps() {
        // A box cut by a cylinder through its side: the cut leaves open arcs.
        let a = Solid::new_box(DVec3::ZERO, DVec3::splat(20.0)).unwrap();
        let c = Solid::new_cylinder(DVec3::new(10.0, 0.0, -5.0), 4.0, 30.0).unwrap();
        let d = a.boolean(&c, BooleanOp::Difference).unwrap();
        let s = d.display().unwrap().snaps;
        let arcs: Vec<&SnapCurve> = s.edges.iter().map(|e| &e.curve).filter(|c| matches!(c, SnapCurve::Arc { .. })).collect();
        assert_eq!(arcs.len(), 2, "{:?}", s.edges);
        for arc in arcs {
            let SnapCurve::Arc { center, sweep, .. } = arc else { unreachable!() };
            assert!((sweep - std::f64::consts::PI).abs() < 1e-6, "{sweep}");
            let m = arc.midpoint().unwrap();
            // The middle of the half circle is the deepest point of the notch.
            assert!((m - (*center + DVec3::new(0.0, 4.0, 0.0))).length() < 1e-9, "{m} {center}");
        }
    }

    #[test]
    fn serde_round_trip() {
        let a = Solid::new_cylinder(DVec3::ZERO, 3.0, 7.0).unwrap();
        let json = serde_json::to_string(&a).unwrap();
        let b: Solid = serde_json::from_str(&json).unwrap();
        assert_eq!(a, b);
        b.mesh().unwrap();
    }

    /// A job and its result cross the worker boundary as MessagePack, exactly.
    #[test]
    fn job_round_trip_messagepack() {
        let a = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::splat(20.0)).unwrap());
        let c = Arc::new(Solid::new_cylinder(DVec3::new(10.0, 10.0, -5.0), 4.0, 30.0).unwrap());
        let job = Job {
            inputs: vec![a.clone(), c],
            tasks: vec![
                Task::Boolean { target: 0, tool: 1, tool_transform: Some(DAffine3::from_translation(DVec3::X)), op: BooleanOp::Difference },
                Task::Primitive { primitive: Primitive::Sphere { center: DVec3::ZERO, radius: 2.0 }, placement: DAffine3::IDENTITY },
                Task::Display { solid: 0, quality: Quality::Final },
            ],
        };
        let back: Job = decode(&encode(&job).unwrap()).unwrap();
        assert_eq!(back.tasks, job.tasks);
        assert_eq!(*back.inputs[0], *a);
        let result = run(&back);
        let bytes = encode(&result).unwrap();
        let outputs = decode::<JobResult>(&bytes).unwrap().unwrap();
        let expected = result.unwrap();
        assert_eq!(outputs.len(), 3);
        for (o, e) in outputs.iter().zip(&expected) {
            match (o, e) {
                (Output::Solid { solid: s, display: Ok(d) }, Output::Solid { solid: t, display: Ok(f) }) => {
                    assert_eq!(s, t);
                    assert_eq!(d.mesh.positions, f.mesh.positions);
                    assert_eq!(d.snaps, f.snaps);
                }
                (Output::Display(Ok(d)), Output::Display(Ok(f))) => assert_eq!(d.mesh.indices, f.mesh.indices),
                _ => panic!("unexpected outputs"),
            }
        }
    }

    #[test]
    fn exact_bounds_of_curved_solids() {
        let close = |(lo, hi): (DVec3, DVec3), (a, b): (DVec3, DVec3)| (lo - a).length() < 1e-6 && (hi - b).length() < 1e-6;
        let cyl = Solid::new_cylinder(DVec3::ZERO, 4.0, 30.0).unwrap();
        assert!(close(cyl.bounds().unwrap(), (DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, 30.0))), "{:?}", cyl.bounds());
        let sphere = Solid::new_sphere(DVec3::splat(1.0), 5.0).unwrap();
        assert!(close(sphere.bounds().unwrap(), (DVec3::splat(-4.0), DVec3::splat(6.0))), "{:?}", sphere.bounds());
        let torus = Solid::new_torus(DVec3::ZERO, 10.0, 2.0).unwrap();
        assert!(close(torus.bounds().unwrap(), (DVec3::new(-12.0, -12.0, -2.0), DVec3::new(12.0, 12.0, 2.0))), "{:?}", torus.bounds());
        let cone = Solid::new_cone(DVec3::ZERO, 6.0, 2.0, 10.0).unwrap();
        assert!(close(cone.bounds().unwrap(), (DVec3::new(-6.0, -6.0, 0.0), DVec3::new(6.0, 6.0, 10.0))), "{:?}", cone.bounds());
        // Mirrored (SolidCore's tight box of a mirrored cylinder was wrong along its axis until
        // `7c4c2f4`) and turned (its loose box grows).
        let mirrored = cyl.transformed(&DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0))).unwrap();
        assert!(close(mirrored.bounds().unwrap(), (DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, 30.0))), "{:?}", mirrored.bounds());
        let turned = cyl.transformed(&DAffine3::from_rotation_z(0.5)).unwrap();
        assert!(close(turned.bounds().unwrap(), (DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, 30.0))), "{:?}", turned.bounds());
        // Cut: a box with a half cylinder off one side keeps the box's extents.
        let b = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 10.0, 5.0)).unwrap();
        let cut = b.boolean(&Solid::new_cylinder(DVec3::new(10.0, 0.0, -1.0), 3.0, 7.0).unwrap(), BooleanOp::Difference).unwrap();
        assert!(close(cut.bounds().unwrap(), (DVec3::ZERO, DVec3::new(20.0, 10.0, 5.0))), "{:?}", cut.bounds());
    }

    #[test]
    fn a_recipe_skips_the_tools_that_fail() {
        let board = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(100.0, 50.0, 18.0)).unwrap());
        let drill = Arc::new(Solid::new_cylinder(DVec3::ZERO, 4.0, 30.0).unwrap());
        let huge = Arc::new(Solid::new_box(DVec3::splat(-10.0), DVec3::splat(200.0)).unwrap());
        let at = |x: f64| DAffine3::from_translation(DVec3::new(x, 25.0, -5.0));
        // Two holes, and a tool swallowing the whole board between them.
        let job = Job {
            inputs: vec![board.clone(), drill, huge],
            tasks: vec![Task::Recipe { blank: 0, tools: vec![(1, at(20.0)), (2, DAffine3::IDENTITY), (1, at(80.0))] }],
        };
        let out = run(&job).unwrap();
        let [Output::Recipe { solid, display, failed }] = out.as_slice() else { panic!("{out:?}") };
        assert_eq!(failed, &[1]);
        assert!(display.is_ok());
        // A box with two through holes: 6 + 2 faces at least.
        assert!(solid.topology_counts().0 >= board.topology_counts().0 + 2, "{:?}", solid.topology_counts());

        // Every tool failing leaves the blank.
        let job = Job {
            inputs: vec![board.clone(), Arc::new(Solid::new_box(DVec3::splat(-10.0), DVec3::splat(200.0)).unwrap())],
            tasks: vec![Task::Recipe { blank: 0, tools: vec![(1, DAffine3::IDENTITY)] }],
        };
        let out = run(&job).unwrap();
        let [Output::Recipe { solid, failed, .. }] = out.as_slice() else { panic!("{out:?}") };
        assert_eq!(failed, &[0]);
        assert_eq!(solid.topology_counts(), board.topology_counts());
    }

    #[test]
    fn failing_task_fails_the_job() {
        let a = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::splat(1.0)).unwrap());
        let far = Arc::new(Solid::new_box(DVec3::splat(10.0), DVec3::splat(1.0)).unwrap());
        let job = Job {
            inputs: vec![a, far],
            tasks: vec![Task::Boolean { target: 0, tool: 1, tool_transform: None, op: BooleanOp::Intersection }],
        };
        assert!(run(&job).is_err());
        let mut inline = Inline::default();
        inline.submit(7, job);
        let r = inline.poll();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, 7);
        assert!(r[0].1.is_err());
    }

    #[test]
    fn panic_task_is_caught_natively() {
        let job = Job { inputs: vec![], tasks: vec![Task::Panic { message: "exprès".into() }] };
        match run(&job) {
            Err(Error::Panic { message, .. }) => assert_eq!(message, "exprès"),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn box_job() -> Job {
        Job {
            inputs: vec![],
            tasks: vec![Task::Primitive { primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::ONE }, placement: DAffine3::IDENTITY }],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wait(p: &mut Pool, n: usize) -> Vec<(JobId, JobResult)> {
        let mut got = Vec::new();
        let start = std::time::Instant::now();
        while got.len() < n && start.elapsed().as_secs() < 30 {
            got.extend(p.poll());
            std::thread::yield_now();
        }
        got.sort_by_key(|(id, _)| *id);
        got
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pool_runs_and_wakes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        let mut p = Pool::with_threads(2, Some(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        p.submit(1, box_job());
        p.submit(2, box_job());
        let got = wait(&mut p, 2);
        assert_eq!(got.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![1, 2]);
        assert!(got.iter().all(|(_, r)| r.is_ok()));
        let start = std::time::Instant::now();
        while woken.load(Ordering::SeqCst) < 2 && start.elapsed().as_secs() < 30 {
            std::thread::yield_now();
        }
        assert!(woken.load(Ordering::SeqCst) >= 2);
    }

    /// An operation goes before the meshes waiting; a job cancelled before it starts comes back
    /// cancelled at once; one running is stopped through its token.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pool_puts_operations_first_and_cancels() {
        let solid = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::ONE).unwrap());
        let mesh = Job { inputs: vec![solid], tasks: vec![Task::Display { solid: 0, quality: Quality::Final }] };
        // No thread: the queues as they stand.
        let mut p = Pool::with_threads(0, None);
        p.submit(1, mesh.clone());
        p.submit(2, mesh.clone());
        p.submit(3, mesh.clone());
        p.submit(9, box_job());
        p.cancel(2);
        let got = p.poll();
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], (2, Err(Error::Cancelled))));
        let (first, _, _) = p.shared.lock().next().unwrap();
        let (second, _, token) = p.shared.lock().next().unwrap();
        assert_eq!((first, second), (9, 1), "the operation first, then the meshes in order");
        p.cancel(1);
        assert!(token.is_cancelled(), "a running job's token is cancelled");
    }

    /// A job cancelled while it runs stops: a STEP read between its bodies.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_running_job_is_cancelled() {
        let token = CancelToken::new();
        token.cancel();
        let job = box_job();
        assert!(matches!(run_with(&job, &Context { report: &|_| {}, cancel: Some(token) }), Err(Error::Cancelled)));
    }

}
