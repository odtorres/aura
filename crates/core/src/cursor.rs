//! Cursor position and selection management.

/// A position in the buffer as (row, col), both 0-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Zero-indexed line number.
    pub row: usize,
    /// Zero-indexed column (character offset within the line).
    pub col: usize,
}

impl Cursor {
    /// Create a cursor at the given row and column.
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    /// Create a cursor at position (0, 0).
    pub fn origin() -> Self {
        Self { row: 0, col: 0 }
    }
}

/// A selection is defined by an anchor (where selection started) and the cursor (where it ends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: Cursor,
    /// Current end of the selection.
    pub cursor: Cursor,
}

impl Selection {
    /// Returns (start, end) with start <= end.
    pub fn ordered(&self) -> (Cursor, Cursor) {
        if self.anchor.row < self.cursor.row
            || (self.anchor.row == self.cursor.row && self.anchor.col <= self.cursor.col)
        {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}
