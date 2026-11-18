//! The editor's values as JSON: points as `[x, y, z]` (mm, world unless said), transforms
//! by their axes and translation, a tool's preview and inference as data.

use glam::{DAffine3, DVec2, DVec3};
use serde_json::{Value, json};
use skraft_model::{EntityPath, Id};
use skraft_tools::draw;
use skraft_tools::journal::{Entry, Event, Failure, JobKind};
use skraft_tools::tools::Preview;
use skraft_tools::{Inference, Modifiers, ToolInput};

pub fn v3(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}

pub fn v2(v: DVec2) -> Value {
    json!([v.x, v.y])
}

pub fn ids(ids: &[Id]) -> Value {
    json!(ids.iter().map(|i| i.0).collect::<Vec<_>>())
}

/// A path as `{instances, entity}`, by ids.
pub fn path(p: &EntityPath) -> Value {
    json!({ "instances": ids(&p.instances), "entity": p.entity.0 })
}

/// A transform by its axes (columns) and translation.
pub fn affine(t: &DAffine3) -> Value {
    json!({
        "x_axis": v3(t.matrix3.x_axis),
        "y_axis": v3(t.matrix3.y_axis),
        "z_axis": v3(t.matrix3.z_axis),
        "translation": v3(t.translation),
    })
}

pub fn bounds((lo, hi): (DVec3, DVec3)) -> Value {
    json!({ "min": v3(lo), "max": v3(hi) })
}

fn color(c: draw::Color) -> Value {
    match c {
        draw::Color::Rgba(c) => json!(c),
        draw::Color::Theme(t) => debug(&t),
    }
}

/// A pen, its defaults left out.
fn pen(p: &draw::Pen) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("color".into(), color(p.color));
    out.insert("width".into(), json!(p.width));
    if p.stipple != draw::Stipple::Solid {
        out.insert("stipple".into(), debug(&p.stipple));
    }
    if p.stipple != draw::Stipple::Solid && p.lay != draw::Lay::Model {
        out.insert("lay".into(), debug(&p.lay));
    }
    if p.layer != draw::Layer::Scene {
        out.insert("layer".into(), debug(&p.layer));
    }
    if p.blended {
        out.insert("blended".into(), json!(true));
    }
    if let Some(h) = p.hidden {
        out.insert("hidden".into(), debug(&h.stipple));
    }
    Value::Object(out)
}

/// What a tool or the editor draws through the drawer (docs/render/drawer.md). Empty
/// lists are left out.
pub fn drawing(d: &draw::Drawing) -> Value {
    let mut out = serde_json::Map::new();
    let mut put = |k: &str, v: Vec<Value>| {
        if !v.is_empty() {
            out.insert(k.into(), Value::Array(v));
        }
    };
    put("lines", d.lines.iter().map(|l| json!({ "a": v3(l.a), "b": v3(l.b), "pen": pen(&l.pen) })).collect());
    put(
        "fills",
        d.fills
            .iter()
            .map(|f| {
                json!({
                    "corners": f.corners.iter().map(|c| v3(*c)).collect::<Vec<_>>(),
                    "triangles": f.triangles,
                    "color": color(f.brush.color),
                    "layer": debug(&f.brush.layer),
                })
            })
            .collect(),
    );
    put(
        "boxes",
        d.boxes
            .iter()
            .map(|b| {
                json!({
                    "frame": affine(&b.bounds.frame),
                    "min": v3(b.bounds.lo),
                    "max": v3(b.bounds.hi),
                    "pen": pen(&b.pen),
                    "corners": b.corners,
                    "shell_on_top": b.shell_on_top,
                })
            })
            .collect(),
    );
    put("grips", d.grips.iter().map(|q| json!({ "at": v3(q.at), "shape": debug(&q.shape) })).collect());
    put("marks", d.marks.iter().map(|m| v3(*m)).collect());
    put(
        "lengths",
        d.lengths.iter().map(|l| json!({ "a": v3(l.a), "b": v3(l.b), "text": l.text, "slot": l.slot, "hidden": l.hidden })).collect(),
    );
    Value::Object(out)
}

fn debug(v: &impl std::fmt::Debug) -> Value {
    Value::String(format!("{v:?}"))
}

pub fn inference(i: &Inference) -> Value {
    json!({
        "point": v3(i.point),
        "kind": debug(&i.kind),
        "path": i.path.as_ref().map(path),
        "hit": i.hit.as_ref().map(|h| json!({
            "path": path(&h.path),
            "piece": debug(&h.piece),
            "face": h.face,
            "point": v3(h.point),
            "normal": v3(h.normal),
        })),
        "line": i.line.map(|(o, d)| json!({ "origin": v3(o), "direction": v3(d) })),
        "line2": i.line2.map(|(o, d)| json!({ "origin": v3(o), "direction": v3(d) })),
        "edge": i.edge.as_ref().map(debug),
        "crossed": i.crossed.len(),
    })
}

/// A tool's preview: what it draws, as data. Empty lists are left out.
pub fn preview(p: &Preview) -> Value {
    let mut out = serde_json::Map::new();
    let mut put = |k: &str, v: Value, empty: bool| {
        if !empty {
            out.insert(k.into(), v);
        }
    };
    put("hover", json!(p.hover.map(|i| i.0)), p.hover.is_none());
    put("hover_solid", p.hover_solid.as_ref().map_or(Value::Null, path), p.hover_solid.is_none());
    put("picked_solid", p.picked_solid.as_ref().map_or(Value::Null, path), p.picked_solid.is_none());
    put("ghost", p.ghost.as_ref().map_or(Value::Null, affine), p.ghost.is_none());
    put("copies", Value::Array(p.copies.iter().map(affine).collect()), p.copies.is_empty());
    put("tip", json!(p.tip), p.tip.is_none());
    put(
        "marquee",
        p.marquee
            .as_ref()
            .map_or(Value::Null, |m| json!({ "from": v2(m.from), "to": v2(m.to), "crossing": m.crossing, "hits": ids(&m.hits) })),
        p.marquee.is_none(),
    );
    put("propagate", p.propagate.as_ref().map_or(Value::Null, path), p.propagate.is_none());
    put("section", p.section.as_ref().map_or(Value::Null, debug), p.section.is_none());
    put(
        "stretch",
        p.stretch.as_ref().map_or(Value::Null, |s| {
            json!({
                "context": ids(&s.context),
                "occurrences": ids(&s.occurrences),
                "bodies": s.bodies.as_deref().map(ids),
                "frame": affine(&s.frame),
                "all": s.all,
                "axis": v3(s.axis),
                "cuts": s.cuts,
                "moves": s.moves,
            })
        }),
        p.stretch.is_none(),
    );
    put("drawing", drawing(&p.drawing), p.drawing.is_empty());
    put("hide_selection", json!(true), !p.hide_selection);
    put("erasing", ids(&p.erasing), p.erasing.is_empty());
    Value::Object(out)
}

/// Replaces every B-rep record in `v` (an object with `faces`, `edges` and `vertices` lists)
/// by its counts.
pub fn sum_up_breps(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let count = |k: &str| m.get(k).and_then(Value::as_array).map(Vec::len);
            if let (Some(faces), Some(edges), Some(vertices)) = (count("faces"), count("edges"), count("vertices"))
                && m.contains_key("shells")
            {
                *v = json!({ "brep": { "faces": faces, "edges": edges, "vertices": vertices } });
                return;
            }
            m.values_mut().for_each(sum_up_breps);
        }
        Value::Array(a) => a.iter_mut().for_each(sum_up_breps),
        _ => {}
    }
}

fn mods(m: &Modifiers) -> Value {
    json!({ "toggle": m.toggle, "copy": m.copy })
}

/// An input as the journal keeps it.
pub fn input(i: &ToolInput) -> Value {
    match i {
        ToolInput::Hover { px } => json!({ "hover": { "px": v2(*px) } }),
        ToolInput::Click { px, mods: m } => json!({ "click": { "px": v2(*px), "mods": mods(m) } }),
        ToolInput::DoubleClick { px, mods: m } => json!({ "double_click": { "px": v2(*px), "mods": mods(m) } }),
        ToolInput::DragStart { px, mods: m } => json!({ "drag_start": { "px": v2(*px), "mods": mods(m) } }),
        ToolInput::Drag { px } => json!({ "drag": { "px": v2(*px) } }),
        ToolInput::DragEnd { px, mods: m } => json!({ "drag_end": { "px": v2(*px), "mods": mods(m) } }),
        ToolInput::Key { key, mods: m } => json!({ "key": { "name": format!("{key:?}"), "mods": mods(m) } }),
        ToolInput::Vcb(text) => json!({ "vcb": text }),
        ToolInput::Value(v) => json!({ "value": debug(v) }),
    }
}

pub fn entry(e: &Entry) -> Value {
    let event = match &e.event {
        Event::Input { input: i, count } => {
            let mut v = input(i);
            if *count > 1 {
                v["count"] = json!(count);
            }
            v
        }
        Event::Tool(k) => json!({ "tool": k.id() }),
        Event::Option { key, value } => json!({ "option": { "key": key, "value": value } }),
        Event::Applied { op, revision } => json!({ "applied": { "op": op, "revision": revision } }),
        Event::Welded { part, body, reference, distance } => {
            json!({ "welded": { "part": part, "body": body, "reference": reference, "distance": distance } })
        }
        Event::Failed { op, error } => json!({ "failed": { "op": op, "error": error } }),
        Event::Job { what, ms, error } => json!({ "job": {
            "for": match what { JobKind::Operation => "operation", JobKind::Mesh => "mesh" },
            "ms": ms,
            "error": error,
        } }),
        Event::Undo => json!("undo"),
        Event::Redo => json!("redo"),
        Event::Forgotten { steps, bytes } => json!({ "forgotten": { "steps": steps, "bytes": bytes } }),
    };
    let mut v = json!({ "seq": e.seq, "ms": e.ms, "event": event, "origin": if e.origin.is_control() { "control" } else { "user" } });
    if let Some(c) = e.origin.client() {
        v["client"] = json!(c);
    }
    v
}

/// A kernel failure: summed up, or in full (its job, which `skraft_kernel::run` runs
/// again).
pub fn failure(f: &Failure, full: bool) -> Value {
    let mut v = json!({
        "seq": f.seq,
        "ms": f.ms,
        "op": f.op.as_ref().map(|o| o.label()),
        "error": f.error,
        "inputs": f.job.inputs.len(),
        "tasks": f.job.tasks.len(),
    });
    if full {
        v["operation"] = f.op.as_ref().map_or(Value::Null, |o| serde_json::to_value(o).unwrap_or(Value::Null));
        v["job"] = serde_json::to_value(&f.job).unwrap_or(Value::Null);
    }
    v
}
