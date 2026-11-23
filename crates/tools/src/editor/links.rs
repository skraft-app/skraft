//! What the editor tells and offers about linked bodies
//! (docs/decisions/0055-linked-bodies-through-imprints.md): after an operation, the sources left
//! off their seats and the machinings that lost their link, with what can be done about them.

use skraft_i18n::{t, tp};
use skraft_model::{Id, ImprintPlan, LinkSource, LinkStatus, Operation, Outcome};

use super::{Editor, Notice, NoticeAction, NoticeKind};

/// A button of the toast about linked bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkAct {
    /// « Replacer »: the other sources back on their seats.
    Replace,
    /// « Séparer »: the parts the moved sources enter made parts of their own.
    Separate,
}

/// What the toast's buttons act on, for the state it was shown in.
#[derive(Clone, Debug)]
pub(super) struct LinkOffer {
    revision: u64,
    replace: Vec<LinkSource>,
    separate: Vec<LinkSource>,
}

impl Editor {
    /// What the links need the user to see, as of the current document.
    pub fn link_status(&self) -> &LinkStatus {
        &self.link_status
    }

    /// After an operation (`before`, the status it was applied on): the sources it left off,
    /// but those it moved itself, offered to be put back; the sources that moved offered to take
    /// their receivers apart; the machinings that lost their link told.
    pub(super) fn offer_links(&mut self, before: &LinkStatus, outcome: &Outcome) {
        self.link_offer = None;
        let followed = &outcome.links;
        if outcome.imprinted.1 > 0 {
            self.notify(NoticeKind::Success, tp!("notice.imprinted", outcome.imprinted.1), None);
        }
        let others: Vec<LinkSource> =
            self.link_status.off.iter().filter(|s| !before.off.contains(s) && !followed.sources.contains(s)).copied().collect();
        let conflict = !followed.conflicts.is_empty();
        if conflict || !others.is_empty() {
            let separable = conflict || !followed.moved.is_empty();
            let (title, detail) = if conflict {
                (t!("notice.link_conflict"), Some(t!("notice.link_conflict_detail")))
            } else {
                (tp!("notice.links_off", others.len()), None)
            };
            let (action, also) = match (others.is_empty(), separable) {
                (false, true) => (Some(NoticeAction::Link(LinkAct::Replace)), Some(NoticeAction::Link(LinkAct::Separate))),
                (false, false) => (Some(NoticeAction::Link(LinkAct::Replace)), None),
                (true, _) => (Some(NoticeAction::Link(LinkAct::Separate)), None),
            };
            self.link_offer = Some(LinkOffer { revision: self.history.revision(), replace: others, separate: followed.sources.clone() });
            self.push_notice(Notice { kind: NoticeKind::Warning, title, detail, action, also });
        }
        let lost = self.link_status.broken.difference(&before.broken).count();
        if lost > 0 {
            // Goes by itself: undone, it would stay wrong. The navigator keeps the ⚠.
            self.notify(NoticeKind::Info, tp!("notice.links_lost", lost), None);
        }
    }

    /// The assembly edited and the occurrences selected in it, for « Imprimer »: not with a
    /// part open.
    fn imprint_sources(&self) -> Option<(Id, Vec<Id>)> {
        if self.open_part.is_some() {
            return None;
        }
        let context = self.context_definition();
        let a = self.document().assembly(context).ok()?;
        let sources: Vec<Id> = self.selection().iter().copied().filter(|id| a.occurrence(*id).is_some()).collect();
        (!sources.is_empty()).then_some((context, sources))
    }

    /// Can « Imprimer » be asked: occurrences selected, nothing computing?
    pub fn can_imprint(&self) -> bool {
        !self.busy() && self.imprint_sources().is_some()
    }

    /// What « Imprimer » would do with the selection.
    pub fn imprint_plan(&self) -> Option<ImprintPlan> {
        let (context, sources) = self.imprint_sources()?;
        self.document().imprint_plan(context, &sources).ok()
    }

    /// « Imprimer » on the selection, with this clearance around the tool and past its end.
    pub fn imprint(&mut self, around: f64, deep: f64) {
        let Some((context, sources)) = self.imprint_sources() else { return };
        let _ = self.apply(Operation::Imprint { context, sources, around, deep });
    }

    /// Can the toast's button `act` still be run: nothing changed since it was shown?
    pub fn link_act_live(&self, _act: LinkAct) -> bool {
        self.link_offer.as_ref().is_some_and(|o| o.revision == self.history.revision())
    }

    /// Runs the toast's button `act`. A failure is told as any operation's.
    pub fn run_link_act(&mut self, act: LinkAct) {
        let Some(offer) = self.link_offer.take().filter(|o| o.revision == self.history.revision()) else { return };
        let op = match act {
            LinkAct::Replace => Operation::ReseatSources { sources: offer.replace },
            LinkAct::Separate => Operation::SeparateReceivers { sources: offer.separate },
        };
        let _ = self.apply(op);
    }
}
