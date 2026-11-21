//! The app's clipboard (docs/tools/copy-paste.md): one for every document, holding
//! what was copied in memory; the system clipboard only gets a token naming it. Copying
//! anything else anywhere replaces the token: ⌘V then no longer pastes the app's content.

use std::sync::Arc;

use glam::DVec2;
use skraft_model::Document;
use skraft_tools::Clip;

use crate::docs::DocId;

/// What was copied: from the model, or from a sheet (docs/ui/drawing.md#copy-and-paste).
pub enum Copied {
    Model(Clip),
    /// A fragment of the drawing, and the box of what was picked, in the axes it was in.
    Sheet {
        fragment: Arc<Document>,
        bounds: (DVec2, DVec2),
    },
}

impl From<Clip> for Copied {
    fn from(clip: Clip) -> Self {
        Copied::Model(clip)
    }
}

/// What was copied, and where from.
pub struct Clipboard {
    pub content: Copied,
    /// The document it was copied from…
    pub doc: DocId,
    /// …at this revision of its history: pasting there again reuses its parts while that
    /// state is still in the history ([`Clipboard::reuse`]).
    pub revision: u64,
    /// The text put on the system clipboard for it.
    pub token: String,
}

impl Clipboard {
    /// Takes `content`, copied from `doc` at `revision`; `serial` counts the copies.
    pub fn new(content: impl Into<Copied>, doc: DocId, revision: u64, serial: u64) -> Self {
        Clipboard { content: content.into(), doc, revision, token: format!("{TOKEN_PREFIX}{:016x}:{serial}", run_nonce()) }
    }

    /// Is `text`, the system clipboard's, this clipboard's token?
    pub fn is_on_system_clipboard(&self, text: &str) -> bool {
        text.trim() == self.token
    }

    /// Does pasting into `doc`, whose history holds `holds`, reuse the copied parts (the same
    /// document, its ids still naming the same things)?
    pub fn reuse(&self, doc: DocId, holds: impl FnOnce(u64) -> bool) -> bool {
        self.doc == doc && holds(self.revision)
    }
}

/// What every token starts with.
const TOKEN_PREFIX: &str = "skraft-clipboard:";

/// A number drawn once per run of the app: two instances give different tokens.
fn run_nonce() -> u64 {
    static NONCE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *NONCE.get_or_init(|| {
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        // Mixed with an address, in case two instances start within the clock's resolution.
        now ^ (&NONCE as *const _ as u64).rotate_left(32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skraft_tools::Content;

    #[test]
    fn the_token_names_one_copy_and_reuse_needs_the_same_state() {
        let clip = || Clip::new(Content::Entities(Arc::new(Document::new())), Vec::new());
        let a = Clipboard::new(clip(), 3, 7, 1);
        let b = Clipboard::new(clip(), 3, 7, 2);
        assert_ne!(a.token, b.token);
        assert!(a.is_on_system_clipboard(&format!("{}\n", a.token)));
        assert!(!a.is_on_system_clipboard(&b.token));
        assert!(!a.is_on_system_clipboard("anything"));
        assert!(a.reuse(3, |r| r == 7));
        assert!(!a.reuse(3, |_| false), "the state copied is gone");
        assert!(!a.reuse(4, |_| true), "another document");
    }
}
