//! Text editor widget for document editing.

use crossterm::style::Color;
use super::{Rect, colors, truncate};

/// Text editor state.
#[derive(Debug, Clone)]
pub struct EditorState {
    /// Lines of text.
    pub lines: Vec<String>,
    /// Cursor column.
    pub cursor_x: usize,
    /// Cursor row.
    pub cursor_y: usize,
    /// Scroll offset (first visible line).
    pub scroll_offset: usize,
    /// Whether the editor is in insert mode.
    pub insert_mode: bool,
    /// Whether the content has been modified.
    pub modified: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_x: 0,
            cursor_y: 0,
            scroll_offset: 0,
            insert_mode: false,
            modified: false,
        }
    }
}

impl EditorState {
    /// Create a new editor with content.
    pub fn with_content(content: &str) -> Self {
        let lines: Vec<String> = content.lines().map(String::from).collect();
        let lines = if lines.is_empty() { vec![String::new()] } else { lines };
        Self {
            lines,
            cursor_x: 0,
            cursor_y: 0,
            scroll_offset: 0,
            insert_mode: false,
            modified: false,
        }
    }

    /// Get the current line.
    pub fn current_line(&self) -> &str {
        self.lines.get(self.cursor_y).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get mutable current line.
    pub fn current_line_mut(&mut self) -> &mut String {
        if self.cursor_y >= self.lines.len() {
            self.lines.push(String::new());
        }
        &mut self.lines[self.cursor_y]
    }

    /// Move cursor up.
    pub fn move_up(&mut self) {
        if self.cursor_y > 0 {
            self.cursor_y -= 1;
            self.clamp_cursor_x();
        }
    }

    /// Move cursor down.
    pub fn move_down(&mut self) {
        if self.cursor_y < self.lines.len().saturating_sub(1) {
            self.cursor_y += 1;
            self.clamp_cursor_x();
        }
    }

    /// Move cursor left.
    pub fn move_left(&mut self) {
        if self.cursor_x > 0 {
            self.cursor_x -= 1;
        } else if self.cursor_y > 0 {
            self.cursor_y -= 1;
            self.cursor_x = self.current_line().len();
        }
    }

    /// Move cursor right.
    pub fn move_right(&mut self) {
        let line_len = self.current_line().len();
        if self.cursor_x < line_len {
            self.cursor_x += 1;
        } else if self.cursor_y < self.lines.len() - 1 {
            self.cursor_y += 1;
            self.cursor_x = 0;
        }
    }

    /// Move to start of line.
    pub fn move_to_line_start(&mut self) {
        self.cursor_x = 0;
    }

    /// Move to end of line.
    pub fn move_to_line_end(&mut self) {
        self.cursor_x = self.current_line().len();
    }

    /// Insert a character at cursor.
    pub fn insert_char(&mut self, c: char) {
        let cursor_x = self.cursor_x;
        let line = self.current_line_mut();
        let insert_pos = cursor_x.min(line.len());
        line.insert(insert_pos, c);
        self.cursor_x = insert_pos + 1;
        self.modified = true;
    }

    /// Delete character before cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor_x > 0 {
            let cursor_x = self.cursor_x;
            let line = self.current_line_mut();
            line.remove(cursor_x - 1);
            self.cursor_x -= 1;
            self.modified = true;
        } else if self.cursor_y > 0 {
            let current_line = self.lines.remove(self.cursor_y);
            self.cursor_y -= 1;
            self.cursor_x = self.lines[self.cursor_y].len();
            self.lines[self.cursor_y].push_str(&current_line);
            self.modified = true;
        }
    }

    /// Delete character at cursor.
    pub fn delete(&mut self) {
        let line_len = self.current_line().len();
        let cursor_x = self.cursor_x;
        if cursor_x < line_len {
            self.current_line_mut().remove(cursor_x);
            self.modified = true;
        } else if self.cursor_y < self.lines.len() - 1 {
            let next_line = self.lines.remove(self.cursor_y + 1);
            self.lines[self.cursor_y].push_str(&next_line);
            self.modified = true;
        }
    }

    /// Insert a newline at cursor.
    pub fn newline(&mut self) {
        let cursor_x = self.cursor_x;
        let line = self.current_line_mut();
        let split_pos = cursor_x.min(line.len());
        let remainder = line.split_off(split_pos);
        self.lines.insert(self.cursor_y + 1, remainder);
        self.cursor_y += 1;
        self.cursor_x = 0;
        self.modified = true;
    }

    /// Get content as a single string.
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Ensure cursor X is within line bounds.
    fn clamp_cursor_x(&mut self) {
        let line_len = self.current_line().len();
        if self.cursor_x > line_len {
            self.cursor_x = line_len;
        }
    }

    /// Ensure scroll keeps cursor visible.
    pub fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        if self.cursor_y < self.scroll_offset {
            self.scroll_offset = self.cursor_y;
        } else if self.cursor_y >= self.scroll_offset + visible_lines {
            self.scroll_offset = self.cursor_y - visible_lines + 1;
        }
    }
}

/// Render the editor to a list of positioned strings.
pub fn render_editor(
    state: &EditorState,
    area: &Rect,
) -> Vec<(u16, u16, String, Color)> {
    let mut output = Vec::new();

    if area.height < 3 || area.width < 10 {
        return output;
    }

    // Calculate visible region
    let content_height = (area.height - 2) as usize;
    let content_width = (area.width - 6) as usize; // Leave room for line numbers
    let line_num_width = 4;

    // Render visible lines
    for (i, line_idx) in (state.scroll_offset..state.scroll_offset + content_height).enumerate() {
        let y = area.y + 1 + i as u16;

        if line_idx < state.lines.len() {
            // Line number
            let line_num = format!("{:>3} ", line_idx + 1);
            output.push((area.x + 1, y, line_num, colors::MUTED));

            // Line content
            let line = &state.lines[line_idx];
            let display_line = truncate(line, content_width);
            output.push((area.x + 1 + line_num_width, y, display_line, colors::TEXT));

            // Cursor
            if line_idx == state.cursor_y && state.insert_mode {
                let cursor_x = area.x + 1 + line_num_width + state.cursor_x as u16;
                if cursor_x < area.x + area.width - 1 {
                    let cursor_char = line.chars().nth(state.cursor_x).unwrap_or(' ');
                    output.push((cursor_x, y, cursor_char.to_string(), colors::HIGHLIGHT));
                }
            }
        } else {
            // Empty line indicator
            output.push((area.x + 1, y, "  ~ ".to_string(), colors::MUTED));
        }
    }

    // Status bar
    let status_y = area.y + area.height - 1;
    let mode = if state.insert_mode { "INSERT" } else { "NORMAL" };
    let modified = if state.modified { "[+]" } else { "" };
    let position = format!("{}:{}", state.cursor_y + 1, state.cursor_x + 1);
    let status = format!(
        " {} {} {:>10} ",
        mode, modified, position
    );
    output.push((area.x + 1, status_y, status, colors::ACCENT));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_basic_operations() {
        let mut editor = EditorState::default();

        // Insert text
        editor.insert_mode = true;
        for c in "Hello".chars() {
            editor.insert_char(c);
        }
        assert_eq!(editor.content(), "Hello");
        assert_eq!(editor.cursor_x, 5);

        // Backspace
        editor.backspace();
        assert_eq!(editor.content(), "Hell");

        // Move cursor
        editor.move_left();
        editor.insert_char('p');
        assert_eq!(editor.content(), "Helpl");
    }

    #[test]
    fn editor_multiline() {
        let mut editor = EditorState::with_content("Line 1\nLine 2\nLine 3");

        assert_eq!(editor.lines.len(), 3);

        editor.move_down();
        assert_eq!(editor.cursor_y, 1);
        assert_eq!(editor.current_line(), "Line 2");

        editor.move_to_line_end();
        assert_eq!(editor.cursor_x, 6);
    }

    #[test]
    fn editor_newline() {
        let mut editor = EditorState::default();
        editor.insert_mode = true;

        for c in "Hello".chars() {
            editor.insert_char(c);
        }
        editor.newline();
        for c in "World".chars() {
            editor.insert_char(c);
        }

        assert_eq!(editor.lines.len(), 2);
        assert_eq!(editor.content(), "Hello\nWorld");
    }
}
