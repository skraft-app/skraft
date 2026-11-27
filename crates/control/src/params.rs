//! The methods' parameters (docs/control-protocol.md): strict (an unknown field is an
//! error), every field optional with a default.

use schemars::JsonSchema;
use serde::Deserialize;

/// No parameters.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// `components.*`: a part or an assembly of the document (docs/ui/components-library.md).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentParams {
    /// The component's id (`model.components` lists them).
    pub component: u64,
}

/// `app.run`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// The command's id (`app.commands` lists them): `tool.move`, `edit.undo`, `view.top`…
    pub id: String,
}

/// `app.save`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    /// Where to save it, an absolute path (`.skraft` added if missing); it becomes the
    /// document's file. None: the document's own file.
    #[serde(default)]
    pub path: Option<String>,
    /// Overwrites another file already there (the document's own is always rewritten).
    #[serde(default)]
    pub force: bool,
}

/// `app.workspace`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceParams {
    /// The workspace to show the active document in (`modelling`…); none: only list them.
    #[serde(default)]
    pub id: Option<String>,
}

/// What `app.docs` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocsAction {
    /// The open documents (name, path, modified), the one shown, the question asked.
    #[default]
    List,
    /// A new document, shown.
    New,
    /// Shows the tab `index`.
    Switch,
    /// Closes the tab `index`: asks to save it first, as its ×, unless `force`.
    Close,
}

/// `app.docs`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocsParams {
    #[serde(default)]
    pub action: DocsAction,
    /// The tab (`switch`, `close`), from 0.
    pub index: Option<usize>,
    /// `close` without asking to save.
    #[serde(default)]
    pub force: bool,
}

/// How long `app.wait` waits at most when not told.
pub const WAIT_DEFAULT_MS: u64 = 10_000;
/// How long it may be told to wait at most.
pub const WAIT_MAX_MS: u64 = 120_000;

/// `app.wait`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitParams {
    /// Waits until the active document's revision is another (as `app.state` or a reply gave it).
    pub revision: Option<u64>,
    /// Waits until the editor is idle: no kernel job, no load. The default when `revision` is
    /// not given; with it, both are waited for.
    pub idle: Option<bool>,
    /// At most this long (ms; default 10 000, at most 120 000): then answers `"timed_out": true`.
    pub timeout_ms: Option<u64>,
}

impl WaitParams {
    /// Is what it waits for there, the active document at `revision`, `idle` or not?
    pub fn met(&self, revision: u64, idle: bool) -> bool {
        let wants_idle = self.idle.unwrap_or(self.revision.is_none());
        self.revision.is_none_or(|r| r != revision) && (!wants_idle || idle)
    }

    /// How long it waits at most.
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms.unwrap_or(WAIT_DEFAULT_MS).min(WAIT_MAX_MS))
    }
}

/// `model.document`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentParams {
    /// The solids' B-rep records in full; without it each is summed up as its counts of faces,
    /// edges and vertices.
    #[serde(default)]
    pub brep: bool,
}

/// How many parts `model.geometry` gives at most when not told.
pub const GEOMETRY_DEFAULT_LIMIT: usize = 100;
/// How many it may be told to give at most.
pub const GEOMETRY_MAX_LIMIT: usize = 2_000;

/// `model.geometry`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeometryParams {
    /// Only the parts placed under one of these occurrences (any level of their path).
    pub ids: Option<Vec<u64>>,
    /// Only the parts placed under the selected occurrences.
    #[serde(default)]
    pub selection: bool,
    /// Each part's volume, measured by the kernel (default true; mm³).
    pub volume: Option<bool>,
    /// Each part's B-rep record (SolidCore's: faces with their surfaces, edges with their
    /// curves, vertices), in its own axes: the exact geometry.
    #[serde(default)]
    pub brep: bool,
    /// Each part's display mesh, in its own axes (positions relative to `origin`): what the view
    /// draws, an approximation. `null` until meshed.
    #[serde(default)]
    pub mesh: bool,
    /// Skips this many parts (paging).
    #[serde(default)]
    pub offset: usize,
    /// At most this many parts (default 100, at most 2 000).
    pub limit: Option<usize>,
}

impl GeometryParams {
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(GEOMETRY_DEFAULT_LIMIT).min(GEOMETRY_MAX_LIMIT)
    }
}

/// Where a pointer input lands: a world point, projected by the camera, or a pixel of the
/// viewport (points, origin top left, as `view.project` gives them). One of the two.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pointer {
    /// A world point (mm).
    pub at: Option<[f64; 3]>,
    /// A pixel of the viewport.
    pub px: Option<[f64; 2]>,
    /// The toggle modifier held (Shift).
    #[serde(default)]
    pub toggle: bool,
    /// The copy modifier held (Option on macOS, Ctrl elsewhere).
    #[serde(default)]
    pub copy: bool,
}

/// A drag of the primary button.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragInput {
    pub from: Pointer,
    /// Where it is let go (its modifiers are the release's).
    pub to: Pointer,
    /// The moves in between (default 8).
    pub steps: Option<usize>,
}

/// A key for the active tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyName {
    Escape,
    Delete,
    Enter,
    Left,
    Right,
    Up,
    Down,
    /// The copy modifier tapped alone.
    Copy,
    /// Shift tapped alone.
    Shift,
    /// Command (macOS) or Alt tapped alone.
    Alt,
    Tab,
    /// Shift with `+`: one copy more (the copy tools).
    More,
    /// Shift with `−`: one copy fewer.
    Fewer,
}

/// A key pressed.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyInput {
    pub name: KeyName,
    #[serde(default)]
    pub toggle: bool,
    #[serde(default)]
    pub copy: bool,
}

/// One input for the active tool, as the viewport gives them.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    /// The cursor moved there.
    Hover(Pointer),
    /// A click there (the cursor moved there first).
    Click(Pointer),
    DoubleClick(Pointer),
    /// Pressed at `from`, dragged to `to`, let go.
    Drag(DragInput),
    Key(KeyInput),
    /// Text typed in the measurements box, then Enter.
    Vcb(String),
    /// Makes this tool the active one, by its id (`box`, `move`, `stretch`… as `tool.state`
    /// names them).
    Tool(String),
    /// Sets an option of the active tool, as its bar does (`tool.state` lists them, with their
    /// choices).
    Option(OptionInput),
}

/// An option of the active tool set: its key, its value.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionInput {
    pub key: String,
    pub value: OptionSetting,
}

/// A tool option's value, as `tool.state` gives it.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionSetting {
    /// One of a choice's values (its `choices`).
    Choice(String),
    Toggle(bool),
    /// A length, mm.
    Length(f64),
}

/// `context.open`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    /// Opened in turn, each from the level the one before opened: an occurrence of an
    /// assembly (edited), of a part (opened), a group or a body node of the part or group open
    /// (opened). The first that cannot be stops the rest.
    pub ids: Vec<u64>,
}

/// `context.close`: one level, `levels` of them, or `all`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseParams {
    /// How many levels to leave (default 1).
    pub levels: Option<u32>,
    /// Every level: back to the model.
    #[serde(default)]
    pub all: bool,
}

/// `context.select`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectParams {
    /// The entities of the level open: occurrences of its assembly, bodies of the part or
    /// group open, its guides. Empty: nothing selected.
    pub ids: Vec<u64>,
    /// Added to the selection rather than replacing it.
    #[serde(default)]
    pub add: bool,
}

/// `context.sign`: the sign of the bodies the drawing tools add to an open part (its
/// « Solide »: positive adds material, negative is a machining).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignParams {
    pub sign: SignName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignName {
    Positive,
    Negative,
    /// An intersection (∩, docs/decisions/0045-intersection-a-third-sign.md).
    Intersect,
}

/// `tool.input`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolInputParams {
    /// Given to the active tool in order; the first one refused stops them.
    pub inputs: Vec<Input>,
}

/// A standard view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewName {
    Top,
    Front,
    Right,
    Back,
    Left,
    Iso,
}

/// `view.camera`: without parameters, reads it; else sets what is given, in this order.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraParams {
    /// Perspective (true) or parallel projection.
    pub perspective: Option<bool>,
    /// A standard view, in the active frame, framing the model.
    pub view: Option<ViewName>,
    /// The eye (mm, world); with `target`, what it looks at.
    pub eye: Option<[f64; 3]>,
    pub target: Option<[f64; 3]>,
    /// The vertical field of view in perspective (degrees).
    pub fov_deg: Option<f64>,
    /// The visible height of a parallel view (mm).
    pub height: Option<f64>,
    /// Frames the whole model (zoom extents), last.
    #[serde(default)]
    pub fit: bool,
}

/// `view.project`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectParams {
    /// World points (mm).
    pub points: Vec<[f64; 3]>,
}

/// `view.pick`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PickParams {
    /// Pixels of the viewport.
    #[serde(default)]
    pub px: Vec<[f64; 2]>,
    /// World points, picked at the pixel they project to (what the eye sees there, maybe in
    /// front of them).
    #[serde(default)]
    pub at: Vec<[f64; 3]>,
}

/// How many entries `journal.log` gives at most when not told.
pub const LOG_DEFAULT_LIMIT: usize = 200;

/// `journal.log`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogParams {
    /// Only the entries numbered after this one (the `next` of a previous answer, less one).
    pub since: Option<u64>,
    /// At most this many, the last ones (default 200).
    pub limit: Option<usize>,
}

/// `journal.failures`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FailuresParams {
    /// One failure in full, by its `seq`: its job with its input solids (B-rep records) and
    /// tasks, which `skraft_kernel::run` runs again. Without it, every failure summed up.
    pub seq: Option<u64>,
}

/// What `journal.record` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordAction {
    /// Whether a recording is under way, and its steps so far.
    #[default]
    Status,
    /// Records from the document and the view as they are now (a recording under way starts
    /// again).
    Start,
    /// Stops, and writes the recording as a test of `crates/tools/tests`.
    Stop,
}

/// `journal.record`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    #[serde(default)]
    pub action: RecordAction,
    /// `stop`: the test's name, a Rust identifier (default `recorded`); its file is
    /// `crates/tools/tests/<name>.rs`, the document beside it as `<name>.skraft`.
    pub name: Option<String>,
}

/// Where a real pointer event lands, in the window (points, origin top left): a point, a
/// named handle's centre (`ui.handles` lists them), or a world point (its pixel in the viewport).
/// One of the three.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWhere {
    pub pos: Option<[f32; 2]>,
    pub handle: Option<String>,
    pub at: Option<[f64; 3]>,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiButton {
    #[default]
    Left,
    Right,
    Middle,
    /// Button 4.
    Back,
    /// Button 5.
    Forward,
}

/// Modifier keys held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiMods {
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    /// Option on macOS.
    #[serde(default)]
    pub alt: bool,
    /// Command on macOS, Ctrl elsewhere.
    #[serde(default)]
    pub command: bool,
}

/// `ui.click`, `ui.move`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiPointerParams {
    #[serde(rename = "where")]
    pub at: UiWhere,
    #[serde(default)]
    pub button: UiButton,
    /// Two clicks (`ui.click`).
    #[serde(default)]
    pub double: bool,
    #[serde(default)]
    pub mods: UiMods,
}

/// `ui.drag`: pressed at `from`, moved to `to` in `steps` frames, let go.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDragParams {
    pub from: UiWhere,
    pub to: UiWhere,
    /// The moves in between (default 8).
    pub steps: Option<usize>,
    #[serde(default)]
    pub button: UiButton,
    #[serde(default)]
    pub mods: UiMods,
}

/// `ui.touch`: fingers on the screen, as a touch screen gives them (docs/ui/touch.md).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTouchParams {
    /// One path per finger, in the order they come down (a frame apart); the first one also
    /// moves the pointer, as the platforms emulate it.
    pub fingers: Vec<UiFinger>,
    /// The moves in between (default 8; 0: a tap).
    pub steps: Option<usize>,
    /// Put the fingers down first (default); false: they are down already, from a call that did
    /// not lift them, and go on from `from`.
    pub press: Option<bool>,
    /// Lift them at the end (default); false: they stay down, for a screenshot or a long press,
    /// till a call with `press` false.
    pub lift: Option<bool>,
}

/// One finger of `ui.touch`: down at `from`, moved to `to` (default: stays), lifted.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiFinger {
    pub from: UiWhere,
    pub to: Option<UiWhere>,
}

/// `ui.key`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiKeyParams {
    /// egui's name of the key: `Escape`, `Enter`, `Tab`, `ArrowUp`, `A`, `Delete`…
    pub key: String,
    #[serde(default)]
    pub mods: UiMods,
    /// Only the press (true) or the release (false); without it, both.
    pub pressed: Option<bool>,
}

/// `ui.text`: text typed (in the field that has the focus).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTextParams {
    pub text: String,
}

/// `ui.scroll`: the wheel, at a place.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiScrollParams {
    #[serde(rename = "where")]
    pub at: UiWhere,
    /// How far, in points (x, y; positive y moves the content down).
    pub delta: [f32; 2],
    #[serde(default)]
    pub mods: UiMods,
}

/// `ui.handles`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiHandlesParams {
    /// Only those whose name starts with it (`ribbon:`, `option:`, `tab`…).
    #[serde(default)]
    pub prefix: String,
}

/// `ui.messages`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiMessagesParams {
    /// Empties the list after answering.
    #[serde(default)]
    pub clear: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_for_a_revision_an_idle_editor_or_both() {
        // Nothing said: idle.
        let w = WaitParams::default();
        assert!(w.met(3, true));
        assert!(!w.met(3, false));
        // A revision: another one, busy or not.
        let w = WaitParams { revision: Some(3), ..Default::default() };
        assert!(!w.met(3, true));
        assert!(w.met(4, false));
        // Both.
        let w = WaitParams { revision: Some(3), idle: Some(true), ..Default::default() };
        assert!(!w.met(4, false));
        assert!(w.met(4, true));
        assert_eq!(WaitParams { timeout_ms: Some(1_000_000), ..Default::default() }.timeout().as_millis(), WAIT_MAX_MS as u128);
    }

    #[test]
    fn inputs_read_as_written() {
        let p: ToolInputParams = serde_json::from_value(serde_json::json!({ "inputs": [
            { "tool": "box" },
            { "click": { "at": [0, 0, 0] } },
            { "drag": { "from": { "px": [10, 10] }, "to": { "at": [1, 2, 3], "toggle": true } } },
            { "key": { "name": "escape" } },
            { "vcb": "600;400" },
        ] }))
        .unwrap();
        assert_eq!(p.inputs.len(), 5);
        assert_eq!(p.inputs[1], Input::Click(Pointer { at: Some([0.0; 3]), ..Default::default() }));
        assert!(serde_json::from_value::<ToolInputParams>(serde_json::json!({ "inputs": [{ "clack": {} }] })).is_err());
        assert!(
            serde_json::from_value::<ToolInputParams>(serde_json::json!({ "inputs": [{ "click": { "at": [0, 0, 0], "shift": true } }] }))
                .is_err()
        );
    }
}
