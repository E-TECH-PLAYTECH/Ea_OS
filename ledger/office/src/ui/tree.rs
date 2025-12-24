//! File tree widget for the file manager.

use crossterm::style::Color;
use super::{Rect, colors, truncate};

/// A node in the file tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// Display name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// Whether this is a directory.
    pub is_directory: bool,
    /// Whether directory is expanded.
    pub expanded: bool,
    /// Nesting depth.
    pub depth: u16,
    /// File size (for files).
    pub size: Option<u64>,
}

impl TreeNode {
    /// Create a directory node.
    pub fn directory(name: impl Into<String>, path: impl Into<String>, depth: u16) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            is_directory: true,
            expanded: false,
            depth,
            size: None,
        }
    }

    /// Create a file node.
    pub fn file(name: impl Into<String>, path: impl Into<String>, depth: u16, size: u64) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            is_directory: false,
            expanded: false,
            depth,
            size: Some(size),
        }
    }
}

/// Tree widget state.
#[derive(Debug, Clone)]
pub struct TreeState {
    /// All visible nodes.
    pub nodes: Vec<TreeNode>,
    /// Currently selected index.
    pub cursor: usize,
    /// Scroll offset.
    pub scroll_offset: usize,
}

impl Default for TreeState {
    fn default() -> Self {
        Self {
            nodes: vec![TreeNode::directory("/", "/", 0)],
            cursor: 0,
            scroll_offset: 0,
        }
    }
}

impl TreeState {
    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.cursor < self.nodes.len().saturating_sub(1) {
            self.cursor += 1;
        }
    }

    /// Get the selected node.
    pub fn selected(&self) -> Option<&TreeNode> {
        self.nodes.get(self.cursor)
    }

    /// Toggle directory expansion.
    pub fn toggle_expand(&mut self) {
        if let Some(node) = self.nodes.get_mut(self.cursor) {
            if node.is_directory {
                node.expanded = !node.expanded;
            }
        }
    }

    /// Update the tree with new nodes, preserving expanded state.
    pub fn update_nodes(&mut self, new_nodes: Vec<TreeNode>) {
        // Preserve expanded state for directories
        let expanded_paths: std::collections::HashSet<String> = self.nodes
            .iter()
            .filter(|n| n.is_directory && n.expanded)
            .map(|n| n.path.clone())
            .collect();

        self.nodes = new_nodes;
        for node in &mut self.nodes {
            if node.is_directory && expanded_paths.contains(&node.path) {
                node.expanded = true;
            }
        }

        // Clamp cursor
        if self.cursor >= self.nodes.len() {
            self.cursor = self.nodes.len().saturating_sub(1);
        }
    }

    /// Ensure cursor is visible.
    pub fn ensure_visible(&mut self, visible_lines: usize) {
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + visible_lines {
            self.scroll_offset = self.cursor - visible_lines + 1;
        }
    }
}

/// Format file size in human-readable form.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// Render the tree.
pub fn render_tree(
    state: &TreeState,
    area: &Rect,
) -> Vec<(u16, u16, String, Color)> {
    let mut output = Vec::new();

    if area.height < 3 || area.width < 15 {
        return output;
    }

    let content_height = (area.height - 2) as usize;
    let content_width = (area.width - 2) as usize;

    // Render visible nodes
    for (i, node_idx) in (state.scroll_offset..state.scroll_offset + content_height).enumerate() {
        let y = area.y + 1 + i as u16;

        if node_idx >= state.nodes.len() {
            break;
        }

        let node = &state.nodes[node_idx];
        let is_selected = node_idx == state.cursor;

        // Build the line
        let indent = "  ".repeat(node.depth as usize);
        let icon = if node.is_directory {
            if node.expanded { "📂" } else { "📁" }
        } else {
            "📄"
        };

        let size_str = node.size.map(format_size).unwrap_or_default();
        let name_width = content_width.saturating_sub(indent.len() + 3 + size_str.len() + 2);
        let name = truncate(&node.name, name_width);

        let line = format!("{}{} {} {}", indent, icon, name, size_str);
        let display = truncate(&line, content_width);

        let color = if is_selected {
            colors::HIGHLIGHT
        } else if node.is_directory {
            colors::ACCENT
        } else {
            colors::TEXT
        };

        output.push((area.x + 1, y, display, color));
    }

    // Scrollbar hint
    if state.nodes.len() > content_height {
        let scroll_pos = if state.nodes.is_empty() {
            0
        } else {
            (state.scroll_offset * content_height / state.nodes.len()).min(content_height - 1)
        };

        for i in 0..content_height {
            let y = area.y + 1 + i as u16;
            let char = if i == scroll_pos { "█" } else { "░" };
            output.push((area.x + area.width - 2, y, char.to_string(), colors::MUTED));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_navigation() {
        let mut tree = TreeState {
            nodes: vec![
                TreeNode::directory("/", "/", 0),
                TreeNode::directory("docs", "/docs", 1),
                TreeNode::file("readme.txt", "/docs/readme.txt", 2, 1024),
            ],
            cursor: 0,
            scroll_offset: 0,
        };

        tree.move_down();
        assert_eq!(tree.cursor, 1);

        tree.move_down();
        assert_eq!(tree.cursor, 2);

        tree.move_up();
        assert_eq!(tree.cursor, 1);
    }

    #[test]
    fn format_size_test() {
        assert_eq!(format_size(500), "500B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(1048576), "1.0M");
        assert_eq!(format_size(1073741824), "1.0G");
    }

    #[test]
    fn toggle_expand() {
        let mut tree = TreeState {
            nodes: vec![TreeNode::directory("/", "/", 0)],
            cursor: 0,
            scroll_offset: 0,
        };

        assert!(!tree.nodes[0].expanded);
        tree.toggle_expand();
        assert!(tree.nodes[0].expanded);
        tree.toggle_expand();
        assert!(!tree.nodes[0].expanded);
    }
}
