//! Right-click context menu overlay for the editor area.
//!
//! Shown on `MouseButton::Right` over the editor; offers Cut / Copy / Paste /
//! Delete / Select All. Items that need a selection (Cut / Copy / Delete) are
//! disabled when none is active. Dismissed by clicking outside, pressing Esc,
//! or executing an action.

use ratatui::layout::Rect;

/// What a context-menu entry does when activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    /// Copy the visual selection to the system clipboard, then delete it.
    Cut,
    /// Copy the visual selection to the system clipboard.
    Copy,
    /// Insert clipboard text at the cursor.
    Paste,
    /// Delete the visual selection without copying it.
    Delete,
    /// Select the entire buffer (visual mode, anchor=0, cursor=end).
    SelectAll,
}

/// A single entry in the context menu.
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    /// User-visible label.
    pub label: &'static str,
    /// Action triggered on activation.
    pub action: ContextMenuAction,
    /// Whether the item can currently be activated.
    pub enabled: bool,
}

/// Right-click popup state for the editor.
#[derive(Debug, Clone, Default)]
pub struct ContextMenu {
    /// Whether the menu is currently shown.
    pub visible: bool,
    /// Top-left anchor in terminal coordinates.
    pub anchor: (u16, u16),
    /// Currently highlighted item index.
    pub selected: usize,
    /// Items shown in the menu (built from app state when opened).
    pub items: Vec<ContextMenuItem>,
    /// Rect of the rendered menu — used to hit-test left clicks.
    pub rect: Rect,
}

impl ContextMenu {
    /// Build a menu populated from selection state and open it at `(col, row)`.
    pub fn open(&mut self, col: u16, row: u16, has_selection: bool) {
        self.items = vec![
            ContextMenuItem {
                label: "Cut",
                action: ContextMenuAction::Cut,
                enabled: has_selection,
            },
            ContextMenuItem {
                label: "Copy",
                action: ContextMenuAction::Copy,
                enabled: has_selection,
            },
            ContextMenuItem {
                label: "Paste",
                action: ContextMenuAction::Paste,
                enabled: true,
            },
            ContextMenuItem {
                label: "Delete",
                action: ContextMenuAction::Delete,
                enabled: has_selection,
            },
            ContextMenuItem {
                label: "Select All",
                action: ContextMenuAction::SelectAll,
                enabled: true,
            },
        ];
        // Highlight the first enabled item by default.
        self.selected = self.items.iter().position(|i| i.enabled).unwrap_or(0);
        self.anchor = (col, row);
        self.rect = Rect::default();
        self.visible = true;
    }

    /// Hide the menu and forget any cached rect.
    pub fn close(&mut self) {
        self.visible = false;
        self.rect = Rect::default();
    }

    /// Move highlight to the previous enabled item, wrapping around.
    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        for _ in 0..self.items.len() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
            if self.items[self.selected].enabled {
                return;
            }
        }
    }

    /// Move highlight to the next enabled item, wrapping around.
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        for _ in 0..self.items.len() {
            self.selected = (self.selected + 1) % self.items.len();
            if self.items[self.selected].enabled {
                return;
            }
        }
    }

    /// The currently highlighted action, if it is enabled.
    pub fn current_action(&self) -> Option<ContextMenuAction> {
        let item = self.items.get(self.selected)?;
        if item.enabled {
            Some(item.action)
        } else {
            None
        }
    }

    /// If `(col, row)` lands on an enabled item inside `self.rect`, return that
    /// action. The 1-cell border around the menu is treated as inert.
    pub fn action_at(&self, col: u16, row: u16) -> Option<ContextMenuAction> {
        let r = self.rect;
        if r.width == 0 || r.height == 0 {
            return None;
        }
        if col < r.x || col >= r.x + r.width || row < r.y || row >= r.y + r.height {
            return None;
        }
        // Items are listed below the top border, one row each.
        let inside_y = row.checked_sub(r.y + 1)? as usize;
        let item = self.items.get(inside_y)?;
        if item.enabled {
            Some(item.action)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_disables_selection_items_when_no_selection() {
        let mut menu = ContextMenu::default();
        menu.open(5, 5, false);
        assert!(menu.visible);
        let by_action = |a: ContextMenuAction| {
            menu.items
                .iter()
                .find(|i| i.action == a)
                .map(|i| i.enabled)
                .unwrap()
        };
        assert!(!by_action(ContextMenuAction::Cut));
        assert!(!by_action(ContextMenuAction::Copy));
        assert!(!by_action(ContextMenuAction::Delete));
        assert!(by_action(ContextMenuAction::Paste));
        assert!(by_action(ContextMenuAction::SelectAll));
    }

    #[test]
    fn open_enables_selection_items_when_selected() {
        let mut menu = ContextMenu::default();
        menu.open(0, 0, true);
        assert!(menu.items.iter().all(|i| i.enabled));
        // First item (Cut) is enabled, so it's the default highlight.
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn open_skips_disabled_items_for_default_highlight() {
        let mut menu = ContextMenu::default();
        menu.open(0, 0, false);
        // Cut/Copy disabled → first enabled is Paste at index 2.
        assert_eq!(menu.items[menu.selected].action, ContextMenuAction::Paste);
    }

    #[test]
    fn select_next_skips_disabled() {
        let mut menu = ContextMenu::default();
        menu.open(0, 0, false);
        // Start at Paste, next enabled is SelectAll, then wrap back to Paste.
        let start = menu.selected;
        menu.select_next();
        assert_eq!(
            menu.items[menu.selected].action,
            ContextMenuAction::SelectAll
        );
        menu.select_next();
        assert_eq!(menu.selected, start);
    }

    #[test]
    fn action_at_maps_row_to_item() {
        let mut menu = ContextMenu::default();
        menu.open(10, 10, true);
        menu.rect = Rect::new(10, 10, 14, 7); // 5 items + 2 border rows
                                              // Top border row → no action.
        assert!(menu.action_at(15, 10).is_none());
        // First content row → Cut.
        assert_eq!(menu.action_at(15, 11), Some(ContextMenuAction::Cut));
        // Last content row → Select All.
        assert_eq!(menu.action_at(15, 15), Some(ContextMenuAction::SelectAll));
        // Outside rect → none.
        assert!(menu.action_at(50, 50).is_none());
    }

    #[test]
    fn action_at_returns_none_for_disabled_item() {
        let mut menu = ContextMenu::default();
        menu.open(0, 0, false);
        menu.rect = Rect::new(0, 0, 14, 7);
        // Cut row is disabled when no selection.
        assert!(menu.action_at(5, 1).is_none());
        // Paste row is enabled.
        assert_eq!(menu.action_at(5, 3), Some(ContextMenuAction::Paste));
    }
}
