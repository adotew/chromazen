const MAX_ACTIONS: usize = 256;

#[derive(Clone, Copy)]
pub(super) enum AppHistoryAction {
    Paint,
}

#[derive(Default)]
pub(super) struct AppHistory {
    actions: Vec<AppHistoryAction>,
    cursor: usize,
}

impl AppHistory {
    pub(super) fn clear(&mut self) {
        self.actions.clear();
        self.cursor = 0;
    }

    pub(super) fn record_paint(&mut self) {
        self.record(AppHistoryAction::Paint);
    }

    pub(super) fn undo_action(&self) -> Option<AppHistoryAction> {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.actions.get(index))
            .cloned()
    }

    pub(super) fn commit_undo(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(super) fn redo_action(&self) -> Option<AppHistoryAction> {
        self.actions.get(self.cursor).cloned()
    }

    pub(super) fn commit_redo(&mut self) {
        self.cursor = (self.cursor + 1).min(self.actions.len());
    }

    pub(super) fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub(super) fn can_redo(&self) -> bool {
        self.cursor < self.actions.len()
    }

    fn record(&mut self, action: AppHistoryAction) {
        self.actions.truncate(self.cursor);
        self.actions.push(action);
        self.cursor = self.actions.len();
        if self.actions.len() > MAX_ACTIONS {
            let excess = self.actions.len() - MAX_ACTIONS;
            self.actions.drain(..excess);
            self.cursor = self.cursor.saturating_sub(excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_actions_discard_redo_entries() {
        let mut history = AppHistory::default();
        history.record_paint();
        history.record_paint();
        history.commit_undo();
        assert!(history.can_redo());

        history.record_paint();
        assert!(!history.can_redo());
        assert!(history.can_undo());
    }
}
