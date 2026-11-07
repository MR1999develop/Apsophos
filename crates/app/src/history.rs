//! General Undo/Redo.
//!
//! Actions are objects that know how to apply and revert themselves on the
//! user-editable state. Today the undoable state is the parameters; new
//! actions are added by implementing [`Action`] in the module that knows them.

use denoise_core::Params;

/// State the undoable actions work on.
pub type State = Params;

/// An undoable action.
pub trait Action {
    fn apply(&self, state: &mut State);
    fn revert(&self, state: &mut State);
    /// Name shown in the menu: "Undo <label>".
    fn label(&self) -> String;
}

/// Change of the parameters (a dragged slider, a preset…).
pub struct ParamsChange {
    pub before: Params,
    pub after: Params,
    pub what: String,
}

impl Action for ParamsChange {
    fn apply(&self, state: &mut State) {
        *state = self.after;
    }

    fn revert(&self, state: &mut State) {
        *state = self.before;
    }

    fn label(&self) -> String {
        self.what.clone()
    }
}

/// History with a length limit (actions are small: the limit is enough to
/// bound memory).
pub struct History {
    done: Vec<Box<dyn Action>>,
    undone: Vec<Box<dyn Action>>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { done: Vec::new(), undone: Vec::new(), limit: 500 }
    }
}

impl History {
    /// Records an action already applied to the state.
    pub fn push(&mut self, action: Box<dyn Action>) {
        self.undone.clear();
        self.done.push(action);
        if self.done.len() > self.limit {
            self.done.remove(0);
        }
    }

    /// Records a parameter change, if something actually changed.
    pub fn push_params(&mut self, before: Params, after: Params, what: impl Into<String>) {
        if before != after {
            self.push(Box::new(ParamsChange { before, after, what: what.into() }));
        }
    }

    pub fn undo(&mut self, state: &mut State) -> bool {
        match self.done.pop() {
            Some(a) => {
                a.revert(state);
                self.undone.push(a);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, state: &mut State) -> bool {
        match self.undone.pop() {
            Some(a) => {
                a.apply(state);
                self.done.push(a);
                true
            }
            None => false,
        }
    }

    pub fn undo_label(&self) -> Option<String> {
        self.done.last().map(|a| a.label())
    }

    pub fn redo_label(&self) -> Option<String> {
        self.undone.last().map(|a| a.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use denoise_core::{Percent, Preset};

    #[test]
    fn undo_redo_roundtrip() {
        let mut h = History::default();
        let mut p = Params::default();
        let before = p;
        p.luma = Percent::clamped(30.0);
        h.push_params(before, p, "grain 30 %");
        assert_eq!(h.undo_label().as_deref(), Some("grain 30 %"));
        assert!(h.undo(&mut p));
        assert_eq!(p, before);
        assert!(h.redo(&mut p));
        assert_eq!(p.luma, Percent::clamped(30.0));
        assert!(!h.redo(&mut p));
    }

    #[test]
    fn no_op_changes_are_not_recorded_and_new_action_clears_redo() {
        let mut h = History::default();
        let mut p = Params::default();
        h.push_params(p, p, "nothing");
        assert!(h.undo_label().is_none());
        h.push_params(p, Preset::Strong.params(), "Strong");
        p = Preset::Strong.params();
        h.undo(&mut p);
        h.push_params(p, Preset::Light.params(), "Light");
        assert!(h.redo_label().is_none());
    }
}
