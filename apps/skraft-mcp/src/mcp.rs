//! The Model Context Protocol over stdio (JSON-RPC 2.0, one message per line): the lifecycle
//! (`initialize`, `notifications/initialized`, `ping`) and the tools (`tools/list`,
//! `tools/call`). One tool per method of the control protocol the backend serves, named with
//! `_` for `.` (`model.geometry` is `model_geometry`), its input schema the method's
//! parameters'. A JSON result is a text block; a screenshot an image block; a method's refusal
//! a result flagged `isError`, its message for the model to read and correct.

use serde_json::{Value, json};
use skraft_control::{Answer, Call, Error, Method};

use crate::backend::Backend;

/// The protocol revisions understood, the newest first.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
Skraft: a simple direct-modelling 3D app on real solids (exact B-reps; parts with \
OpenCutList-like cut list dimensions). Every tool is a method of its control protocol (docs/control-protocol.md \
of the Skraft repository).
- Units: millimetres; world Z up. A part's `dims` are its length, width, thickness (along its red, green, blue axes).
- Drive tools with `tool_input` at world points (`at`: exact) — e.g. the box tool: `{\"tool\": \
\"box\"}`, two `click`s at the base's corners, then `{\"vcb\": \"18\"}` for its height.
- The kernel computes in the background in the app: after a change, `app_wait` with \
`{\"idle\": true}` before reading geometry or picking what it made.
- Read the model with `model_geometry` (summary; `brep`, `mesh` on demand), `model_document` \
(the recipe), `tool_state` (the active tool, its preview, the inference under the cursor).
- `ui_*` tools (when connected to the window) send real input: clicks on named widgets \
(`ui_handles` lists them) or points; `app_screenshot` / `ui_screenshot` show the view / window.
- `journal_log`, `journal_failures` tell what happened and what the kernel failed (its job, to \
replay headless); each entry's `origin` says whether the user or a pilot (`client`) caused it.
- Several apps may run (one per worktree): `app_state`'s `instance` says which one answers \
(build, worktree, who launched it), `app_instances` lists them all. Launch yours from your \
worktree with `SKRAFT_CONTROL=1` and a port of its own.";

/// The tool's name of `method`.
pub fn tool_name(method: &str) -> String {
    method.replace('.', "_")
}

pub struct Server<B: Backend> {
    backend: B,
    initialized: bool,
}

impl<B: Backend> Server<B> {
    pub fn new(backend: B) -> Self {
        Server { backend, initialized: false }
    }

    /// Answers one message (a line): the line to write back, none for a notification.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}")).to_string()),
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // A notification (no id): nothing answered.
        let id = id?;
        let reply = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tools() })),
            "tools/call" => self.call_tool(&params),
            "" => Err((-32600, "invalid request: no method".to_owned())),
            m => Err((-32601, format!("method not found: {m}"))),
        };
        Some(match reply {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
            Err((code, message)) => error(id, code, &message).to_string(),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        self.initialized = true;
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOLS[0]);
        let version = PROTOCOLS.iter().find(|p| **p == asked).copied().unwrap_or(PROTOCOLS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "skraft", "title": "Skraft", "version": env!("CARGO_PKG_VERSION") },
            "instructions": format!("{INSTRUCTIONS}\n\nThis server: {}", self.backend.describe()),
        })
    }

    /// The methods offered as tools: those the backend serves, but `app.ping` and
    /// `app.methods` (MCP has its own).
    fn methods(&self) -> Vec<Method> {
        skraft_control::methods().into_iter().filter(|m| !matches!(m.name, "app.ping" | "app.methods") && self.backend.serves(m)).collect()
    }

    fn tools(&self) -> Vec<Value> {
        self.methods()
            .into_iter()
            .map(|m| {
                let mut schema = serde_json::to_value((m.params)()).unwrap_or_else(|_| json!({ "type": "object" }));
                if let Some(o) = schema.as_object_mut() {
                    o.remove("$schema");
                    o.remove("title");
                    o.entry("type").or_insert(json!("object"));
                }
                json!({
                    "name": tool_name(m.name),
                    "description": m.summary,
                    "inputSchema": schema,
                    "annotations": { "readOnlyHint": read_only(m.name) },
                })
            })
            .collect()
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call: no `name`".to_owned()))?;
        let Some(method) = self.methods().into_iter().find(|m| tool_name(m.name) == name) else {
            return Err((-32602, format!("unknown tool {name:?}")));
        };
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        let outcome = self.backend.call(&Call::new(method.name, arguments));
        Ok(match outcome {
            Ok(Answer::Json(v)) => json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                "isError": false,
            }),
            Ok(Answer::Png(bytes)) => json!({
                "content": [{ "type": "image", "data": crate::base64::encode(&bytes), "mimeType": "image/png" }],
                "isError": false,
            }),
            Err(e) => json!({ "content": [{ "type": "text", "text": message(&e) }], "isError": true }),
        })
    }
}

fn message(e: &Error) -> String {
    e.message()
}

/// Whether a method only reads (the tools' `readOnlyHint`).
fn read_only(method: &str) -> bool {
    matches!(
        method,
        "app.state"
            | "app.wait"
            | "app.commands"
            | "app.screenshot"
            | "model.document"
            | "model.geometry"
            | "tool.state"
            | "view.project"
            | "view.pick"
            | "journal.log"
            | "journal.failures"
            | "ui.handles"
            | "ui.screenshot"
            | "ui.messages"
    )
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skraft_control::Outcome;

    /// A backend answering every editor method, recording what it was asked.
    #[derive(Default)]
    struct Fake {
        calls: Vec<Call>,
    }

    impl Backend for Fake {
        fn call(&mut self, call: &Call) -> Outcome {
            self.calls.push(call.clone());
            match call.method.as_str() {
                "app.screenshot" => Ok(Answer::Png(vec![1, 2, 3])),
                "tool.state" => Err(Error::BadParams("tool.state: unknown field `x`".into())),
                _ => Ok(Answer::Json(json!({ "ok": 1 }))),
            }
        }

        fn serves(&self, m: &Method) -> bool {
            m.headless || m.name == "app.screenshot"
        }

        fn describe(&self) -> String {
            "a fake".into()
        }
    }

    fn ask(s: &mut Server<Fake>, msg: Value) -> Value {
        serde_json::from_str(&s.handle(&msg.to_string()).expect("an answer")).unwrap()
    }

    #[test]
    fn the_lifecycle_and_the_tools() {
        let mut s = Server::new(Fake::default());
        let init = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(init["result"]["instructions"].as_str().unwrap().contains("a fake"));
        let newer =
            ask(&mut s, json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "2099-01-01" } }));
        assert_eq!(newer["result"]["protocolVersion"], PROTOCOLS[0]);
        // A notification: no answer.
        assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string()).is_none());
        assert_eq!(ask(&mut s, json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }))["result"], json!({}));

        let tools = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" }));
        let tools = tools["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"model_geometry") && names.contains(&"app_screenshot"), "{names:?}");
        // Not served (a window's), nor MCP's own.
        assert!(!names.contains(&"ui_click") && !names.contains(&"app_ping"), "{names:?}");
        let geometry = tools.iter().find(|t| t["name"] == "model_geometry").unwrap();
        assert_eq!(geometry["inputSchema"]["type"], "object");
        assert!(geometry["inputSchema"]["properties"]["brep"].is_object());
        assert_eq!(geometry["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn calls_reach_the_backend_and_come_back_as_content() {
        let mut s = Server::new(Fake::default());
        let r = ask(
            &mut s,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "model_geometry", "arguments": { "brep": true } } }),
        );
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("\"ok\""));
        assert_eq!(s.backend.calls[0], Call::new("model.geometry", json!({ "brep": true })));
        let png = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "app_screenshot" } }));
        assert_eq!(png["result"]["content"][0]["type"], "image");
        assert_eq!(png["result"]["content"][0]["data"], "AQID");
        // A refusal: a result flagged, for the model.
        let bad = ask(
            &mut s,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "tool_state", "arguments": { "x": 1 } } }),
        );
        assert_eq!(bad["result"]["isError"], true);
        assert!(bad["result"]["content"][0]["text"].as_str().unwrap().contains("unknown field"));
        // No such tool, no such method, not JSON: protocol errors.
        let unknown = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ui_click" } }));
        assert_eq!(unknown["error"]["code"], -32602);
        assert_eq!(ask(&mut s, json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/list" }))["error"]["code"], -32601);
        let garbage: Value = serde_json::from_str(&s.handle("{nope").unwrap()).unwrap();
        assert_eq!(garbage["error"]["code"], -32700);
    }
}
