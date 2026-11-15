//! The documents open, one per file tab (docs/ARCHITECTURE.md §6 octies): each with its own
//! editor (history, scene, camera, selection, tool), its file and what is typed for it. Every
//! document is kept alike, shown or not; `active` is the one shown (`None`: the start page).
//! The editors share one kernel service ([`SharedKernel`]), each through a handle of its own.

use solidsketch_i18n::t;
use solidsketch_model::Document;
use solidsketch_model::kernel::SharedKernel;
use solidsketch_tools::Editor;

/// A document's identity while the app runs: never given to another one, unlike its place
/// among the tabs.
pub type DocId = u64;

/// Most documents open at once.
pub const MAX_OPEN: usize = 32;

pub struct Doc {
    pub id: DocId,
    pub editor: Editor,
    /// Where it was opened from or saved to.
    pub path: Option<String>,
    /// The history revision when last saved or opened (modified if it differs).
    pub saved_at: u64,
    /// What is being typed into the measurements box.
    pub vcb: String,
}

impl Doc {
    pub fn modified(&self) -> bool {
        self.editor.history().revision() != self.saved_at
    }

    /// Its tab's name: its file's, or « Sans titre ».
    pub fn name(&self) -> String {
        self.path.as_deref().map(file_name).map(str::to_owned).unwrap_or_else(|| t!("window.untitled"))
    }

    /// Never changed nor saved, nothing computing: a file opened takes its place.
    pub fn blank(&self) -> bool {
        self.path.is_none() && self.editor.history().revision() == 0 && !self.editor.busy() && !self.editor.is_importing()
    }
}

/// The name of the file at `path`, without its folders.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The documents open, in tab order.
pub struct Docs {
    list: Vec<Doc>,
    active: Option<usize>,
    next_id: DocId,
    kernel: SharedKernel,
}

/// No room for another document ([`MAX_OPEN`]).
#[derive(Debug)]
pub struct Full;

impl Docs {
    pub fn new(kernel: SharedKernel) -> Self {
        Docs { list: Vec::new(), active: None, next_id: 1, kernel }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Doc> {
        self.list.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Doc> {
        self.list.iter_mut()
    }

    pub fn get(&self, i: usize) -> Option<&Doc> {
        self.list.get(i)
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&Doc> {
        self.list.get(self.active?)
    }

    pub fn active_mut(&mut self) -> Option<&mut Doc> {
        self.list.get_mut(self.active?)
    }

    pub fn index_of(&self, id: DocId) -> Option<usize> {
        self.list.iter().position(|d| d.id == id)
    }

    pub fn by_id_mut(&mut self, id: DocId) -> Option<&mut Doc> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    /// The tab of the file at `path`, if it is open.
    pub fn find_path(&self, path: &str) -> Option<usize> {
        self.list.iter().position(|d| d.path.as_deref() == Some(path))
    }

    pub fn any_modified(&self) -> bool {
        self.list.iter().any(Doc::modified)
    }

    /// Opens `document` (from `path`, if any) and shows it: a file in place of the active
    /// document if that one is [blank](Doc::blank), else (and a new document always) in a new
    /// tab after the others. `setup` readies its editor (the tools' settings…). Gives its id,
    /// and the id of the document it replaced.
    pub fn open(
        &mut self,
        document: Document,
        path: Option<String>,
        setup: impl FnOnce(&mut Editor),
    ) -> Result<(DocId, Option<DocId>), Full> {
        let replaced = self.active.filter(|&i| path.is_some() && self.list[i].blank());
        if replaced.is_none() && self.list.len() >= MAX_OPEN {
            return Err(Full);
        }
        let mut editor = Editor::new(document, Box::new(self.kernel.handle()));
        setup(&mut editor);
        let id = self.next_id;
        self.next_id += 1;
        let doc = Doc { id, editor, path, saved_at: 0, vcb: String::new() };
        Ok(match replaced {
            Some(i) => (id, Some(std::mem::replace(&mut self.list[i], doc).id)),
            None => {
                self.list.push(doc);
                self.active = Some(self.list.len() - 1);
                (id, None)
            }
        })
    }

    pub fn select(&mut self, i: usize) {
        if i < self.list.len() {
            self.active = Some(i);
        }
    }

    /// Shows the next tab (or the previous one), round the ends.
    pub fn cycle(&mut self, forward: bool) {
        let n = self.list.len();
        if let Some(i) = self.active
            && n > 1
        {
            self.active = Some(if forward { (i + 1) % n } else { (i + n - 1) % n });
        }
    }

    /// Closes tab `i`, without asking. The tab after it is shown if it was (the one before it
    /// if it was the last), none when no tab is left.
    pub fn close(&mut self, i: usize) -> Option<Doc> {
        if i >= self.list.len() {
            return None;
        }
        let doc = self.list.remove(i);
        let n = self.list.len();
        self.active = match self.active {
            _ if n == 0 => None,
            Some(a) if a > i => Some(a - 1),
            Some(a) if a == i => Some(i.min(n - 1)),
            a => a,
        };
        Some(doc)
    }

    /// Moves tab `from` to place `to`, the shown one staying shown.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let n = self.list.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let shown = self.active.map(|a| self.list[a].id);
        let doc = self.list.remove(from);
        self.list.insert(to, doc);
        self.active = shown.and_then(|id| self.index_of(id));
    }
}

#[cfg(test)]
mod tests {
    use solidsketch_model::kernel::Inline;
    use solidsketch_model::{Operation, Settings};

    use super::*;

    fn docs() -> Docs {
        Docs::new(SharedKernel::new(Box::new(Inline::default())))
    }

    fn open(d: &mut Docs, path: Option<&str>) -> (DocId, Option<DocId>) {
        d.open(Document::default(), path.map(str::to_owned), |_| {}).unwrap()
    }

    /// A change to the shown document (one with no kernel call: its grid switched).
    fn change(d: &mut Docs) {
        let e = &mut d.active_mut().unwrap().editor;
        let mut s = e.document().settings().clone();
        s.display.grid = !s.display.grid;
        e.apply(Operation::SetSettings(s)).unwrap();
    }

    #[test]
    fn a_file_takes_the_place_of_a_blank_document_only() {
        let mut d = docs();
        let (blank, _) = open(&mut d, None);
        let (a, replaced) = open(&mut d, Some("/a.solidsketch"));
        assert_eq!((replaced, d.len()), (Some(blank), 1), "the blank one gives its place");
        assert_ne!(a, blank);
        // `a` has a file: the next one gets a tab of its own, shown.
        let (b, replaced) = open(&mut d, None);
        assert_eq!((replaced, d.len(), d.active().unwrap().id), (None, 2, b));
        // A new document never replaces the blank one: `+` always adds a tab.
        let (_, replaced) = open(&mut d, None);
        assert_eq!((replaced, d.len()), (None, 3));
        d.close(2);
        // A modified document keeps its tab too.
        change(&mut d);
        assert!(!d.active().unwrap().blank());
        let (_, replaced) = open(&mut d, Some("/c.solidsketch"));
        assert_eq!((replaced, d.len()), (None, 3));
        assert_eq!(d.find_path("/a.solidsketch"), Some(0));
        assert!(d.any_modified());
    }

    #[test]
    fn each_document_keeps_its_own_history() {
        let mut d = docs();
        open(&mut d, None);
        change(&mut d);
        open(&mut d, None);
        assert!(!d.active().unwrap().modified(), "the second one is untouched");
        d.select(0);
        assert!(d.active().unwrap().modified());
        assert!(d.active().unwrap().editor.document().settings().display.grid != Settings::default().display.grid);
    }

    #[test]
    fn closing_shows_the_next_tab_then_the_start_page() {
        let mut d = docs();
        let ids: Vec<DocId> = (0..3).map(|k| open(&mut d, Some(&format!("/{k}"))).0).collect();
        d.select(1);
        d.close(1);
        assert_eq!(d.active().unwrap().id, ids[2], "the one after it");
        d.close(1);
        assert_eq!(d.active().unwrap().id, ids[0], "the last closed: the one before it");
        // Closing a tab before the shown one keeps that one shown.
        let (x, _) = open(&mut d, Some("/x"));
        d.close(0);
        assert_eq!(d.active().unwrap().id, x);
        d.close(0);
        assert!(d.active().is_none() && d.len() == 0, "the start page");
    }

    #[test]
    fn moving_and_cycling_tabs() {
        let mut d = docs();
        let ids: Vec<DocId> = (0..3).map(|k| open(&mut d, Some(&format!("/{k}"))).0).collect();
        d.select(0);
        d.move_tab(0, 2);
        assert_eq!(d.iter().map(|x| x.id).collect::<Vec<_>>(), [ids[1], ids[2], ids[0]]);
        assert_eq!(d.active_index(), Some(2), "the shown one moved with its tab");
        d.cycle(true);
        assert_eq!(d.active_index(), Some(0));
        d.cycle(false);
        assert_eq!(d.active_index(), Some(2));
    }

    #[test]
    fn no_more_than_max_open() {
        let mut d = docs();
        for k in 0..MAX_OPEN {
            open(&mut d, Some(&format!("/{k}")));
        }
        assert!(d.open(Document::default(), Some("/more".into()), |_| {}).is_err());
    }
}
