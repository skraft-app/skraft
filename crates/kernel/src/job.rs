//! What crosses to the kernel and back (docs/ARCHITECTURE.md §7 ter): a [`Job`] of pure tasks,
//! records in, records and display meshes out, and the [`KernelService`] that runs it.

use std::sync::Arc;

use glam::DAffine3;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{BooleanOp, Error, Mesh, Primitive, Snaps, Solid};

/// Names a job to its service; results come back with it.
pub type JobId = u64;

/// A task input `EARLIER | k` is the solid task `k` of the same job produced.
pub const EARLIER: u32 = 1 << 31;

/// Records in, tasks to run in order. Tasks name their input solids by index in `inputs`, so
/// a record many tasks read (an array of copies) crosses once — or, the index ORed with
/// [`EARLIER`], the solid an earlier task of the job produced (a primitive then cut by a
/// recipe).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Job {
    pub inputs: Vec<Arc<Solid>>,
    pub tasks: Vec<Task>,
}

/// One kernel call. Every task but `Display` produces a solid, which comes back with its
/// display (and, for a recipe, the tools that failed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Task {
    /// A primitive built in its own frame, then placed by the rigid `placement`.
    Primitive { primitive: Primitive, placement: DAffine3 },
    /// `inputs[solid]` moved by the rigid (or uniformly scaled) `transform`.
    Transform { solid: u32, transform: DAffine3 },
    /// `inputs[target] op inputs[tool]`, the tool first moved by `tool_transform` into the
    /// target's coordinates.
    Boolean { target: u32, tool: u32, tool_transform: Option<DAffine3>, op: BooleanOp },
    /// A part's recipe (docs/ARCHITECTURE.md §2.4): `inputs[blank]` minus each tool
    /// `inputs[tool]` placed by its transform, one after the other. A subtraction that fails
    /// is skipped and reported (by its index in `tools`), the others still cut: the task
    /// itself never fails on a tool.
    Recipe { blank: u32, tools: Vec<(u32, DAffine3)> },
    /// The display mesh and snap features of `inputs[solid]`.
    Display { solid: u32 },
    /// Panics with `message`, on purpose: tests how a kernel crash is recovered from (the
    /// app's Débogage menu). Natively the panic is caught: an error. In the web
    /// worker it aborts the wasm: the worker is replaced.
    Panic { message: String },
}

/// What the viewport draws of a solid and what the tools infer on, from one tessellation;
/// with the solid's exact bounding box (what dimensions are read on: the mesh's lies inside
/// curved faces, the vertices' misses them).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Display {
    pub mesh: Mesh,
    pub snaps: Snaps,
    #[serde(default)]
    pub bounds: Option<(glam::DVec3, glam::DVec3)>,
}

/// One task's result. A display the mesher refused does not fail the task: the solid is
/// still the operation's result, it is only not drawn.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Output {
    Solid { solid: Solid, display: Result<Display, String> },
    Display(Result<Display, String>),
    /// A recipe's final solid, and the tools (by index in the task's list) that failed to cut.
    Recipe { solid: Solid, display: Result<Display, String>, failed: Vec<u32> },
}

/// The outputs of a job's tasks, in order; or why a task producing a solid failed (the whole
/// job fails then: an operation is applied entirely or not at all).
pub type JobResult = Result<Vec<Output>, Error>;

/// Wakes the UI when a result is ready (e.g. `egui::Context::request_repaint`).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Runs jobs away from the caller. Results come back through [`KernelService::poll`], in any
/// order, each with the id it was submitted with.
pub trait KernelService {
    fn submit(&mut self, id: JobId, job: Job);
    /// The results ready now.
    fn poll(&mut self) -> Vec<(JobId, JobResult)>;
}

/// The bytes sent between the UI and the web worker: MessagePack with named fields (the
/// record has internally tagged enums, which need a self-describing format).
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    rmp_serde::to_vec_named(value).map_err(|e| e.to_string())
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    rmp_serde::from_slice(bytes).map_err(|e| e.to_string())
}
