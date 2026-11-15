//! The methods an [`Editor`] answers alone (docs/ARCHITECTURE.md §7 septies): the model and
//! the active tool. The window answers them through here, and so will a headless session.

use std::collections::HashMap;

use glam::{DMat3, DVec2, DVec3};
use serde_json::{Value, json};
use solidsketch_model::{Id, PlacedPart};
use solidsketch_tools::{Editor, Key, Modifiers, Projection, StandardView, ToolInput, ToolKind};

use crate::json::{self, v3};
use crate::params::{
    CameraParams, DocumentParams, GeometryParams, Input, KeyName, NoParams, PickParams, Pointer, ProjectParams, ToolInputParams, ViewName,
};
use crate::params::{FailuresParams, LogParams, RecordAction, RecordParams};
use crate::{Answer, Call, Error, Outcome};

/// Answers `call` if it is one of the editor's methods (`model.*`, `tool.*`, `view.*`);
/// `None` for the others.
pub fn call(e: &mut Editor, call: &Call) -> Option<Outcome> {
    if !is_editors(&call.method) {
        return None;
    }
    // What the kernel finished (a window does it every frame; a headless session has none):
    // the meshes picks and inferences need.
    e.poll();
    Some(match call.method.as_str() {
        "model.document" => call.params().and_then(|p| document(e, &p)),
        "model.geometry" => call.params().and_then(|p| geometry(e, &p)),
        "tool.state" => call.params::<NoParams>().map(|_| tool_state(e)),
        "tool.input" => call.params().and_then(|p| tool_input(e, &p)),
        "view.camera" => call.params().and_then(|p| camera(e, &p)),
        "view.project" => call.params().map(|p| project(e, &p)),
        "view.pick" => call.params().map(|p| pick(e, &p)),
        "journal.log" => call.params().map(|p| log(e, &p)),
        "journal.failures" => call.params().and_then(|p| failures(e, &p)),
        "journal.record" => call.params().and_then(|p| record(e, &p)),
        m => Err(Error::UnknownMethod(m.into())),
    })
}

/// Is `method` one the editor answers?
fn is_editors(method: &str) -> bool {
    ["model.", "tool.", "view.", "journal."].iter().any(|p| method.starts_with(p))
}

/// Is the editor idle: no kernel job, no mesh on its way, no load, no import?
pub fn idle(e: &Editor) -> bool {
    !e.busy() && !e.meshing() && e.loading().is_none() && !e.is_importing()
}

fn document(e: &Editor, p: &DocumentParams) -> Outcome {
    let text = e.document().to_json().map_err(|e| Error::Failed(e.to_string()))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| Error::Failed(e.to_string()))?;
    if !p.brep {
        json::sum_up_breps(&mut v);
    }
    Ok(Answer::Json(json!({ "revision": e.history().revision(), "busy": e.busy(), "file": v })))
}

fn geometry(e: &Editor, p: &GeometryParams) -> Outcome {
    let doc = e.document();
    let filter: Option<Vec<Id>> = match (&p.ids, p.selection) {
        (_, true) => Some(e.selection().to_vec()),
        (Some(ids), false) => Some(ids.iter().map(|i| Id(*i)).collect()),
        (None, false) => None,
    };
    let placed: Vec<PlacedPart> = doc
        .placed_parts()
        .into_iter()
        .filter(|pp| filter.as_ref().is_none_or(|ids| pp.path.instances.iter().chain([&pp.path.entity]).any(|i| ids.contains(i))))
        .collect();
    let mut volumes = HashMap::new();
    let parts: Vec<Value> = placed.iter().skip(p.offset).take(p.limit()).map(|pp| part(e, pp, p, &mut volumes)).collect();
    Ok(Answer::Json(json!({
        "revision": e.history().revision(),
        // The kernel is computing: what follows is the document before its result.
        "stale": e.busy(),
        "total": placed.len(),
        "offset": p.offset,
        "count": parts.len(),
        "parts": parts,
    })))
}

/// The box of `solid` in its own axes: the exact one the scene has from the kernel, else the
/// kernel's (engine), else its vertices'; and which.
fn local_bounds(e: &Editor, pp: &PlacedPart) -> Option<((DVec3, DVec3), &'static str)> {
    if let Some(b) = e.scene().bounds_of(&pp.shape) {
        return Some((b, "exact"));
    }
    #[cfg(any(test, feature = "engine"))]
    if let Ok(b) = pp.shape.bounds() {
        return Some((b, "exact"));
    }
    pp.shape.vertex_bounds().map(|b| (b, "vertices"))
}

fn part(e: &Editor, pp: &PlacedPart, p: &GeometryParams, volumes: &mut HashMap<usize, Option<f64>>) -> Value {
    let doc = e.document();
    let occurrence = doc.resolve(&pp.path).ok();
    let component = doc.component(pp.part).ok();
    let part = doc.part(pp.part).ok();
    let mut out = json!({
        "path": json::path(&pp.path),
        "part": pp.part.0,
        "name": component.map(|c| c.name.as_str()),
        "occurrence_name": occurrence.map(|o| o.name.as_str()).filter(|n| !n.is_empty()),
        "hidden": pp.hidden_at.is_some(),
        "material": pp.material.map(|m| m.0),
        "transform": json::affine(&pp.transform),
        "pieces": pp.shape.pieces(),
    });
    let (faces, edges, vertices) = pp.shape.topology_counts();
    out["topology"] = json!({ "faces": faces, "edges": edges, "vertices": vertices });
    if let Some(((lo, hi), source)) = local_bounds(e, pp) {
        let size = hi - lo;
        let mut dims = size.to_array();
        dims.sort_by(|a, b| b.total_cmp(a));
        let corners = (0..8).map(|i| {
            pp.transform.transform_point3(DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            ))
        });
        let world = corners.fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, b), c| (a.min(c), b.max(c)));
        out["bounds"] = json!({ "local": json::bounds((lo, hi)), "world": json::bounds(world), "source": source });
        out["size"] = v3(size);
        // OpenCutList's: length, width, thickness — the part's box, largest first.
        out["dims"] = json!(dims);
    }
    if p.volume.unwrap_or(true) {
        let key = std::sync::Arc::as_ptr(&pp.shape) as usize;
        let volume = *volumes.entry(key).or_insert_with(|| volume(&pp.shape));
        out["volume"] = json!(volume);
    }
    if let Some(part) = part {
        out["bodies"] = Value::Array(
            part.bodies
                .iter()
                .map(|b| {
                    json!({
                        "id": b.id.0,
                        "def": b.def.0,
                        "sign": b.sign,
                        "placement": json::affine(&b.placement),
                        "profiled": part.def_of(b).profile.is_some(),
                        "failed": part.is_failed(b.id),
                    })
                })
                .collect(),
        );
    }
    if p.brep {
        out["brep"] = serde_json::to_value(&*pp.shape).unwrap_or(Value::Null);
    }
    if p.mesh {
        out["mesh"] = e.scene().part_at(&pp.path).map_or(Value::Null, |(_, m)| serde_json::to_value(&*m.mesh).unwrap_or(Value::Null));
    }
    out
}

#[cfg(any(test, feature = "engine"))]
fn volume(solid: &solidsketch_model::kernel::Solid) -> Option<f64> {
    solid.volume().ok()
}

#[cfg(not(any(test, feature = "engine")))]
fn volume(_: &solidsketch_model::kernel::Solid) -> Option<f64> {
    None
}

fn tool_state(e: &Editor) -> Answer {
    let preview = e.preview();
    let options: Vec<Value> = e
        .tool_options()
        .into_iter()
        .map(|o| {
            json!({
                "key": o.key,
                "label": o.label,
                "value": o.value,
                "choices": o.choices.iter().map(|c| c.value).collect::<Vec<_>>(),
            })
        })
        .collect();
    Answer::Json(json!({
        "tool": e.tool_kind().id(),
        "status": e.status(),
        "busy": e.busy(),
        "revision": e.history().revision(),
        "copying": e.tool_copying(),
        "vcb": e.vcb().map(|(label, shown)| json!({ "label": label, "shown": shown })),
        "takes_typing": e.takes_typing(),
        "axis_lock": e.axis_lock(),
        "face_lock": e.face_lock().map(v3),
        "options": options,
        "selection": json::ids(e.selection()),
        "inference": preview.inference.as_ref().map(json::inference),
        "preview": json::preview(&preview),
    }))
}

// ---- tool.input ------------------------------------------------------------------------------

/// The pixel `p` lands on: its world point projected, or its pixel.
fn pixel(e: &Editor, p: &Pointer) -> Result<DVec2, Error> {
    match (p.at, p.px) {
        (Some(at), None) => {
            e.camera.project(DVec3::from_array(at), e.viewport_size()).ok_or_else(|| Error::Failed(format!("{at:?} is behind the camera")))
        }
        (None, Some(px)) => Ok(DVec2::from_array(px)),
        _ => Err(Error::BadParams("a pointer is `at` (a world point) or `px` (a pixel), one of them".into())),
    }
}

fn mods(toggle: bool, copy: bool) -> Modifiers {
    Modifiers { toggle, copy }
}

fn key(k: KeyName) -> Key {
    match k {
        KeyName::Escape => Key::Escape,
        KeyName::Delete => Key::Delete,
        KeyName::Enter => Key::Enter,
        KeyName::Left => Key::Left,
        KeyName::Right => Key::Right,
        KeyName::Up => Key::Up,
        KeyName::Down => Key::Down,
        KeyName::Copy => Key::Copy,
        KeyName::Shift => Key::Shift,
        KeyName::Alt => Key::Alt,
        KeyName::Tab => Key::Tab,
    }
}

/// The editor's inputs one input gives, the cursor's pixels resolved now (an input before may
/// have moved the camera… it does not, but the tool may have changed the scene).
fn expand(e: &Editor, input: &Input) -> Result<Vec<ToolInput>, Error> {
    Ok(match input {
        Input::Hover(p) => vec![ToolInput::Hover { px: pixel(e, p)? }],
        Input::Click(p) => {
            let px = pixel(e, p)?;
            vec![ToolInput::Hover { px }, ToolInput::Click { px, mods: mods(p.toggle, p.copy) }]
        }
        Input::DoubleClick(p) => {
            let px = pixel(e, p)?;
            vec![ToolInput::Hover { px }, ToolInput::DoubleClick { px, mods: mods(p.toggle, p.copy) }]
        }
        Input::Drag(d) => {
            let (from, to) = (pixel(e, &d.from)?, pixel(e, &d.to)?);
            let steps = d.steps.unwrap_or(8).clamp(1, 1000);
            let mut out = vec![ToolInput::Hover { px: from }, ToolInput::DragStart { px: from, mods: mods(d.from.toggle, d.from.copy) }];
            out.extend((1..=steps).map(|k| ToolInput::Drag { px: from.lerp(to, k as f64 / steps as f64) }));
            out.push(ToolInput::DragEnd { px: to, mods: mods(d.to.toggle, d.to.copy) });
            out
        }
        Input::Key(k) => vec![ToolInput::Key { key: key(k.name), mods: mods(k.toggle, k.copy) }],
        Input::Vcb(text) => vec![ToolInput::Vcb(text.clone())],
        Input::Tool(_) => Vec::new(),
    })
}

fn tool_input(e: &mut Editor, p: &ToolInputParams) -> Outcome {
    let mut done = Vec::new();
    for (i, input) in p.inputs.iter().enumerate() {
        let refused = |err: Error| Error::Failed(format!("input {i}: {}", err.message()));
        if let Input::Tool(id) = input {
            let kind = ToolKind::from_id(id).ok_or_else(|| refused(Error::BadParams(format!("no tool {id:?}"))))?;
            e.set_tool(kind);
            done.push(json!({ "tool": id }));
            continue;
        }
        let inputs = expand(e, input).map_err(refused)?;
        let px = inputs.iter().find_map(|t| match t {
            ToolInput::Hover { px } => Some(json::v2(*px)),
            _ => None,
        });
        for t in inputs {
            e.input(t);
        }
        done.push(json!({ "px": px, "status": e.status() }));
    }
    let mut v = match tool_state(e) {
        Answer::Json(v) => v,
        Answer::Png(_) => unreachable!(),
    };
    v["inputs"] = Value::Array(done);
    Ok(Answer::Json(v))
}

// ---- view.* ------------------------------------------------------------------------------------

/// Steepest pitch a camera takes (as `Camera`'s).
const MAX_PITCH: f64 = 89.99 * std::f64::consts::PI / 180.0;

fn camera(e: &mut Editor, p: &CameraParams) -> Outcome {
    if let Some(perspective) = p.perspective {
        e.set_parallel(!perspective);
    }
    if let Some(v) = p.view {
        e.standard_view(match v {
            ViewName::Top => StandardView::Top,
            ViewName::Front => StandardView::Front,
            ViewName::Right => StandardView::Right,
            ViewName::Back => StandardView::Back,
            ViewName::Left => StandardView::Left,
            ViewName::Iso => StandardView::Iso,
        });
    }
    match (p.eye, p.target) {
        (Some(eye), Some(target)) => {
            let (eye, target) = (DVec3::from_array(eye), DVec3::from_array(target));
            let d = target - eye;
            let length = d.length();
            if length < 1e-9 {
                return Err(Error::BadParams("`eye` and `target` are the same point".into()));
            }
            // Yaw and pitch in the camera's frame (its Z up on screen).
            let local = e.camera.frame.inverse() * (d / length);
            e.camera.eye = eye;
            e.camera.yaw = local.y.atan2(local.x);
            e.camera.pitch = local.z.clamp(-1.0, 1.0).asin().clamp(-MAX_PITCH, MAX_PITCH);
            e.camera.focus = length;
        }
        (None, None) => {}
        _ => return Err(Error::BadParams("`eye` and `target` go together".into())),
    }
    match (&mut e.camera.projection, p.fov_deg, p.height) {
        (Projection::Perspective { fov_y }, Some(fov), _) if fov > 0.0 && fov < 180.0 => *fov_y = fov.to_radians(),
        (Projection::Parallel { height }, _, Some(h)) if h > 0.0 => *height = h,
        (_, None, None) => {}
        _ => return Err(Error::BadParams("`fov_deg` (0..180) goes with a perspective, `height` (> 0) with a parallel view".into())),
    }
    if p.fit {
        e.zoom_extents();
    }
    Ok(Answer::Json(camera_json(e)))
}

fn camera_json(e: &Editor) -> Value {
    let c = &e.camera;
    let size = e.viewport_size();
    let frame: DMat3 = c.frame;
    json!({
        "eye": v3(c.eye),
        "target": v3(c.eye + c.forward() * c.focus),
        "forward": v3(c.forward()),
        "up": v3(c.up()),
        "right": v3(c.right()),
        "focus": c.focus,
        "yaw_deg": c.yaw.to_degrees(),
        "pitch_deg": c.pitch.to_degrees(),
        "frame": { "x_axis": v3(frame.x_axis), "y_axis": v3(frame.y_axis), "z_axis": v3(frame.z_axis) },
        "projection": match c.projection {
            Projection::Perspective { fov_y } => json!({ "perspective": true, "fov_deg": fov_y.to_degrees() }),
            Projection::Parallel { height } => json!({ "perspective": false, "height": height }),
        },
        "viewport": json::v2(size),
        "free_area": e.free_area().map(|(lo, hi)| json!({ "min": json::v2(lo), "max": json::v2(hi) })),
    })
}

fn project(e: &Editor, p: &ProjectParams) -> Answer {
    let size = e.viewport_size();
    let points: Vec<Value> = p
        .points
        .iter()
        .map(|q| match e.camera.project(DVec3::from_array(*q), size) {
            Some(px) => json!({ "px": json::v2(px), "in_view": px.x >= 0.0 && px.y >= 0.0 && px.x <= size.x && px.y <= size.y }),
            None => json!({ "px": null, "in_view": false }),
        })
        .collect();
    Answer::Json(json!({ "viewport": json::v2(size), "points": points }))
}

fn pick(e: &Editor, p: &PickParams) -> Answer {
    let size = e.viewport_size();
    let pixels = p.px.iter().map(|q| Some(DVec2::from_array(*q))).chain(p.at.iter().map(|q| e.camera.project(DVec3::from_array(*q), size)));
    let doc = e.document();
    let hits: Vec<Value> = pixels
        .map(|px| {
            let Some(px) = px else { return json!({ "px": null, "hit": null }) };
            let ray = e.camera.ray(px, size);
            let hit = e.scene().pick(&ray, |_| true).map(|h| {
                let part = doc.resolve_part(&h.path).ok();
                json!({
                    "path": json::path(&h.path),
                    "part": part.map(|(id, _, _)| id.0),
                    "name": part.and_then(|(id, _, _)| doc.component(id).ok()).map(|c| c.name.as_str()),
                    "face": h.face,
                    "point": v3(h.point),
                    "normal": v3(h.normal),
                    "distance": h.t,
                })
            });
            json!({ "px": json::v2(px), "hit": hit })
        })
        .collect();
    Answer::Json(json!({ "hits": hits }))
}

// ---- journal.* ---------------------------------------------------------------------------------

fn log(e: &Editor, p: &LogParams) -> Answer {
    let j = e.journal();
    let all: Vec<_> = j.since(p.since.unwrap_or(0)).collect();
    let limit = p.limit.unwrap_or(crate::params::LOG_DEFAULT_LIMIT);
    let entries: Vec<Value> = all[all.len().saturating_sub(limit)..].iter().map(|en| json::entry(en)).collect();
    Answer::Json(json!({
        "entries": entries,
        "left_out": all.len().saturating_sub(limit),
        // The `since` that gives what comes after these.
        "last": j.next_seq() - 1,
    }))
}

fn failures(e: &Editor, p: &FailuresParams) -> Outcome {
    let j = e.journal();
    match p.seq {
        Some(seq) => j
            .failures()
            .find(|f| f.seq == seq)
            .map(|f| Answer::Json(json::failure(f, true)))
            .ok_or_else(|| Error::Failed(format!("no failure {seq} (kept: the last {})", solidsketch_tools::journal::FAILURES_LEN))),
        None => Ok(Answer::Json(json!({ "failures": j.failures().map(|f| json::failure(f, false)).collect::<Vec<_>>() }))),
    }
}

fn record(e: &mut Editor, p: &RecordParams) -> Outcome {
    match p.action {
        RecordAction::Status => {
            let r = e.journal().recording();
            Ok(Answer::Json(json!({ "recording": r.is_some(), "steps": r.map(|r| r.steps.len()) })))
        }
        RecordAction::Start => {
            e.record(true);
            Ok(Answer::Json(json!({ "recording": true, "revision": e.history().revision() })))
        }
        RecordAction::Stop => {
            let name = p.name.clone().unwrap_or_else(|| "recorded".into());
            if !crate::record::valid_name(&name) {
                return Err(Error::BadParams(format!("{name:?}: a test's name is a Rust identifier in lower case")));
            }
            let r = e.record(false).ok_or_else(|| Error::Failed("no recording under way (`\"action\": \"start\"` first)".into()))?;
            let reached = crate::record::Reached { revision: e.history().revision(), parts: e.document().placed_parts().len() };
            Ok(Answer::Json(json!({
                "steps": r.steps.len(),
                "test_file": format!("crates/tools/tests/{name}.rs"),
                "test": crate::record::test_file(&r, &name, &reached),
                "document_file": format!("crates/tools/tests/{name}.solidsketch"),
                "document": r.document,
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec2, DVec3};
    use solidsketch_model::Document;
    use solidsketch_tools::{StandardView, ToolInput, ToolKind};

    use super::*;

    fn editor() -> Editor {
        let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
        e.set_viewport_size(DVec2::new(1000.0, 1000.0));
        e.set_parallel(true);
        e.camera.set_standard_view(
            StandardView::Top,
            glam::DMat3::IDENTITY,
            Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
            solidsketch_tools::ViewArea::full(e.viewport_size()),
        );
        e
    }

    fn click(e: &mut Editor, p: DVec3) {
        let at = e.camera.project(p, e.viewport_size()).unwrap();
        e.input(ToolInput::Hover { px: at });
        e.input(ToolInput::Click { px: at, mods: Default::default() });
    }

    fn json_of(e: &mut Editor, method: &str, params: Value) -> Value {
        match call(e, &Call::new(method, params)).expect("an editor method") {
            Ok(Answer::Json(v)) => v,
            other => panic!("{other:?}"),
        }
    }

    /// A box of 600 × 400 × 18 drawn with the tool.
    fn draw_box(e: &mut Editor) {
        e.set_tool(ToolKind::Box);
        click(e, DVec3::ZERO);
        click(e, DVec3::new(600.0, 400.0, 0.0));
        e.input(ToolInput::Vcb("18".into()));
        e.set_tool(ToolKind::Select);
    }

    #[test]
    fn geometry_gives_sizes_bounds_and_volume() {
        let mut e = editor();
        assert_eq!(json_of(&mut e, "model.geometry", Value::Null)["total"], 0);
        draw_box(&mut e);
        let g = json_of(&mut e, "model.geometry", Value::Null);
        assert_eq!(g["total"], 1, "{g}");
        let part = &g["parts"][0];
        let dims: Vec<f64> = serde_json::from_value(part["dims"].clone()).unwrap();
        // Clicked at world points: exactly there (the projection is f64, the inverse of the ray).
        assert!((dims[0] - 600.0).abs() < 1e-9 && (dims[1] - 400.0).abs() < 1e-9 && (dims[2] - 18.0).abs() < 1e-9, "{part}");
        let volume = part["volume"].as_f64().unwrap();
        assert!((volume - 600.0 * 400.0 * 18.0).abs() < 1.0, "{volume}");
        assert_eq!(part["pieces"], 1);
        assert_eq!(part["topology"]["faces"], 6);
        assert_eq!(part["bodies"][0]["sign"], "positive");
        assert!(part.get("brep").is_none() && part.get("mesh").is_none());
        // Filtered: by an id that places nothing, nothing.
        assert_eq!(json_of(&mut e, "model.geometry", json!({ "ids": [999_999] }))["total"], 0);
        // Its B-rep on demand, without the volume.
        let g = json_of(&mut e, "model.geometry", json!({ "brep": true, "volume": false }));
        assert_eq!(g["parts"][0]["brep"]["faces"].as_array().unwrap().len(), 6);
        assert!(g["parts"][0].get("volume").is_none());
    }

    #[test]
    fn the_document_sums_up_its_solids_unless_asked() {
        let mut e = editor();
        draw_box(&mut e);
        let d = json_of(&mut e, "model.document", Value::Null);
        let text = d["file"].to_string();
        assert!(text.contains("\"brep\":{\"edges\":12,\"faces\":6,\"vertices\":8}"), "{text}");
        let full = json_of(&mut e, "model.document", json!({ "brep": true }))["file"].to_string();
        assert!(full.len() > text.len());
        assert!(!full.contains("\"brep\":{"));
    }

    #[test]
    fn tool_state_shows_the_tool_at_work() {
        let mut e = editor();
        e.set_tool(ToolKind::Box);
        click(&mut e, DVec3::ZERO);
        let at = e.camera.project(DVec3::new(300.0, 200.0, 0.0), e.viewport_size()).unwrap();
        e.input(ToolInput::Hover { px: at });
        let s = json_of(&mut e, "tool.state", Value::Null);
        assert_eq!(s["tool"], "box", "{s}");
        assert!(s["vcb"].is_object(), "{s}");
        // The rectangle being drawn, its sides written.
        let labels: Vec<&str> = s["preview"]["labels"].as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap()).collect();
        assert_eq!(labels, ["300", "200"], "{s}");
        assert_eq!(s["preview"]["lines"].as_array().unwrap().len(), 4, "{s}");
        assert!(s["preview"].as_object().is_some_and(|p| !p.is_empty()), "{s}");
    }

    #[test]
    fn other_methods_are_not_the_editors() {
        let mut e = editor();
        assert!(call(&mut e, &Call::new("app.state", Value::Null)).is_none());
        let bad = call(&mut e, &Call::new("tool.state", json!({ "x": 1 }))).unwrap();
        assert!(matches!(bad, Err(Error::BadParams(_))));
        assert!(idle(&e));
    }
    #[test]
    fn a_box_drawn_by_inputs_alone_then_picked() {
        let mut e = editor();
        let r = json_of(
            &mut e,
            "tool.input",
            json!({ "inputs": [
                { "tool": "box" },
                { "click": { "at": [0, 0, 0] } },
                { "click": { "at": [600, 400, 0] } },
                { "vcb": "18" },
                { "tool": "select" },
            ] }),
        );
        assert_eq!(r["tool"], "select", "{r}");
        assert_eq!(r["inputs"].as_array().unwrap().len(), 5);
        assert_eq!(r["revision"], 1, "{r}");
        let g = json_of(&mut e, "model.geometry", json!({ "volume": false }));
        assert_eq!(g["total"], 1, "{g}");
        // Seen from above: the box's top face, 18 above the ground, facing up (the pixels first,
        // then the points).
        let p = json_of(&mut e, "view.pick", json!({ "at": [[300, 200, 18]], "px": [[1, 1]] }));
        let hit = &p["hits"][1]["hit"];
        let point: Vec<f64> = serde_json::from_value(hit["point"].clone()).unwrap();
        assert!((point[0] - 300.0).abs() < 1e-9 && (point[1] - 200.0).abs() < 1e-9 && (point[2] - 18.0).abs() < 1e-9, "{p}");
        assert!((hit["normal"][2].as_f64().unwrap() - 1.0).abs() < 1e-6, "{p}");
        assert_eq!(hit["path"], g["parts"][0]["path"]);
        // The corner pixel sees nothing.
        assert!(p["hits"][0]["hit"].is_null(), "{p}");
        // In perspective, a point behind the eye is no pixel: the input says which one.
        json_of(&mut e, "view.camera", json!({ "perspective": true, "eye": [0, -1000, 1000], "target": [0, 0, 0] }));
        let behind = call(&mut e, &Call::new("tool.input", json!({ "inputs": [{ "hover": { "at": [0, -2000, 2000] } }] }))).unwrap();
        assert!(matches!(&behind, Err(Error::Failed(m)) if m.starts_with("input 0")), "{behind:?}");
        let both = call(&mut e, &Call::new("tool.input", json!({ "inputs": [{ "hover": { "at": [0, 0, 0], "px": [1, 1] } }] }))).unwrap();
        assert!(both.is_err());
        let tool = call(&mut e, &Call::new("tool.input", json!({ "inputs": [{ "tool": "hammer" }] }))).unwrap();
        assert!(tool.is_err());
    }

    #[test]
    fn the_camera_is_read_set_and_projects() {
        let mut e = editor();
        let c = json_of(&mut e, "view.camera", Value::Null);
        assert_eq!(c["projection"]["perspective"], false, "{c}");
        assert_eq!(c["viewport"], json!([1000.0, 1000.0]));
        // Set by its eye and target, in perspective.
        let c =
            json_of(&mut e, "view.camera", json!({ "perspective": true, "eye": [2000, -2000, 1000], "target": [0, 0, 0], "fov_deg": 40 }));
        let target: Vec<f64> = serde_json::from_value(c["target"].clone()).unwrap();
        assert!(target.iter().all(|t| t.abs() < 1e-6), "{c}");
        assert!((c["projection"]["fov_deg"].as_f64().unwrap() - 40.0).abs() < 1e-9, "{c}");
        // What it looks at is in the middle of the view.
        let p = json_of(&mut e, "view.project", json!({ "points": [[0, 0, 0], [-2.0e4, 2.0e4, -1.0e4]] }));
        let px: Vec<f64> = serde_json::from_value(p["points"][0]["px"].clone()).unwrap();
        assert!((px[0] - 500.0).abs() < 1e-6 && (px[1] - 500.0).abs() < 1e-6, "{p}");
        assert_eq!(p["points"][0]["in_view"], true);
        // A standard view, framed.
        let c = json_of(&mut e, "view.camera", json!({ "view": "front", "fit": true }));
        assert!(c["pitch_deg"].as_f64().unwrap().abs() < 1e-9, "{c}");
        // Refused: a height for a perspective, an eye alone.
        assert!(call(&mut e, &Call::new("view.camera", json!({ "height": 100 }))).unwrap().is_err());
        assert!(call(&mut e, &Call::new("view.camera", json!({ "eye": [0, 0, 1] }))).unwrap().is_err());
    }

    #[test]
    fn every_tool_is_found_by_its_id() {
        for k in ToolKind::ALL {
            assert_eq!(ToolKind::from_id(k.id()), Some(k));
        }
    }
    #[test]
    fn the_journal_tells_what_happened() {
        let mut e = editor();
        let first = json_of(&mut e, "journal.log", Value::Null)["last"].as_u64().unwrap();
        draw_box(&mut e);
        let log = json_of(&mut e, "journal.log", json!({ "since": first }));
        let events: Vec<&Value> = log["entries"].as_array().unwrap().iter().map(|en| &en["event"]).collect();
        assert_eq!(events[0], &json!({ "tool": "box" }), "{log}");
        assert!(events.iter().any(|ev| ev["click"].is_object()), "{log}");
        assert!(events.iter().any(|ev| ev["vcb"] == "18"), "{log}");
        assert!(events.iter().any(|ev| ev["job"]["for"] == "operation" && ev["job"]["error"].is_null()), "{log}");
        assert!(events.iter().any(|ev| ev["applied"]["revision"] == 1), "{log}");
        // Nothing after the last one.
        let last = log["last"].as_u64().unwrap();
        assert!(json_of(&mut e, "journal.log", json!({ "since": last }))["entries"].as_array().unwrap().is_empty());
        assert_eq!(json_of(&mut e, "journal.log", json!({ "since": first, "limit": 2 }))["entries"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_kernel_failure_keeps_its_job_which_fails_again() {
        use solidsketch_model::{Boolean, EntityPath, Operation, Primitive};
        let mut e = editor();
        let root = e.document().root();
        let add = |e: &mut Editor, origin: f64, size: f64| {
            let primitive = Primitive::Box { origin: DVec3::splat(origin), size: DVec3::splat(size) };
            e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY }).unwrap().unwrap().created[0]
        };
        let small = add(&mut e, 0.0, 10.0);
        let huge = add(&mut e, -100.0, 1000.0);
        // The small box minus one swallowing it: nothing left, the kernel fails.
        let op = Operation::Boolean {
            target: EntityPath::root(small),
            tool: EntityPath::root(huge),
            op: Boolean::Difference,
            scope: Default::default(),
        };
        assert!(e.apply(op).is_err());
        let all = json_of(&mut e, "journal.failures", Value::Null);
        let failures = all["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1, "{all}");
        assert!(failures[0].get("job").is_none());
        let seq = failures[0]["seq"].as_u64().unwrap();
        let full = json_of(&mut e, "journal.failures", json!({ "seq": seq }));
        assert!(full["operation"].is_object(), "{full}");
        // Its job, read back and run again: the same failure, headless.
        let job: solidsketch_kernel::Job = serde_json::from_value(full["job"].clone()).unwrap();
        assert_eq!(job.inputs.len(), 2);
        assert!(solidsketch_kernel::run(&job).is_err());
        assert!(call(&mut e, &Call::new("journal.failures", json!({ "seq": 999_999 }))).unwrap().is_err());
    }

    #[test]
    fn a_recording_is_a_test_of_the_inputs() {
        let mut e = editor();
        let started = json_of(&mut e, "journal.record", json!({ "action": "start" }));
        assert_eq!(started["recording"], true);
        draw_box(&mut e);
        // What the editor does itself (its own hovers, the tool it goes back to) is not the
        // recording's: only the calls from outside.
        let status = json_of(&mut e, "journal.record", Value::Null);
        let steps = status["steps"].as_u64().unwrap();
        assert!((6..=9).contains(&steps), "{status}");
        let bad = call(&mut e, &Call::new("journal.record", json!({ "action": "stop", "name": "Bad Name" }))).unwrap();
        assert!(bad.is_err());
        let done = json_of(&mut e, "journal.record", json!({ "action": "stop", "name": "a_box" }));
        let test = done["test"].as_str().unwrap();
        assert!(test.contains("fn a_box()") && test.contains("include_str!(\"a_box.solidsketch\")"), "{test}");
        assert!(test.contains("ToolInput::Vcb(\"18\".into())"), "{test}");
        assert!(test.contains("assert_eq!(e.history().revision(), 1"), "{test}");
        assert!(solidsketch_model::Document::from_json(done["document"].as_str().unwrap()).is_ok());
        assert_eq!(json_of(&mut e, "journal.record", Value::Null)["recording"], false);
    }
}
