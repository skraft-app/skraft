//! What crosses to the kernel and back (docs/model/kernel.md): a [`Job`] of pure tasks,
//! records in, records and display meshes out, and the [`KernelService`] that runs it.

use std::sync::Arc;

use glam::DAffine3;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{BooleanOp, Error, Mesh, Primitive, Snaps, Solid, Stretch};

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
    /// A part's recipe (docs/model/data-model.md, docs/model/bodies.md): the union of the `positives`, minus
    /// each of the `negatives` — every one `inputs[k]` placed by its transform —, then joined
    /// to `inputs[base]` when given (a positive body added to a part as it is). A boolean that
    /// fails is skipped and reported (by its index in `positives` then `negatives`, one list),
    /// the others still done: the task itself fails only when no positive is left.
    Recipe {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<u32>,
        positives: Vec<(u32, DAffine3)>,
        negatives: Vec<(u32, DAffine3)>,
        /// A group's: its inputs named, for its result's face map
        /// (docs/decisions/0042-body-nodes-operators.md).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        names: Option<crate::names::RecipeNames>,
    },
    /// `inputs[solid]` stretched (docs/tools/stretch.md), read back by the kernel.
    Stretch { solid: u32, stretch: Stretch },
    /// A body node (docs/decisions/0042-body-nodes-operators.md): its `children` computed from
    /// their leaves in one session, then `op` applied to the edges its names match — or, with
    /// `picks` (points in the node's axes, the tool's), to the edges nearest them, named.
    Node {
        children: Vec<crate::names::TreeBody>,
        op: crate::names::NodeOp,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        picks: Vec<[f64; 3]>,
    },
    /// The display mesh and snap features of `inputs[solid]`, at `quality`.
    Display {
        solid: u32,
        #[serde(default)]
        quality: Quality,
    },
    /// Reads a STEP file (its bytes): bodies built, product structure, colours
    /// (docs/io/step-import.md). Produces no solid of the job's: an [`Output::Step`].
    ImportStep {
        #[serde(with = "serde_bytes_compat")]
        bytes: Vec<u8>,
    },
    /// Panics with `message`, on purpose: tests how a kernel crash is recovered from (the
    /// app's Débogage menu). Natively the panic is caught: an error. In the web
    /// worker it aborts the wasm: the worker is replaced.
    Panic { message: String },
}

/// How fine a display mesh is: a coarse one first, to show a model being opened at once (a
/// hundredth of the body, `solidcore_tess::preview`), then the one to work with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    Preview,
    #[default]
    Final,
}

/// How far a long task is (docs/ui/loading.md), reported while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Progress {
    /// A STEP file being parsed and its structure read.
    Reading,
    /// A STEP file's bodies: `done` of `total` read.
    Bodies { done: u32, total: u32 },
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
    Solid {
        solid: Solid,
        display: Result<Display, String>,
    },
    Display(Result<Display, String>),
    /// A recipe's final solid, and the bodies (by index in the task's lists) that failed.
    Recipe {
        solid: Solid,
        display: Result<Display, String>,
        failed: Vec<u32>,
        /// Its faces' names, when the task named its inputs and the session gave them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        faces: Option<crate::names::FaceMap>,
    },
    /// A node's solid, the edges it took (by name), its children whose boolean failed (by
    /// path of body ids), and why it left its children's result as it was.
    Node {
        solid: Solid,
        display: Result<Display, String>,
        /// Its result's face map (relative to it).
        #[serde(default)]
        faces: crate::names::FaceMap,
        edges: Vec<crate::names::NodeEdges>,
        failed: Vec<Vec<u64>>,
        refusal: Option<crate::names::NodeRefusal>,
        /// The nodes inside it computed with it that left their children's result as it was
        /// (by path of body ids), and why: theirs to show.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        inner: Vec<(Vec<u64>, crate::names::NodeRefusal)>,
        /// Its children's result before its operation, and its face map (for the node to start
        /// from next time: [`crate::names::TreeNode::Base`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<(Solid, crate::names::FaceMap)>,
    },
    /// A STEP file read (boxed: far larger than the others).
    Step(Box<crate::StepImport>),
}

/// A file's bytes crossing to the web worker as one MessagePack binary, not one number per
/// byte.
mod serde_bytes_compat {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element()? {
                    out.push(b);
                }
                Ok(out)
            }
        }
        d.deserialize_byte_buf(V)
    }
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
    /// The progress jobs reported since the last call, the latest per job.
    fn progress(&mut self) -> Vec<(JobId, Progress)> {
        Vec::new()
    }
    /// Stops a job: it is dropped if not started, else stopped at the kernel's next check; its
    /// result comes back as [`Error::Cancelled`] (or as it ended, when it had already). A
    /// service that runs jobs as they are submitted has nothing to stop.
    fn cancel(&mut self, _id: JobId) {}
}

/// The bytes sent between the UI and the web worker: MessagePack with named fields (the
/// record has internally tagged enums, which need a self-describing format).
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    rmp_serde::to_vec_named(value).map_err(|e| e.to_string())
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    rmp_serde::from_slice(bytes).map_err(|e| e.to_string())
}
