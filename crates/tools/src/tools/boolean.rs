//! Union / Subtract / Intersect: click the target part, then the tool part, in any contexts
//! (ARCHITECTURE.md §4: the target is changed in its own axes, the tool is consumed). The
//! scope option says whether every occurrence of the target's part changes, or only the one
//! clicked (§2.5).

use solidsketch_model::{Boolean, EntityPath, Operation, Scope};

use super::{Command, Cx, OptionChoice, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption};

/// The scope option's key and values.
pub const SCOPE: &str = "scope";
const ALL: &str = "all";
const THIS: &str = "this";

fn scope(cx: &Cx<'_>) -> Scope {
    if cx.choice(SCOPE) == Some(THIS) { Scope::This } else { Scope::All }
}

pub struct BooleanTool {
    op: Boolean,
    target: Option<EntityPath>,
    hover: Option<EntityPath>,
}

impl BooleanTool {
    pub fn new(op: Boolean) -> Self {
        BooleanTool { op, target: None, hover: None }
    }

    fn noun(&self) -> &'static str {
        match self.op {
            Boolean::Union => "unir",
            Boolean::Difference => "soustraire",
            Boolean::Intersection => "intersecter",
        }
    }
}

impl Tool for BooleanTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Boolean(self.op)
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => self.hover = cx.pick(*px).map(|h| h.path),
            ToolInput::Click { px, .. } => {
                let Some(path) = cx.pick(*px).map(|h| h.path) else { return vec![] };
                match self.target.take() {
                    None => self.target = Some(path),
                    Some(target) if target == path => {}
                    Some(target) => {
                        let op = Operation::Boolean { target, tool: path, op: self.op, scope: scope(cx) };
                        return vec![Command::Apply(op)];
                    }
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        // What a click would change: the target picked, else the part under the cursor.
        let aimed = self.target.clone().or_else(|| self.hover.clone());
        let propagate = aimed.filter(|_| scope(cx) == Scope::All);
        Preview { hover_solid: self.hover.clone(), picked_solid: self.target.clone(), propagate, ..Default::default() }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let aimed = self.target.as_ref().or(self.hover.as_ref());
        let all = match aimed.map(|p| cx.copies_shown(p)) {
            Some(n) if n > 1 => format!("Tous les exemplaires ×{n}"),
            Some(_) => "Tous les exemplaires (un seul)".to_owned(),
            None => "Tous les exemplaires".to_owned(),
        };
        let value = if scope(cx) == Scope::This { THIS } else { ALL };
        vec![ToolOption {
            key: SCOPE,
            label: "Portée".into(),
            tip: "Ce que l'opération modifie (Tab pour changer)".into(),
            value: OptionValue::Choice(value.into()),
            choices: vec![
                OptionChoice { value: ALL, label: all, tip: "La pièce elle-même : tous ses exemplaires changent".into() },
                OptionChoice {
                    value: THIS,
                    label: "Celui-ci".into(),
                    tip: "Seulement l'exemplaire cliqué : il devient une pièce à part (et son assemblage, s'il est copié)".into(),
                },
            ],
            saved: true,
        }]
    }

    fn status(&self) -> String {
        match self.target {
            None => format!("Cliquer la pièce à modifier ({}) · Tab : changer la portée", self.noun()),
            Some(_) => format!("Cliquer la pièce outil (elle sera consommée) pour {} · Tab : changer la portée", self.noun()),
        }
    }

    fn busy(&self) -> bool {
        self.target.is_some()
    }

    fn reset(&mut self) {
        self.target = None;
    }
}
