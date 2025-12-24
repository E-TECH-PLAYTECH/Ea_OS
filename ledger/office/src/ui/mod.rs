//! TUI widgets for the office suite.
//!
//! Common primitives and specialized widgets for each application.

pub mod editor;
pub mod grid;
pub mod tree;
pub mod calendar;

use crossterm::style::Color;

/// Common colors for the office UI.
pub mod colors {
    use super::Color;

    /// Primary accent color.
    pub const ACCENT: Color = Color::Cyan;
    /// Secondary accent color.
    pub const SECONDARY: Color = Color::Blue;
    /// Success/positive color.
    pub const SUCCESS: Color = Color::Green;
    /// Warning color.
    pub const WARNING: Color = Color::Yellow;
    /// Error/negative color.
    pub const ERROR: Color = Color::Red;
    /// Muted/disabled color.
    pub const MUTED: Color = Color::DarkGrey;
    /// Text color.
    pub const TEXT: Color = Color::White;
    /// Background highlight.
    pub const HIGHLIGHT: Color = Color::DarkBlue;
}

/// A rectangular region on the terminal.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    /// X coordinate (column).
    pub x: u16,
    /// Y coordinate (row).
    pub y: u16,
    /// Width in columns.
    pub width: u16,
    /// Height in rows.
    pub height: u16,
}

impl Rect {
    /// Create a new rectangle.
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self { x, y, width, height }
    }

    /// Create a rectangle from terminal size.
    pub fn from_size(width: u16, height: u16) -> Self {
        Self { x: 0, y: 0, width, height }
    }

    /// Get inner rect with padding.
    pub fn inner(&self, padding: u16) -> Self {
        Self {
            x: self.x + padding,
            y: self.y + padding,
            width: self.width.saturating_sub(padding * 2),
            height: self.height.saturating_sub(padding * 2),
        }
    }

    /// Split horizontally at a percentage.
    pub fn split_horizontal(&self, percent: u16) -> (Self, Self) {
        let split = (self.width as u32 * percent as u32 / 100) as u16;
        let left = Self {
            x: self.x,
            y: self.y,
            width: split,
            height: self.height,
        };
        let right = Self {
            x: self.x + split,
            y: self.y,
            width: self.width - split,
            height: self.height,
        };
        (left, right)
    }

    /// Split vertically at a percentage.
    pub fn split_vertical(&self, percent: u16) -> (Self, Self) {
        let split = (self.height as u32 * percent as u32 / 100) as u16;
        let top = Self {
            x: self.x,
            y: self.y,
            width: self.width,
            height: split,
        };
        let bottom = Self {
            x: self.x,
            y: self.y + split,
            width: self.width,
            height: self.height - split,
        };
        (top, bottom)
    }
}

/// Draw a box border.
pub fn draw_box(area: &Rect, title: Option<&str>) -> Vec<(u16, u16, String, Color)> {
    let mut output = Vec::new();

    if area.width < 2 || area.height < 2 {
        return output;
    }

    // Top border
    let top = format!(
        "┌{}┐",
        "─".repeat((area.width - 2) as usize)
    );
    output.push((area.x, area.y, top, colors::MUTED));

    // Title
    if let Some(title) = title {
        let title_str = format!("┤ {} ├", title);
        let title_x = area.x + 2;
        output.push((title_x, area.y, title_str, colors::ACCENT));
    }

    // Side borders
    for y in 1..area.height - 1 {
        output.push((area.x, area.y + y, "│".to_string(), colors::MUTED));
        output.push((area.x + area.width - 1, area.y + y, "│".to_string(), colors::MUTED));
    }

    // Bottom border
    let bottom = format!(
        "└{}┘",
        "─".repeat((area.width - 2) as usize)
    );
    output.push((area.x, area.y + area.height - 1, bottom, colors::MUTED));

    output
}

/// Truncate a string to fit a width, adding ellipsis if needed.
pub fn truncate(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;

    if s.width() <= max_width {
        return s.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut result = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + w + 3 > max_width {
            break;
        }
        result.push(c);
        width += w;
    }
    result.push_str("...");
    result
}

/// Pad a string to a fixed width.
pub fn pad(s: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;

    let current_width = s.width();
    if current_width >= width {
        truncate(s, width)
    } else {
        format!("{}{}", s, " ".repeat(width - current_width))
    }
}
