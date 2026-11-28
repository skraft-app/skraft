//! The model as a STEP file's products (docs/io/step-export.md,
//! docs/decisions/0059-exporters-step-exact-skp-meshed.md): one per component, shared by its
//! occurrences, a part's final shape its one body; the kernel writes them.

use std::collections::HashMap;
use std::sync::Arc;

use glam::DAffine3;
use skraft_kernel::{Job, MIRROR, Solid, StepExport, StepExportBody, StepExportProduct, Task};

use crate::document::{ComponentKind, Document, EntityPath, Id};

/// What [`Document::step_export`] gathers as it goes.
struct Gather<'a> {
    doc: &'a Document,
    products: Vec<StepExportProduct>,
    /// A product per component and hand (mirrored or not).
    index: HashMap<(Id, bool), usize>,
    inputs: Vec<Arc<Solid>>,
    /// The job's input of each solid, by address: a shape shared by parts crosses once.
    input_of: HashMap<*const Solid, u32>,
    /// Bodies written, to tell whether the file holds anything.
    bodies: usize,
}

impl Gather<'_> {
    /// The product of `component`, mirrored across its YZ plane when `mirrored`; made with its
    /// children's the first time.
    fn product(&mut self, component: Id, mirrored: bool) -> Option<usize> {
        if let Some(&i) = self.index.get(&(component, mirrored)) {
            return Some(i);
        }
        let doc = self.doc;
        let c = doc.component(component).ok()?;
        let mut product = StepExportProduct { name: c.name.clone(), ..Default::default() };
        match &c.kind {
            ComponentKind::Part(part) => {
                let shape = part.shape();
                if !shape.record().faces.is_empty() {
                    let solid = self.input(shape);
                    let color = doc.seen_srgb(part.shown());
                    let faces = part
                        .faces_shown()
                        .into_iter()
                        .filter_map(|(f, seen)| doc.seen_srgb(seen).filter(|c| Some(*c) != color).map(|c| (f, c)))
                        .collect();
                    product.bodies.push(StepExportBody { name: c.name.clone(), solid, mirrored, color, faces });
                    self.bodies += 1;
                }
            }
            ComponentKind::Assembly(a) => {
                let hand = if mirrored { MIRROR } else { DAffine3::IDENTITY };
                for o in a.occurrences().iter().filter(|o| !o.hidden) {
                    // STEP places rigidly: a placement that mirrors places the child's twin,
                    // the mirror moved into it.
                    let placed = hand * o.transform;
                    let twin = placed.matrix3.determinant() < 0.0;
                    let placement = if twin { placed * MIRROR } else { placed };
                    if let Some(child) = self.product(o.component, twin) {
                        product.children.push((child, placement, o.name.clone()));
                    }
                }
            }
        }
        let i = self.products.len();
        self.products.push(product);
        self.index.insert((component, mirrored), i);
        Some(i)
    }

    fn input(&mut self, solid: &Arc<Solid>) -> u32 {
        let next = self.inputs.len() as u32;
        let i = *self.input_of.entry(Arc::as_ptr(solid)).or_insert(next);
        if i == next {
            self.inputs.push(solid.clone());
        }
        i
    }
}

/// A STEP export ready for the kernel ([`Document::step_export`]).
pub struct StepExportPlan {
    /// The job writing it.
    pub job: Job,
    /// Its top product's name, the file's offered: the component exported alone, else the
    /// common parent's, else the model's.
    pub name: String,
    /// How many parts it places (each occurrence counted).
    pub parts: usize,
}

impl Document {
    /// The STEP export of `paths` (occurrences, docs/io/step-export.md), the whole model when
    /// none: their visible occurrences, each component once. One component goes out in its own
    /// axes, as the top product; several are placed in their common parent's axes, under a top
    /// product named after it (`name`, the model's, when it is the model). None when it holds no
    /// solid.
    pub fn step_export(&self, paths: &[EntityPath], name: &str) -> Option<StepExportPlan> {
        let mut g =
            Gather { doc: self, products: Vec::new(), index: HashMap::new(), inputs: Vec::new(), input_of: HashMap::new(), bodies: 0 };
        let paths = outermost(paths);
        let (root, name) = match paths.as_slice() {
            [] => (g.product(self.root, false)?, name.to_owned()),
            [one] => {
                let c = self.resolve(one).ok()?.component;
                (g.product(c, false)?, self.component(c).ok()?.name.clone())
            }
            many => {
                // The deepest assembly holding them all: their occurrence chains' common start.
                let first = &many[0].occurrences;
                let common = (0..first.len()).take_while(|&k| many.iter().all(|p| p.occurrences.get(k) == Some(&first[k]))).count();
                let prefix = &first[..common];
                let parent = self.context_of(prefix).ok()?;
                let frame = self.transform_of(prefix).ok()?.inverse();
                let mut top = StepExportProduct { name: String::new(), ..Default::default() };
                for p in many {
                    let o = self.resolve(p).ok()?;
                    let mut chain = p.occurrences.clone();
                    chain.push(p.entity);
                    let placed = frame * self.transform_of(&chain).ok()?;
                    let twin = placed.matrix3.determinant() < 0.0;
                    let placement = if twin { placed * MIRROR } else { placed };
                    if let Some(child) = g.product(o.component, twin) {
                        top.children.push((child, placement, o.name.clone()));
                    }
                }
                let parent_name = if parent == self.root { name.to_owned() } else { self.component(parent).ok()?.name.clone() };
                g.products.push(top);
                (g.products.len() - 1, parent_name)
            }
        };
        if g.bodies == 0 {
            return None;
        }
        g.products[root].name = name.clone();
        let parts = placed_parts(&g.products, root, &mut HashMap::new());
        let export = StepExport {
            products: g.products,
            root,
            file_name: format!("{name}.step"),
            program: format!("Skraft {}", env!("CARGO_PKG_VERSION")),
        };
        Some(StepExportPlan { job: Job { inputs: g.inputs, tasks: vec![Task::ExportStep { export: Box::new(export) }] }, name, parts })
    }
}

/// `paths` without those under another of them (an assembly and a part of it: the assembly).
fn outermost(paths: &[EntityPath]) -> Vec<EntityPath> {
    let under = |p: &EntityPath, a: &EntityPath| {
        p.occurrences.len() > a.occurrences.len()
            && p.occurrences[..a.occurrences.len()] == a.occurrences[..]
            && p.occurrences[a.occurrences.len()] == a.entity
    };
    let mut out: Vec<EntityPath> = Vec::new();
    for p in paths {
        let p = EntityPath::new(p.occurrences.clone(), p.entity);
        if !paths.iter().any(|a| under(&p, a)) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// How many parts the product `i` places, its own body counted.
fn placed_parts(products: &[StepExportProduct], i: usize, memo: &mut HashMap<usize, usize>) -> usize {
    if let Some(&n) = memo.get(&i) {
        return n;
    }
    let p = &products[i];
    let n = usize::from(!p.bodies.is_empty()) + p.children.iter().map(|(c, _, _)| placed_parts(products, *c, memo)).sum::<usize>();
    memo.insert(i, n);
    n
}

#[cfg(test)]
mod tests {
    use glam::{DAffine3, DVec3};
    use skraft_kernel::{Output, StepImport, Task};

    use crate::{Document, Family, History, MaterialKind, Operation, Primitive};

    fn written(doc: &Document) -> String {
        let plan = doc.step_export(&[], "Meuble").expect("something to write");
        match skraft_kernel::run(&plan.job).unwrap().pop() {
            Some(Output::StepFile(text)) => text,
            o => panic!("{o:?}"),
        }
    }

    fn read(text: String) -> StepImport {
        match skraft_kernel::run(&skraft_kernel::Job { inputs: vec![], tasks: vec![Task::ImportStep { bytes: text.into_bytes() }] })
            .unwrap()
            .pop()
        {
            Some(Output::Step(s)) => *s,
            o => panic!("{o:?}"),
        }
    }

    fn add_box(h: &mut History) -> crate::Id {
        let root = h.document().root();
        let primitive = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 20.0, 10.0) };
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0]
    }

    /// An empty model writes nothing.
    #[test]
    fn an_empty_model_has_nothing_to_write() {
        assert!(Document::new().step_export(&[], "Vide").is_none());
    }

    /// Two copies of a part and a mirrored one: one product per hand, placed three times, its
    /// colour the part's material under its finish.
    #[test]
    fn copies_share_their_product_a_mirror_makes_a_twin() {
        let mut h = History::new(Document::new());
        let first = add_box(&mut h);
        let root = h.document().root();
        let part = h.document().root_assembly().occurrence(first).unwrap().component;
        let mirrored = DAffine3::from_translation(DVec3::new(-300.0, 0.0, 0.0)) * skraft_kernel::MIRROR;
        for transform in [DAffine3::from_translation(DVec3::new(0.0, 100.0, 0.0)), mirrored] {
            h.apply(Operation::Duplicate { context: root, entities: vec![first], transform }).unwrap();
        }
        let red = h
            .apply(Operation::AddMaterial { name: "Rouge".into(), color: [255, 0, 0], kind: Some(MaterialKind::Generic) })
            .unwrap()
            .material
            .unwrap();
        h.apply(Operation::PaintPart { part, layer: Family::Material, material: Some(red) }).unwrap();

        let doc = h.document();
        let text = written(doc);
        let step = read(text);
        assert!(step.excluded.is_empty(), "{:?}", step.excluded);
        // The part's two hands, and the top product.
        assert_eq!(
            step.products.iter().filter(|p| !p.bodies.is_empty()).count(),
            2,
            "{:?}",
            step.products.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        let top = &step.products[step.roots[0].product];
        assert_eq!(top.name, "Meuble");
        assert_eq!(top.children.len(), 3);
        // The mirrored copy's box lies from −400 to −300 along x.
        let twin = top.children.iter().find(|c| c.transform.translation.x < -100.0).unwrap();
        let (lo, hi) = step.products[twin.product].bodies[0].solid.vertex_bounds().unwrap();
        let (a, b) = (twin.transform.transform_point3(lo).x, twin.transform.transform_point3(hi).x);
        assert!((a.min(b) + 400.0).abs() < 1e-6 && (a.max(b) + 300.0).abs() < 1e-6, "{a} {b}");
        assert!(step.products.iter().flat_map(|p| &p.bodies).all(|b| b.color == Some([255, 0, 0])));
    }

    /// A hidden occurrence stays out of the file.
    #[test]
    fn hidden_occurrences_stay_out() {
        let mut h = History::new(Document::new());
        let first = add_box(&mut h);
        let root = h.document().root();
        h.apply(Operation::Duplicate { context: root, entities: vec![first], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
            .unwrap();
        h.apply(Operation::SetHidden { context: root, entities: vec![first], hidden: true }).unwrap();
        let step = read(written(h.document()));
        assert_eq!(step.products[step.roots[0].product].children.len(), 1);
    }

    /// The world box of every body read back, along x.
    fn spans(step: &StepImport) -> Vec<(f64, f64)> {
        fn walk(step: &StepImport, p: usize, t: DAffine3, out: &mut Vec<(f64, f64)>) {
            for b in &step.products[p].bodies {
                let (lo, hi) = b.solid.vertex_bounds().unwrap();
                let (a, c) = (t.transform_point3(lo).x, t.transform_point3(hi).x);
                out.push((a.min(c), a.max(c)));
            }
            for c in &step.products[p].children {
                walk(step, c.product, t * c.transform, out);
            }
        }
        let mut out = Vec::new();
        for r in &step.roots {
            walk(step, r.product, r.transform, &mut out);
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    fn export(doc: &Document, paths: &[crate::EntityPath]) -> (crate::StepExportPlan, StepImport) {
        let plan = doc.step_export(paths, "Meuble").unwrap();
        let text = match skraft_kernel::run(&plan.job).unwrap().pop() {
            Some(Output::StepFile(t)) => t,
            o => panic!("{o:?}"),
        };
        (plan, read(text))
    }

    /// One part selected goes out alone, in its own axes, named after it; two parts of an
    /// assembly moved away, in the assembly's axes, the top product named after it.
    #[test]
    fn a_selection_goes_out_from_its_origin() {
        let mut h = History::new(Document::new());
        let a = add_box(&mut h);
        let root = h.document().root();
        let b = h
            .apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::X * 200.0) })
            .unwrap()
            .created[0];
        let shelf =
            h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: Some("Étagère".into()) }).unwrap().created[0];
        h.apply(Operation::Transform { context: root, entities: vec![shelf], transform: DAffine3::from_translation(DVec3::X * 1000.0) })
            .unwrap();
        let doc = h.document();
        let asm = doc.root_assembly().occurrence(shelf).unwrap().component;
        let inside: Vec<_> = doc.assembly(asm).unwrap().occurrences().iter().map(|o| crate::EntityPath::new(vec![shelf], o.id)).collect();
        assert_eq!(inside.len(), 2);

        // The second board alone: at its own origin, not at x = 1200.
        let (plan, step) = export(doc, &inside[1..]);
        assert_eq!(plan.parts, 1);
        let name = &doc.component(doc.resolve(&inside[1]).unwrap().component).unwrap().name;
        assert_eq!(&plan.name, name);
        assert_eq!(step.products[step.roots[0].product].name, *name);
        let s = spans(&step);
        assert!(s.len() == 1 && s[0].0.abs() < 1e-6 && (s[0].1 - 100.0).abs() < 1e-6, "{s:?}");

        // Both boards: in the shelf's axes (0 and 200), not the world's (1000 and 1200).
        let (plan, step) = export(doc, &inside);
        assert_eq!((plan.parts, plan.name.as_str()), (2, "Étagère"));
        let s = spans(&step);
        assert!(s.len() == 2 && s[0].0.abs() < 1e-6 && (s[1].0 - 200.0).abs() < 1e-6, "{s:?}");

        // The shelf and one of its boards: the shelf alone, the board in it once.
        let mut both = vec![crate::EntityPath::root(shelf)];
        both.push(inside[0].clone());
        let (plan, _) = export(doc, &both);
        assert_eq!((plan.parts, plan.name.as_str()), (2, "Étagère"));
    }
}
