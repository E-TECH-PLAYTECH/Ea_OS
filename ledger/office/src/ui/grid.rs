//! Spreadsheet grid widget.

use crossterm::style::Color;
use crate::events::{CellRef, CellValue};
use super::{Rect, colors, pad, truncate};

/// Grid state for spreadsheet display.
#[derive(Debug, Clone)]
pub struct GridState {
    /// Selected cell column.
    pub cursor_col: u32,
    /// Selected cell row.
    pub cursor_row: u32,
    /// First visible column.
    pub scroll_col: u32,
    /// First visible row.
    pub scroll_row: u32,
    /// Whether we're editing the current cell.
    pub editing: bool,
    /// Current edit buffer.
    pub edit_buffer: String,
    /// Column widths (default 10).
    pub col_widths: Vec<u16>,
    /// Total columns.
    pub total_cols: u32,
    /// Total rows.
    pub total_rows: u32,
}

impl GridState {
    /// Create a new grid state.
    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            cursor_col: 0,
            cursor_row: 0,
            scroll_col: 0,
            scroll_row: 0,
            editing: false,
            edit_buffer: String::new(),
            col_widths: vec![10; cols as usize],
            total_cols: cols,
            total_rows: rows,
        }
    }

    /// Get current cell reference.
    pub fn current_cell(&self) -> CellRef {
        CellRef::new(self.cursor_col, self.cursor_row)
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.cursor_row < self.total_rows - 1 {
            self.cursor_row += 1;
        }
    }

    /// Move selection left.
    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Move selection right.
    pub fn move_right(&mut self) {
        if self.cursor_col < self.total_cols - 1 {
            self.cursor_col += 1;
        }
    }

    /// Start editing the current cell.
    pub fn start_edit(&mut self, initial: &str) {
        self.editing = true;
        self.edit_buffer = initial.to_string();
    }

    /// Cancel editing.
    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_buffer.clear();
    }

    /// Finish editing and return the value.
    pub fn finish_edit(&mut self) -> String {
        self.editing = false;
        std::mem::take(&mut self.edit_buffer)
    }

    /// Ensure cursor is visible.
    pub fn ensure_visible(&mut self, visible_cols: u32, visible_rows: u32) {
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + visible_cols {
            self.scroll_col = self.cursor_col - visible_cols + 1;
        }

        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + visible_rows {
            self.scroll_row = self.cursor_row - visible_rows + 1;
        }
    }
}

/// Cell getter function type.
pub type CellGetter<'a> = &'a dyn Fn(u32, u32) -> CellValue;

/// Render the grid.
pub fn render_grid<'a>(
    state: &GridState,
    area: &Rect,
    get_cell: CellGetter<'a>,
) -> Vec<(u16, u16, String, Color)> {
    let mut output = Vec::new();

    if area.height < 4 || area.width < 15 {
        return output;
    }

    let row_header_width = 5u16;
    let content_width = area.width.saturating_sub(row_header_width + 2);
    let content_height = area.height.saturating_sub(3);

    // Calculate visible columns
    let default_col_width = 10u16;
    let mut visible_cols = 0u32;
    let mut x_offset = 0u16;
    while x_offset < content_width && visible_cols < state.total_cols - state.scroll_col {
        let col_idx = state.scroll_col + visible_cols;
        let col_width = state.col_widths.get(col_idx as usize).copied().unwrap_or(default_col_width);
        x_offset += col_width + 1;
        visible_cols += 1;
    }

    let visible_rows = content_height.min(state.total_rows as u16 - state.scroll_row as u16);

    // Column headers
    let header_y = area.y + 1;
    output.push((area.x + 1, header_y, " ".repeat(row_header_width as usize), colors::MUTED));

    x_offset = row_header_width + 1;
    for col_offset in 0..visible_cols {
        let col_idx = state.scroll_col + col_offset;
        let col_width = state.col_widths.get(col_idx as usize).copied().unwrap_or(default_col_width);
        let col_name = CellRef::new(col_idx, 0).to_a1();
        let col_name = col_name.trim_end_matches(char::is_numeric);
        let header = pad(col_name, col_width as usize);

        let color = if col_idx == state.cursor_col {
            colors::ACCENT
        } else {
            colors::MUTED
        };

        output.push((area.x + 1 + x_offset, header_y, header, color));
        x_offset += col_width + 1;
    }

    // Separator line
    let sep_y = area.y + 2;
    let sep = "─".repeat((area.width - 2) as usize);
    output.push((area.x + 1, sep_y, sep, colors::MUTED));

    // Rows
    for row_offset in 0..visible_rows as u32 {
        let row_idx = state.scroll_row + row_offset;
        let y = area.y + 3 + row_offset as u16;

        // Row header
        let row_num = format!("{:>4} ", row_idx + 1);
        let row_color = if row_idx == state.cursor_row {
            colors::ACCENT
        } else {
            colors::MUTED
        };
        output.push((area.x + 1, y, row_num, row_color));

        // Cells
        x_offset = row_header_width + 1;
        for col_offset in 0..visible_cols {
            let col_idx = state.scroll_col + col_offset;
            let col_width = state.col_widths.get(col_idx as usize).copied().unwrap_or(default_col_width);

            let is_cursor = col_idx == state.cursor_col && row_idx == state.cursor_row;

            let cell_text = if is_cursor && state.editing {
                truncate(&state.edit_buffer, col_width as usize)
            } else {
                let value = get_cell(col_idx, row_idx);
                let text = match value {
                    CellValue::Empty => String::new(),
                    CellValue::Text(s) => s,
                    CellValue::Number(n) => format!("{}", n),
                    CellValue::Boolean(b) => if b { "TRUE" } else { "FALSE" }.to_string(),
                    CellValue::Error(e) => format!("#ERR: {}", e),
                };
                truncate(&text, col_width as usize)
            };

            let padded = pad(&cell_text, col_width as usize);

            let color = if is_cursor {
                colors::HIGHLIGHT
            } else {
                colors::TEXT
            };

            output.push((area.x + 1 + x_offset, y, padded, color));
            x_offset += col_width + 1;
        }
    }

    // Status bar
    let status_y = area.y + area.height - 1;
    let cell_ref = state.current_cell().to_a1();
    let mode = if state.editing { "EDIT" } else { "NAV" };
    let status = format!(" {} | Cell: {} ", mode, cell_ref);
    output.push((area.x + 1, status_y, status, colors::ACCENT));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_navigation() {
        let mut grid = GridState::new(10, 20);

        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);

        grid.move_right();
        assert_eq!(grid.cursor_col, 1);

        grid.move_down();
        assert_eq!(grid.cursor_row, 1);

        grid.move_left();
        assert_eq!(grid.cursor_col, 0);

        grid.move_up();
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn grid_edit_mode() {
        let mut grid = GridState::new(5, 5);

        grid.start_edit("Hello");
        assert!(grid.editing);
        assert_eq!(grid.edit_buffer, "Hello");

        let value = grid.finish_edit();
        assert!(!grid.editing);
        assert_eq!(value, "Hello");
    }

    #[test]
    fn current_cell_reference() {
        let mut grid = GridState::new(30, 100);
        grid.cursor_col = 2;
        grid.cursor_row = 4;

        let cell = grid.current_cell();
        assert_eq!(cell.to_a1(), "C5");
    }
}
