//! Navigation styles (docs/ARCHITECTURE.md §6 quindecies): which mouse buttons, modifiers and
//! trackpad gestures move the camera, after the habits of another program. The keyboard and the
//! tools' modifiers are never part of a style; the left button alone belongs to the tools.

use serde::{Deserialize, Serialize};

/// The program a style takes its habits from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    SketchUp,
    Fusion,
    SolidWorks,
    Onshape,
    Rhino,
    Blender,
}

impl Preset {
    pub const ALL: [Preset; 6] = [Preset::SketchUp, Preset::Fusion, Preset::SolidWorks, Preset::Onshape, Preset::Rhino, Preset::Blender];

    /// The program's name (not translated).
    pub fn name(self) -> &'static str {
        match self {
            Preset::SketchUp => "SketchUp",
            Preset::Fusion => "Fusion 360",
            Preset::SolidWorks => "SolidWorks",
            Preset::Onshape => "Onshape",
            Preset::Rhino => "Rhino",
            Preset::Blender => "Blender",
        }
    }

    /// The style as the program has it (§6 quindecies, *The presets*).
    pub fn style(self) -> NavStyle {
        use Button::*;
        use NavAction::*;
        let none = Mods::NONE;
        let shift = Mods { shift: true, ..Mods::NONE };
        let ctrl = Mods { ctrl: true, ..Mods::NONE };
        let drag = |b, mods, action| Binding { trigger: Trigger::Drag(b), mods, action };
        let double = |b, action| Binding { trigger: Trigger::DoubleClick(b), mods: none, action };
        let (mut bindings, pivot, wheel_reversed, trackpad) = match self {
            Preset::SketchUp => (
                vec![drag(Middle, none, Orbit), drag(Middle, shift, Pan), double(Middle, Recentre)],
                Pivot::Aimed,
                false,
                TrackpadBindings::default(),
            ),
            Preset::Fusion => (
                vec![drag(Middle, shift, Orbit), drag(Middle, none, Pan), double(Middle, ZoomExtents)],
                Pivot::Aimed,
                true,
                TrackpadBindings { scroll: Pan, shift_scroll: Orbit },
            ),
            Preset::SolidWorks => (
                vec![drag(Middle, none, Orbit), drag(Middle, ctrl, Pan), drag(Middle, shift, ZoomDrag), double(Middle, ZoomExtents)],
                Pivot::SelectionOrModel,
                true,
                TrackpadBindings::default(),
            ),
            Preset::Onshape => (
                vec![drag(Right, none, Orbit), drag(Middle, none, Pan), drag(Right, ctrl, Pan), double(Middle, ZoomExtents)],
                Pivot::Aimed,
                false,
                TrackpadBindings::default(),
            ),
            Preset::Rhino => (
                vec![drag(Right, none, Orbit), drag(Right, shift, Pan), drag(Right, ctrl, ZoomDrag)],
                Pivot::ScreenCentre,
                false,
                TrackpadBindings::default(),
            ),
            Preset::Blender => (
                vec![drag(Middle, none, Orbit), drag(Middle, shift, Pan), drag(Middle, ctrl, ZoomDrag)],
                Pivot::ScreenCentre,
                false,
                TrackpadBindings::default(),
            ),
        };
        // Every preset: the side buttons go back and forth in the views.
        bindings.push(Binding { trigger: Trigger::Click(Back), mods: none, action: PreviousView });
        bindings.push(Binding { trigger: Trigger::Click(Forward), mods: none, action: NextView });
        NavStyle { preset: self, custom: false, bindings, trackpad, pivot, wheel_reversed }
    }
}

/// A mouse button a style may use: never the left one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Middle,
    Right,
    /// Button 4.
    Back,
    /// Button 5.
    Forward,
}

impl Button {
    pub const ALL: [Button; 4] = [Button::Middle, Button::Right, Button::Back, Button::Forward];
}

/// What sets a gesture off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Drag(Button),
    Click(Button),
    DoubleClick(Button),
}

impl Trigger {
    pub fn button(self) -> Button {
        match self {
            Trigger::Drag(b) | Trigger::Click(b) | Trigger::DoubleClick(b) => b,
        }
    }

    pub fn is_drag(self) -> bool {
        matches!(self, Trigger::Drag(_))
    }

    /// The same kind of trigger on button `b`.
    pub fn with_button(self, b: Button) -> Trigger {
        match self {
            Trigger::Drag(_) => Trigger::Drag(b),
            Trigger::Click(_) => Trigger::Click(b),
            Trigger::DoubleClick(_) => Trigger::DoubleClick(b),
        }
    }
}

/// The modifier keys held, as physical keys: a preset's Ctrl is Ctrl on every OS. `cmd` is
/// macOS's Command key (none elsewhere).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, cmd: false };
}

/// What a gesture does to the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavAction {
    Orbit,
    Pan,
    /// Vertical motion zooms (up: closer).
    ZoomDrag,
    /// The rectangle dragged fills the view.
    ZoomWindow,
    /// The point under the cursor to the centre of the view.
    Recentre,
    ZoomExtents,
    PreviousView,
    NextView,
}

impl NavAction {
    /// The actions a drag does.
    pub const DRAG: [NavAction; 4] = [NavAction::Orbit, NavAction::Pan, NavAction::ZoomDrag, NavAction::ZoomWindow];
    /// The actions a click or a double-click does.
    pub const CLICK: [NavAction; 4] = [NavAction::Recentre, NavAction::ZoomExtents, NavAction::PreviousView, NavAction::NextView];

    /// Is it done by dragging (else by a click)?
    pub fn is_drag(self) -> bool {
        Self::DRAG.contains(&self)
    }
}

/// A gesture of a style: its trigger with exactly these modifiers does `action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Binding {
    pub trigger: Trigger,
    #[serde(default)]
    pub mods: Mods,
    pub action: NavAction,
}

/// What the orbit turns around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pivot {
    /// The point under the cursor when the gesture began (SketchUp).
    #[default]
    Aimed,
    /// The centre of the selection, else of the model (SolidWorks).
    SelectionOrModel,
    /// What the camera looks at, in the middle of the view (Blender, Rhino).
    ScreenCentre,
}

impl Pivot {
    pub const ALL: [Pivot; 3] = [Pivot::Aimed, Pivot::SelectionOrModel, Pivot::ScreenCentre];
}

/// The trackpad's two-finger scroll, without and with Shift: an orbit or a pan. The pinch
/// always zooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackpadBindings {
    pub scroll: NavAction,
    pub shift_scroll: NavAction,
}

impl Default for TrackpadBindings {
    fn default() -> Self {
        TrackpadBindings { scroll: NavAction::Orbit, shift_scroll: NavAction::Pan }
    }
}

/// Why a binding cannot be applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// Another binding has the same trigger and modifiers (the first one is applied).
    Duplicate,
    /// A right click is kept for a context menu.
    RightClick,
    /// A click on a button that also has a double-click: the first click would fire first.
    ClickAndDoubleClick,
    /// A drag action on a click, or the other way.
    WrongKind,
}

/// How the mouse and the trackpad move the camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored", into = "Stored")]
pub struct NavStyle {
    /// The preset it was taken from.
    pub preset: Preset,
    /// Edited since: stored in full (else only its preset, so a preset corrected later
    /// reaches it).
    pub custom: bool,
    pub bindings: Vec<Binding>,
    pub trackpad: TrackpadBindings,
    pub pivot: Pivot,
    /// The wheel forward zooms out (Fusion, SolidWorks), and so does a zoom drag upwards.
    pub wheel_reversed: bool,
}

impl Default for NavStyle {
    fn default() -> Self {
        Preset::SketchUp.style()
    }
}

impl NavStyle {
    /// What `trigger` with `mods` held does: the first valid binding matching exactly.
    pub fn resolve(&self, trigger: Trigger, mods: Mods) -> Option<NavAction> {
        self.bindings
            .iter()
            .enumerate()
            .find(|(i, b)| b.trigger == trigger && b.mods == mods && self.error(*i).is_none())
            .map(|(_, b)| b.action)
    }

    /// Does any valid binding drag with `b`? (Else a press of it is left to egui's click.)
    pub fn drags_with(&self, b: Button) -> bool {
        self.bindings.iter().enumerate().any(|(i, x)| x.trigger == Trigger::Drag(b) && self.error(i).is_none())
    }

    /// Why binding `i` is not applied, if it is not.
    pub fn error(&self, i: usize) -> Option<BindingError> {
        let b = self.bindings.get(i)?;
        if b.trigger.is_drag() != b.action.is_drag() {
            return Some(BindingError::WrongKind);
        }
        if let Trigger::Click(button) = b.trigger {
            if button == Button::Right {
                return Some(BindingError::RightClick);
            }
            if self.bindings.iter().any(|x| x.trigger == Trigger::DoubleClick(button) && x.mods == b.mods) {
                return Some(BindingError::ClickAndDoubleClick);
            }
        }
        if self.bindings[..i].iter().any(|x| x.trigger == b.trigger && x.mods == b.mods) {
            return Some(BindingError::Duplicate);
        }
        None
    }

    /// Marks it edited: stored in full from now on.
    pub fn edited(&mut self) {
        self.custom = true;
    }
}

/// A style as stored: its preset alone while not edited.
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    preset: Preset,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    custom: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bindings: Option<Vec<Binding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trackpad: Option<TrackpadBindings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pivot: Option<Pivot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wheel_reversed: Option<bool>,
}

impl From<Stored> for NavStyle {
    fn from(s: Stored) -> Self {
        let mut style = s.preset.style();
        if s.custom {
            style.custom = true;
            style.bindings = s.bindings.unwrap_or(style.bindings);
            style.trackpad = s.trackpad.unwrap_or(style.trackpad);
            style.pivot = s.pivot.unwrap_or(style.pivot);
            style.wheel_reversed = s.wheel_reversed.unwrap_or(style.wheel_reversed);
        }
        style
    }
}

impl From<NavStyle> for Stored {
    fn from(s: NavStyle) -> Self {
        if !s.custom {
            return Stored { preset: s.preset, custom: false, bindings: None, trackpad: None, pivot: None, wheel_reversed: None };
        }
        Stored {
            preset: s.preset,
            custom: true,
            bindings: Some(s.bindings),
            trackpad: Some(s.trackpad),
            pivot: Some(s.pivot),
            wheel_reversed: Some(s.wheel_reversed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false, cmd: false };
    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false, cmd: false };

    #[test]
    fn every_preset_is_valid_and_resolves_its_table() {
        use Button::*;
        use NavAction::*;
        for p in Preset::ALL {
            let s = p.style();
            for i in 0..s.bindings.len() {
                assert_eq!(s.error(i), None, "{p:?} binding {i}");
            }
            assert_eq!(s.resolve(Trigger::Click(Back), Mods::NONE), Some(PreviousView));
            assert_eq!(s.resolve(Trigger::Click(Forward), Mods::NONE), Some(NextView));
        }
        let s = Preset::SketchUp.style();
        assert_eq!(s.resolve(Trigger::Drag(Middle), Mods::NONE), Some(Orbit));
        assert_eq!(s.resolve(Trigger::Drag(Middle), SHIFT), Some(Pan));
        assert_eq!(s.resolve(Trigger::DoubleClick(Middle), Mods::NONE), Some(Recentre));
        assert_eq!(s.resolve(Trigger::Drag(Right), Mods::NONE), None);
        assert!(!s.drags_with(Right));
        let s = Preset::Fusion.style();
        assert_eq!(s.resolve(Trigger::Drag(Middle), Mods::NONE), Some(Pan));
        assert_eq!(s.resolve(Trigger::Drag(Middle), SHIFT), Some(Orbit));
        assert!(s.wheel_reversed);
        let s = Preset::SolidWorks.style();
        assert_eq!(s.resolve(Trigger::Drag(Middle), CTRL), Some(Pan));
        assert_eq!(s.resolve(Trigger::Drag(Middle), SHIFT), Some(ZoomDrag));
        let s = Preset::Onshape.style();
        assert_eq!(s.resolve(Trigger::Drag(Right), Mods::NONE), Some(Orbit));
        assert_eq!(s.resolve(Trigger::Drag(Middle), Mods::NONE), Some(Pan));
        assert!(s.drags_with(Right));
        let s = Preset::Rhino.style();
        assert_eq!(s.resolve(Trigger::Drag(Right), CTRL), Some(ZoomDrag));
        // Modifiers match exactly.
        assert_eq!(s.resolve(Trigger::Drag(Right), Mods { shift: true, ctrl: true, ..Mods::NONE }), None);
    }

    #[test]
    fn refused_bindings_are_not_applied() {
        use Button::*;
        use NavAction::*;
        let mut s = Preset::SketchUp.style();
        let n = s.bindings.len();
        s.bindings.push(Binding { trigger: Trigger::Drag(Middle), mods: Mods::NONE, action: Pan });
        assert_eq!(s.error(n), Some(BindingError::Duplicate));
        assert_eq!(s.resolve(Trigger::Drag(Middle), Mods::NONE), Some(Orbit), "the first one is kept");
        s.bindings.push(Binding { trigger: Trigger::Click(Right), mods: Mods::NONE, action: ZoomExtents });
        assert_eq!(s.error(n + 1), Some(BindingError::RightClick));
        s.bindings.push(Binding { trigger: Trigger::Click(Middle), mods: Mods::NONE, action: ZoomExtents });
        assert_eq!(s.error(n + 2), Some(BindingError::ClickAndDoubleClick));
        assert_eq!(s.resolve(Trigger::Click(Middle), Mods::NONE), None);
        s.bindings.push(Binding { trigger: Trigger::Drag(Back), mods: Mods::NONE, action: ZoomExtents });
        assert_eq!(s.error(n + 3), Some(BindingError::WrongKind));
        assert!(!s.drags_with(Back));
    }

    #[test]
    fn a_preset_is_stored_alone_until_edited() {
        let s = Preset::Fusion.style();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"preset":"fusion"}"#);
        assert_eq!(serde_json::from_str::<NavStyle>(&json).unwrap(), s);
        let mut s = Preset::Fusion.style();
        s.pivot = Pivot::ScreenCentre;
        s.bindings.pop();
        s.edited();
        let back: NavStyle = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Nothing stored: SketchUp.
        assert_eq!(serde_json::from_str::<NavStyle>("{}").unwrap(), NavStyle::default());
    }
}
