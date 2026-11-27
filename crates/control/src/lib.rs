//! The control protocol (docs/control-protocol.md): what an agent, a script or a test
//! asks the app — a method and its JSON parameters, [`Call`] — and what it answers. One method
//! set for every transport: the loopback HTTP server of the desktop ([`server`]), the page's
//! `window.skraft` (web) and the MCP server (`apps/skraft-mcp`).
//!
//! The methods' parameters are types here ([`params`]), read strictly (an unknown field or a
//! wrong type is refused, naming it, and nothing runs); their JSON Schemas come from them
//! ([`methods`], the method `app.methods`). The methods that need only an [`Editor`] — the
//! model, the active tool — are answered here ([`editor::call`]), so that the window and a
//! headless session answer them alike; the window answers the others (documents, commands,
//! screenshots).
//!
//! [`Editor`]: skraft_tools::Editor

pub mod editor;
pub mod instances;
pub mod json;
pub mod params;
pub mod record;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

use serde::de::DeserializeOwned;
use serde_json::Value;

/// A request: a method and its parameters (`null` or absent: none, as `{}`), and the client
/// that sent it if it said (the `X-Skraft-Client` header: an agent, a script — the
/// journal notes what it caused as its).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub method: String,
    pub params: Value,
    pub client: Option<String>,
}

impl Call {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Call { method: method.into(), params, client: None }
    }

    pub fn from_client(mut self, client: Option<String>) -> Self {
        self.client = client;
        self
    }

    /// The parameters, read as `T` (`null` as `{}`): a field unknown or of the wrong type is an
    /// error naming it.
    pub fn params<T: DeserializeOwned>(&self) -> Result<T, Error> {
        let params = match &self.params {
            Value::Null => Value::Object(Default::default()),
            p => p.clone(),
        };
        serde_json::from_value(params).map_err(|e| Error::BadParams(format!("{}: {e}", self.method)))
    }
}

/// A successful answer: JSON, or an image (a screenshot).
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Json(Value),
    Png(Vec<u8>),
}

/// Why a call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// No such method.
    UnknownMethod(String),
    /// Its parameters do not fit the method's.
    BadParams(String),
    /// The method could not do what was asked (no document, no such tab, a command refused).
    Failed(String),
    /// Not available here (a window's method asked of a headless session).
    Unavailable(String),
}

impl Error {
    /// The HTTP status that carries it.
    pub fn status(&self) -> u16 {
        match self {
            Error::UnknownMethod(_) => 404,
            Error::BadParams(_) => 400,
            Error::Failed(_) => 422,
            Error::Unavailable(_) => 503,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Error::UnknownMethod(m) => format!("no method {m:?} (app.methods lists them)"),
            Error::BadParams(e) | Error::Failed(e) | Error::Unavailable(e) => e.clone(),
        }
    }
}

pub type Outcome = Result<Answer, Error>;

/// A method of the protocol, as `app.methods` describes it.
pub struct Method {
    pub name: &'static str,
    pub summary: &'static str,
    /// Answered without a window (a headless session).
    pub headless: bool,
    /// Its parameters' JSON Schema.
    pub params: fn() -> schemars::Schema,
}

fn schema<T: schemars::JsonSchema>() -> schemars::Schema {
    schemars::schema_for!(T)
}

/// Every method, in the order `app.methods` lists them.
pub fn methods() -> Vec<Method> {
    use params::*;
    let m = |name, summary, headless, params| Method { name, summary, headless, params };
    vec![
        m("app.ping", "Is the app there?", true, schema::<NoParams>),
        m("app.methods", "The methods, with their parameters' JSON Schemas.", true, schema::<NoParams>),
        m(
            "app.state",
            "The active document: tool, the level open (`context`, `open_part`, `open_group`), selection, the sign of the bodies drawn (`body_sign`), status, revision, busy, modified, path; and which app answers (`instance`: its build, worktree, pid, who launched it).",
            true,
            schema::<NoParams>,
        ),
        m(
            "app.instances",
            "The Skraft apps running on this computer: pid, port of their control channel (if on), build (worktree, branch, commit), who launched them (a Claude Code session).",
            true,
            schema::<NoParams>,
        ),
        m(
            "app.wait",
            "Returns once the active document's revision differs from `revision` and/or the editor is idle (no kernel job, no load): never guess a delay.",
            true,
            schema::<WaitParams>,
        ),
        m("app.commands", "The command registry: id, label, enabled, checked, shortcut.", false, schema::<NoParams>),
        m(
            "app.save",
            "Saves the active document: to its own file, or to `path` (absolute, `.skraft` added if missing), which becomes its file, as « Save as »; another file already there is refused unless `force`.",
            true,
            schema::<SaveParams>,
        ),
        m("app.run", "Runs a command of the registry by its id.", false, schema::<RunParams>),
        m("app.docs", "The open documents (list), or a new one, another shown, one closed.", false, schema::<DocsParams>),
        m(
            "app.workspace",
            "The workspaces this build lists (id, label, under way, stage) and the active document's; with `id`, shows the document in that one (the running tool cancelled, the selection kept; not a change of the document).",
            false,
            schema::<WorkspaceParams>,
        ),
        m("app.screenshot", "The viewport, as a PNG.", false, schema::<NoParams>),
        m(
            "context.open",
            "Opens, in turn, each of `ids` from the level open, as a double-click: an occurrence of the assembly edited (edited), of a part (opened), a group or a body node of the part or group open (opened). Refused at the first that cannot be.",
            true,
            schema::<OpenParams>,
        ),
        m(
            "context.close",
            "Leaves one level open (or `levels`, or `all`), as Esc: a body open, a group or node, the part, an assembly; what was left is selected.",
            true,
            schema::<CloseParams>,
        ),
        m(
            "context.select",
            "Selects entities of the level open by id (`add`: added): occurrences of the assembly edited, bodies of the part or group open, guides. Refused, nothing changed, naming one that is not there.",
            true,
            schema::<SelectParams>,
        ),
        m(
            "context.sign",
            "The sign of the bodies the drawing tools add to an open part (its « Solide »): `positive` adds material, `negative` is a machining.",
            true,
            schema::<SignParams>,
        ),
        m(
            "model.document",
            "The document's recipe, as its file holds it (not saved); solids summed up unless `brep`.",
            true,
            schema::<DocumentParams>,
        ),
        m(
            "model.geometry",
            "Every placed part (filtered by `ids` or the selection): world transform, bounds, sizes, volume, pieces, topology, bodies (each its nature, a group's children, a body node's operation and refusal); optionally its B-rep and display mesh.",
            true,
            schema::<GeometryParams>,
        ),
        m(
            "model.components",
            "The document's components library (docs/ui/components-library.md): every part and assembly with its name, kind, whether named, its count in the model (occurrences at any depth) and whether unused (placed nowhere); the one chosen.",
            true,
            schema::<NoParams>,
        ),
        m(
            "components.place",
            "Carries one more occurrence of a component at the cursor, held by the origin of its axes, as a paste: put it down with a tool.input click, Escape gives it up.",
            true,
            schema::<ComponentParams>,
        ),
        m(
            "components.select",
            "Selects the occurrences of a component under the active context (as paths under the « Parts » scope when deeper than its children).",
            true,
            schema::<ComponentParams>,
        ),
        m(
            "components.replace",
            "Replaces the selected occurrences by occurrences of a component, each keeping its placement, name and visibility.",
            true,
            schema::<ComponentParams>,
        ),
        m("components.delete", "Deletes a component and all its occurrences, at any depth.", true, schema::<ComponentParams>),
        m("components.purge", "Deletes every component not in the model (the unused ones).", true, schema::<NoParams>),
        m(
            "tool.state",
            "The active tool: status, measurements box, options, axis lock, its preview as data, the inference under the cursor.",
            true,
            schema::<NoParams>,
        ),
        m(
            "tool.input",
            "Inputs for the active tool as the viewport gives them — hover, click, double click, drag, keys, typed values, another tool, an option of its bar set — at world points (`at`) or pixels (`px`); answers as tool.state, with the pixel of each input.",
            true,
            schema::<ToolInputParams>,
        ),
        m(
            "view.camera",
            "The camera (eye, target, axes, projection, viewport); with parameters, sets it first: projection, standard view, eye and target, field of view or height, fit.",
            true,
            schema::<CameraParams>,
        ),
        m(
            "view.project",
            "World points to pixels of the viewport (points, origin top left), and whether each is in view.",
            true,
            schema::<ProjectParams>,
        ),
        m(
            "view.pick",
            "What the eye sees at pixels (or at the pixels world points project to): part, path, B-rep face, point, normal.",
            true,
            schema::<PickParams>,
        ),
        m(
            "journal.log",
            "What the editor went through: inputs (hovers in a row as one), tools, options, operations applied or failed, kernel jobs (time, error), undo, redo — numbered, `since` one.",
            true,
            schema::<LogParams>,
        ),
        m(
            "journal.failures",
            "The kernel's last failures (an operation's job failed or skipped bodies of a recipe, a mesh refused); with `seq`, one in full: its job (input solids, tasks), which skraft_kernel::run runs again.",
            true,
            schema::<FailuresParams>,
        ),
        m(
            "journal.record",
            "Records the inputs from the document and the view as they are (`start`), then writes them as a test of crates/tools/tests (`stop`): the test's file and the document's.",
            true,
            schema::<RecordParams>,
        ),
        m(
            "ui.handles",
            "Where named widgets were drawn last frame (window points): ribbon:<command>, titlebar:<command>, menu:<command>, option:<key>[:<value>], tab:<i>, tab_close:<i>, tab_new, quick:<command>, vcb, viewport.",
            false,
            schema::<UiHandlesParams>,
        ),
        m(
            "ui.click",
            "A real click (egui events, one per frame) at a point, a handle or a world point; answered once the interface has taken it.",
            false,
            schema::<UiPointerParams>,
        ),
        m("ui.move", "The real pointer moved to a point, a handle or a world point.", false, schema::<UiPointerParams>),
        m("ui.drag", "A real drag: pressed, moved in steps, let go.", false, schema::<UiDragParams>),
        m(
            "ui.touch",
            "Real fingers on a touch screen: each down at `from`, moved to `to` in steps, lifted (two: pan and pinch).",
            false,
            schema::<UiTouchParams>,
        ),
        m("ui.key", "A real key pressed and let go (egui's names), with modifiers.", false, schema::<UiKeyParams>),
        m("ui.text", "Real text typed, in the field with the focus.", false, schema::<UiTextParams>),
        m("ui.scroll", "The real wheel at a place (zooms the view).", false, schema::<UiScrollParams>),
        m("ui.screenshot", "The whole window as a PNG (panels and bars included).", false, schema::<NoParams>),
        m("ui.messages", "The status bar's messages of this run, oldest first (the last 200).", false, schema::<UiMessagesParams>),
    ]
}

/// The catalog as `app.methods` answers it.
pub fn methods_json() -> Value {
    Value::Array(
        methods()
            .into_iter()
            .map(|m| serde_json::json!({ "name": m.name, "summary": m.summary, "headless": m.headless, "params": (m.params)() }))
            .collect(),
    )
}

/// Where `app.save` writes (docs/control-protocol.md): `given` (absolute, the extension
/// added if missing), else the document's `own` file; another file already there is refused
/// unless `force`.
pub fn save_path(given: Option<&str>, own: Option<&std::path::Path>, force: bool) -> Result<std::path::PathBuf, Error> {
    use std::path::{Path, PathBuf};
    let Some(given) = given else {
        return own.map(Path::to_path_buf).ok_or_else(|| Error::Failed("the document has no file yet: give a `path`".into()));
    };
    let mut path = PathBuf::from(given);
    if !path.is_absolute() {
        return Err(Error::BadParams(format!("app.save: `path` must be absolute: {given:?}")));
    }
    if path.extension().and_then(|e| e.to_str()) != Some(skraft_model::EXTENSION) {
        path.as_mut_os_string().push(format!(".{}", skraft_model::EXTENSION));
    }
    let same = |a: &Path, b: &Path| a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if !force && path.exists() && !own.is_some_and(|o| same(o, &path)) {
        return Err(Error::Failed(format!("{} exists already: `force` to overwrite it", path.display())));
    }
    Ok(path)
}

/// Whether `name` is a method of the protocol.
pub fn is_method(name: &str) -> bool {
    methods().iter().any(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use params::*;

    #[test]
    fn params_are_strict_and_null_is_empty() {
        let c = Call::new("model.geometry", Value::Null);
        assert_eq!(c.params::<GeometryParams>().unwrap(), GeometryParams::default());
        let c = Call::new("model.geometry", serde_json::json!({ "brep": true, "ids": [3] }));
        let p: GeometryParams = c.params().unwrap();
        assert!(p.brep);
        assert_eq!(p.ids, Some(vec![3]));
        let e = Call::new("model.geometry", serde_json::json!({ "breps": true })).params::<GeometryParams>().unwrap_err();
        assert!(e.message().contains("breps"), "{}", e.message());
        let e = Call::new("model.geometry", serde_json::json!({ "brep": 1 })).params::<GeometryParams>().unwrap_err();
        assert_eq!(e.status(), 400);
    }

    #[test]
    fn every_method_has_a_schema() {
        let all = methods_json();
        let all = all.as_array().unwrap();
        assert!(all.len() >= 10);
        for m in all {
            assert!(m["params"].is_object(), "{m}");
        }
        assert!(is_method("tool.state"));
        assert!(!is_method("tool.nope"));
    }

    #[test]
    fn where_a_save_goes() {
        let dir = std::env::temp_dir().join(format!("skraft-control-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.join("own.skraft");
        std::fs::write(&own, b"x").unwrap();
        assert_eq!(save_path(None, Some(&own), false), Ok(own.clone()));
        assert!(matches!(save_path(None, None, false), Err(Error::Failed(_))));
        assert!(matches!(save_path(Some("rel.skraft"), None, false), Err(Error::BadParams(_))));
        let s = |p: &std::path::Path| p.to_str().unwrap().to_owned();
        assert_eq!(save_path(Some(&s(&dir.join("a"))), None, false), Ok(dir.join("a.skraft")));
        assert_eq!(save_path(Some(&s(&dir.join("a.v2"))), None, false), Ok(dir.join("a.v2.skraft")));
        // Its own file rewritten, another refused unless forced.
        assert_eq!(save_path(Some(&s(&own)), Some(&own), false), Ok(own.clone()));
        assert!(matches!(save_path(Some(&s(&own)), None, false), Err(Error::Failed(m)) if m.contains("force")));
        assert_eq!(save_path(Some(&s(&own)), None, true), Ok(own.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
