//! Undo and redo (`docs/design/editor.md`, "Keys"): whole snapshots of the editor's objects,
//! which are a few dozen bytes each.

pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// The series the last edit belongs to: further edits of the same series are one step.
    series: Option<u64>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            series: None,
        }
    }
}

impl<T> History<T> {
    /// Records the state `before` an edit. An edit of the same `series` as the last one joins
    /// its step: a run of arrow-key nudges undoes at once.
    pub fn record(&mut self, before: T, series: Option<u64>) {
        if series.is_some() && series == self.series {
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        self.series = series;
    }

    /// The state to go back to, given the `current` one.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        self.series = None;
        Some(prev)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.series = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_a_new_edit_clears_redo() {
        let mut h = History::default();
        let mut state = 0;
        for next in [1, 2, 3] {
            h.record(state, None);
            state = next;
        }
        state = h.undo(state).unwrap();
        assert_eq!(state, 2);
        state = h.undo(state).unwrap();
        assert_eq!(state, 1);
        state = h.redo(state).unwrap();
        assert_eq!(state, 2);
        assert!(h.can_redo());
        h.record(state, None);
        state = 7;
        assert!(!h.can_redo());
        assert_eq!(h.undo(state), Some(2));
    }

    #[test]
    fn a_series_is_one_step() {
        let mut h = History::default();
        let mut state = 0;
        for next in [1, 2, 3] {
            h.record(state, Some(5));
            state = next;
        }
        // Another series starts a new step.
        h.record(state, Some(6));
        state = 4;
        assert_eq!(h.undo(state), Some(3));
        assert_eq!(h.undo(3), Some(0));
        assert!(!h.can_undo());
        // After an undo the same series starts afresh.
        let mut h = History::default();
        h.record(0, Some(5));
        assert_eq!(h.undo(1), Some(0));
        h.record(0, Some(5));
        assert!(h.can_undo());
    }
}
