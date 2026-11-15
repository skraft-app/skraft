//! Where the MCP server's tool calls go (docs/ARCHITECTURE.md §7 septies): the running app,
//! over its control channel ([`Remote`]), or an editor of the server's own, without a window
//! ([`Headless`]).

use std::path::PathBuf;

use serde_json::{Value, json};
use solidsketch_control::params::NoParams;
use solidsketch_control::{Answer, Call, Error, Method, Outcome};
use solidsketch_model::Document;
use solidsketch_tools::Editor;

pub trait Backend {
    fn call(&mut self, call: &Call) -> Outcome;
    /// Whether it serves `m` (offered as a tool).
    fn serves(&self, m: &Method) -> bool;
    /// What it is, for the server's instructions.
    fn describe(&self) -> String;
}

/// The app running on this computer, its channel found anew at every call (the discovery and
/// token files): an app started, or started again, after the server is found.
pub struct Remote {
    dir: PathBuf,
    port: Option<u16>,
}

impl Remote {
    /// The app the discovery file names, or the one on `port`.
    pub fn new(port: Option<u16>) -> Self {
        Remote { dir: solidsketch_control::server::default_dir().unwrap_or_default(), port }
    }

    /// Does an app answer?
    pub fn reachable(&self) -> bool {
        self.ping().is_ok()
    }

    fn ping(&self) -> Outcome {
        let (port, token) = solidsketch_control::server::discover(&self.dir, self.port).map_err(Error::Unavailable)?;
        solidsketch_control::server::call(port, &token, &Call::new("app.ping", Value::Null))
    }
}

impl Backend for Remote {
    fn call(&mut self, call: &Call) -> Outcome {
        let (port, token) = solidsketch_control::server::discover(&self.dir, self.port).map_err(|e| {
            Error::Unavailable(format!("{e}: start SolidSketch with its control channel on (title bar, or SOLIDSKETCH_CONTROL=1)"))
        })?;
        solidsketch_control::server::call(port, &token, call)
    }

    fn serves(&self, _: &Method) -> bool {
        true
    }

    fn describe(&self) -> String {
        let at = match self.port {
            Some(p) => format!("port {p}"),
            None => "the port its discovery file names".into(),
        };
        format!("connected to the running SolidSketch app ({at}): what it does shows in its window, every tool is there.")
    }
}

/// An editor of the server's own: a document (empty, or read from a file), the kernel run
/// inline, a virtual viewport of 1280 × 800 — every method but the window's (`ui.*`, commands,
/// screenshots).
pub struct Headless {
    editor: Editor,
    path: Option<PathBuf>,
}

impl Headless {
    pub fn new(path: Option<PathBuf>) -> Result<Self, String> {
        let doc = match &path {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                Document::from_json(&text).map_err(|e| format!("{}: {e}", p.display()))?
            }
            None => Document::new(),
        };
        let mut editor = Editor::new(doc, Box::new(solidsketch_kernel::Inline::default()));
        editor.set_viewport_size(glam::DVec2::new(1280.0, 800.0));
        editor.zoom_extents();
        Ok(Headless { editor, path })
    }

    fn stamp(&self) -> Value {
        let e = &self.editor;
        json!({ "document": 1, "revision": e.history().revision(), "busy": e.busy(), "idle": solidsketch_control::editor::idle(e) })
    }
}

impl Backend for Headless {
    fn call(&mut self, call: &Call) -> Outcome {
        if let Some(outcome) = solidsketch_control::editor::call(&mut self.editor, call) {
            return outcome;
        }
        let e = &mut self.editor;
        match call.method.as_str() {
            "app.ping" => Ok(Answer::Json(json!({}))),
            "app.methods" => Ok(Answer::Json(solidsketch_control::methods_json())),
            "app.state" => {
                call.params::<NoParams>()?;
                Ok(Answer::Json(json!({
                    "tool": e.tool_kind().id(),
                    "context": e.context().iter().map(|i| i.0).collect::<Vec<_>>(),
                    "open_part": e.open_part().map(|i| i.0),
                    "selection": e.selection().iter().map(|i| i.0).collect::<Vec<_>>(),
                    "status": e.status(),
                    "busy": e.busy(),
                    "idle": solidsketch_control::editor::idle(e),
                    "revision": e.history().revision(),
                    "modified": e.history().revision() > 0,
                    "path": self.path,
                    "document": 1,
                    "headless": true,
                })))
            }
            "app.wait" => {
                call.params::<solidsketch_control::params::WaitParams>()?;
                // The kernel runs inline: what was asked is done.
                e.poll();
                let mut v = self.stamp();
                v["timed_out"] = json!(false);
                Ok(Answer::Json(v))
            }
            m if solidsketch_control::is_method(m) => Err(Error::Unavailable(format!(
                "{m} needs the app's window: start SolidSketch with its control channel on, then this server without --headless"
            ))),
            m => Err(Error::UnknownMethod(m.into())),
        }
    }

    fn serves(&self, m: &Method) -> bool {
        m.headless
    }

    fn describe(&self) -> String {
        let doc = match &self.path {
            Some(p) => format!("the document {}", p.display()),
            None => "an empty document".into(),
        };
        format!(
            "headless — an editor of its own on {doc}, no window (no SolidSketch app was running, or --headless): the model, the tools and the camera, not the interface (ui_*), commands nor screenshots."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of(b: &mut Headless, method: &str, params: Value) -> Value {
        match b.call(&Call::new(method, params)) {
            Ok(Answer::Json(v)) => v,
            other => panic!("{method}: {other:?}"),
        }
    }

    #[test]
    fn headless_models_with_the_tools() {
        let mut b = Headless::new(None).unwrap();
        assert_eq!(json_of(&mut b, "app.state", Value::Null)["tool"], "select");
        json_of(
            &mut b,
            "tool.input",
            json!({ "inputs": [{ "tool": "box" }, { "click": { "at": [0, 0, 0] } }, { "click": { "at": [600, 400, 0] } }, { "vcb": "18" }, { "tool": "select" }] }),
        );
        assert_eq!(json_of(&mut b, "app.wait", json!({ "idle": true }))["revision"], 1);
        let g = json_of(&mut b, "model.geometry", Value::Null);
        let dims: Vec<f64> = serde_json::from_value(g["parts"][0]["dims"].clone()).unwrap();
        assert!((dims[0] - 600.0).abs() < 1e-9 && (dims[1] - 400.0).abs() < 1e-9 && (dims[2] - 18.0).abs() < 1e-9, "{dims:?}");
        assert!((g["parts"][0]["volume"].as_f64().unwrap() - 600.0 * 400.0 * 18.0).abs() < 1.0);
        // The window's methods: refused, said why.
        assert!(matches!(b.call(&Call::new("ui.click", json!({}))), Err(Error::Unavailable(m)) if m.contains("window")));
        assert!(matches!(b.call(&Call::new("app.screenshot", Value::Null)), Err(Error::Unavailable(_))));
    }

    #[test]
    fn headless_opens_a_file() {
        let mut b = Headless::new(None).unwrap();
        json_of(
            &mut b,
            "tool.input",
            json!({ "inputs": [{ "tool": "box" }, { "click": { "at": [0, 0, 0] } }, { "click": { "at": [100, 100, 0] } }, { "vcb": "10" }] }),
        );
        let text = b.editor.document().to_json().unwrap();
        let path = std::env::temp_dir().join(format!("solidsketch-mcp-{}.solidsketch", std::process::id()));
        std::fs::write(&path, text).unwrap();
        let mut opened = Headless::new(Some(path.clone())).unwrap();
        assert_eq!(json_of(&mut opened, "model.geometry", json!({ "volume": false }))["total"], 1);
        assert!(opened.describe().contains(&path.display().to_string()));
        let _ = std::fs::remove_file(&path);
        assert!(Headless::new(Some(path)).is_err());
    }
}
