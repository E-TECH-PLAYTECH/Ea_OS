//! Eä Office Suite - Unified TUI Launcher
//!
//! A complete office suite with cryptographic versioning and Merkle proofs.

use std::io::{stdout, Write};
use std::time::Duration;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ed25519_dalek::SigningKey;
use rand_core::OsRng;

use ledger_core::brainstem::Ledger;
use ledger_spec::{ChannelPolicy, ChannelRegistry, ChannelSpec};

use ledger_office::{
    CalendarApp, DocumentApp, FileManagerApp, SpreadsheetApp,
    ui::{self, Rect, colors, draw_box},
    ui::editor::{EditorState, render_editor},
    ui::grid::{GridState, render_grid},
    ui::tree::{TreeState, TreeNode, render_tree},
    ui::calendar::{CalendarState, render_month, EventMarker},
};

/// Application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    /// Main menu.
    Menu,
    /// Document editor.
    Documents,
    /// Spreadsheet.
    Spreadsheet,
    /// File manager.
    Files,
    /// Calendar.
    Calendar,
}

/// Main application state.
struct OfficeTui {
    mode: AppMode,
    running: bool,
    menu_selection: usize,

    // Apps
    doc_app: DocumentApp,
    sheet_app: SpreadsheetApp,
    file_app: FileManagerApp,
    cal_app: CalendarApp,

    // UI states
    editor_state: EditorState,
    grid_state: GridState,
    tree_state: TreeState,
    calendar_state: CalendarState,

    // Status
    status_message: String,
    last_receipt: Option<String>,
}

impl OfficeTui {
    fn new() -> Self {
        let signer = SigningKey::generate(&mut OsRng);
        let pub_key = signer.verifying_key().to_bytes();

        // Create channel registry
        let mut registry = ChannelRegistry::new();
        for channel in ["office.documents", "office.spreadsheets", "office.files", "office.calendar"] {
            registry.upsert(ChannelSpec {
                name: channel.into(),
                policy: ChannelPolicy {
                    min_signers: 1,
                    allowed_signers: vec![pub_key],
                    require_attestations: false,
                    enforce_timestamp_ordering: true,
                },
            });
        }

        let ledger = Ledger::new(registry);

        Self {
            mode: AppMode::Menu,
            running: true,
            menu_selection: 0,

            doc_app: DocumentApp::new(ledger.clone(), signer.clone(), "office.documents", 1),
            sheet_app: SpreadsheetApp::new(ledger.clone(), signer.clone(), "office.spreadsheets", 1),
            file_app: FileManagerApp::new(ledger.clone(), signer.clone(), "office.files", 1),
            cal_app: CalendarApp::new(ledger, signer, "office.calendar", 1),

            editor_state: EditorState::default(),
            grid_state: GridState::new(26, 100),
            tree_state: TreeState::default(),
            calendar_state: CalendarState::default(),

            status_message: "Welcome to Eä Office Suite".into(),
            last_receipt: None,
        }
    }

    fn handle_input(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        match self.mode {
            AppMode::Menu => self.handle_menu_input(key),
            AppMode::Documents => self.handle_editor_input(key, modifiers),
            AppMode::Spreadsheet => self.handle_grid_input(key),
            AppMode::Files => self.handle_files_input(key),
            AppMode::Calendar => self.handle_calendar_input(key),
        }
    }

    fn handle_menu_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => {
                if self.menu_selection > 0 {
                    self.menu_selection -= 1;
                }
            }
            KeyCode::Down => {
                if self.menu_selection < 3 {
                    self.menu_selection += 1;
                }
            }
            KeyCode::Enter => {
                self.mode = match self.menu_selection {
                    0 => AppMode::Documents,
                    1 => AppMode::Spreadsheet,
                    2 => AppMode::Files,
                    3 => AppMode::Calendar,
                    _ => AppMode::Menu,
                };
                self.status_message = format!("Opened {:?}", self.mode);
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.running = false;
            }
            _ => {}
        }
    }

    fn handle_editor_input(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        if self.editor_state.insert_mode {
            match key {
                KeyCode::Esc => {
                    self.editor_state.insert_mode = false;
                }
                KeyCode::Enter => {
                    self.editor_state.newline();
                }
                KeyCode::Backspace => {
                    self.editor_state.backspace();
                }
                KeyCode::Delete => {
                    self.editor_state.delete();
                }
                KeyCode::Left => self.editor_state.move_left(),
                KeyCode::Right => self.editor_state.move_right(),
                KeyCode::Up => self.editor_state.move_up(),
                KeyCode::Down => self.editor_state.move_down(),
                KeyCode::Char(c) => {
                    self.editor_state.insert_char(c);
                }
                _ => {}
            }
        } else {
            match key {
                KeyCode::Char('i') => {
                    self.editor_state.insert_mode = true;
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.mode = AppMode::Menu;
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.save_document();
                }
                KeyCode::Char('h') | KeyCode::Left => self.editor_state.move_left(),
                KeyCode::Char('j') | KeyCode::Down => self.editor_state.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.editor_state.move_up(),
                KeyCode::Char('l') | KeyCode::Right => self.editor_state.move_right(),
                KeyCode::Char('0') => self.editor_state.move_to_line_start(),
                KeyCode::Char('$') => self.editor_state.move_to_line_end(),
                _ => {}
            }
        }
    }

    fn save_document(&mut self) {
        let content = self.editor_state.content();
        let docs = self.doc_app.list_documents();

        let result = if let Some(doc) = docs.first() {
            self.doc_app.update_document(doc.id, content)
        } else {
            self.doc_app.create_document("Untitled")
                .and_then(|(doc, _)| self.doc_app.update_document(doc.id, self.editor_state.content()))
        };

        match result {
            Ok((doc, receipt)) => {
                self.editor_state.modified = false;
                self.status_message = format!("Saved v{} with Merkle proof", doc.version);
                self.last_receipt = Some(hex::encode(&receipt.merkle.root[..8]));
            }
            Err(e) => {
                self.status_message = format!("Save failed: {}", e);
            }
        }
    }

    fn handle_grid_input(&mut self, key: KeyCode) {
        if self.grid_state.editing {
            match key {
                KeyCode::Esc => {
                    self.grid_state.cancel_edit();
                }
                KeyCode::Enter => {
                    let value = self.grid_state.finish_edit();
                    self.update_cell(value);
                }
                KeyCode::Backspace => {
                    self.grid_state.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.grid_state.edit_buffer.push(c);
                }
                _ => {}
            }
        } else {
            match key {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.mode = AppMode::Menu;
                }
                KeyCode::Up => self.grid_state.move_up(),
                KeyCode::Down => self.grid_state.move_down(),
                KeyCode::Left => self.grid_state.move_left(),
                KeyCode::Right => self.grid_state.move_right(),
                KeyCode::Enter | KeyCode::Char('e') => {
                    self.grid_state.start_edit("");
                }
                _ => {}
            }
        }
    }

    fn update_cell(&mut self, value: String) {
        use ledger_office::events::CellValue;

        let sheets = self.sheet_app.list_sheets();
        let sheet_id = if let Some(sheet) = sheets.first() {
            sheet.id
        } else {
            match self.sheet_app.create_sheet("Sheet1", 26, 100) {
                Ok((sheet, _)) => sheet.id,
                Err(e) => {
                    self.status_message = format!("Failed to create sheet: {}", e);
                    return;
                }
            }
        };

        let cell_value = if value.is_empty() {
            CellValue::Empty
        } else if let Ok(n) = value.parse::<f64>() {
            CellValue::Number(n)
        } else if value.starts_with('=') {
            CellValue::Empty // Formula will be parsed by spreadsheet app
        } else {
            CellValue::Text(value.clone())
        };

        let formula = if value.starts_with('=') { Some(value) } else { None };

        match self.sheet_app.update_cell(
            sheet_id,
            self.grid_state.cursor_col,
            self.grid_state.cursor_row,
            cell_value,
            formula,
        ) {
            Ok(receipt) => {
                self.status_message = "Cell updated".into();
                self.last_receipt = Some(hex::encode(&receipt.merkle.root[..8]));
            }
            Err(e) => {
                self.status_message = format!("Update failed: {}", e);
            }
        }
    }

    fn handle_files_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.mode = AppMode::Menu;
            }
            KeyCode::Up => self.tree_state.move_up(),
            KeyCode::Down => self.tree_state.move_down(),
            KeyCode::Enter => {
                self.tree_state.toggle_expand();
                self.refresh_file_tree();
            }
            _ => {}
        }
    }

    fn refresh_file_tree(&mut self) {
        let mut nodes = vec![TreeNode::directory("/", "/", 0)];

        if let Ok(entries) = self.file_app.list_directory("/") {
            for entry in entries {
                let node = if entry.is_directory() {
                    TreeNode::directory(entry.name(), &entry.path, 1)
                } else {
                    TreeNode::file(entry.name(), &entry.path, 1, entry.metadata.size)
                };
                nodes.push(node);
            }
        }

        self.tree_state.update_nodes(nodes);
    }

    fn handle_calendar_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.mode = AppMode::Menu;
            }
            KeyCode::Left => self.calendar_state.prev_month(),
            KeyCode::Right => self.calendar_state.next_month(),
            KeyCode::Up => self.calendar_state.prev_week(),
            KeyCode::Down => self.calendar_state.next_week(),
            KeyCode::Enter => {
                self.calendar_state.select();
                if let Some(date) = self.calendar_state.selected {
                    self.status_message = format!("Selected: {}", date);
                }
            }
            KeyCode::Char('v') => {
                self.calendar_state.toggle_view();
            }
            _ => {}
        }
    }

    fn render(&self, width: u16, height: u16) -> Vec<(u16, u16, String, Color)> {
        let mut output = Vec::new();
        let area = Rect::new(0, 0, width, height);

        match self.mode {
            AppMode::Menu => output.extend(self.render_menu(&area)),
            AppMode::Documents => output.extend(self.render_documents(&area)),
            AppMode::Spreadsheet => output.extend(self.render_spreadsheet(&area)),
            AppMode::Files => output.extend(self.render_files(&area)),
            AppMode::Calendar => output.extend(self.render_calendar(&area)),
        }

        // Status bar
        let status_y = height - 1;
        let receipt_info = self.last_receipt.as_ref()
            .map(|r| format!(" | Receipt: {}...", r))
            .unwrap_or_default();
        let status = format!(" {} {}", self.status_message, receipt_info);
        output.push((0, status_y, ui::pad(&status, width as usize), colors::ACCENT));

        output
    }

    fn render_menu(&self, area: &Rect) -> Vec<(u16, u16, String, Color)> {
        let mut output = Vec::new();

        // Title
        let title = "╔═══════════════════════════════════════╗";
        let title_x = (area.width - title.len() as u16) / 2;
        output.push((title_x, 2, title.into(), colors::ACCENT));
        output.push((title_x, 3, "║         Eä OFFICE SUITE              ║".into(), colors::ACCENT));
        output.push((title_x, 4, "║   Ledger-Backed Productivity Apps    ║".into(), colors::ACCENT));
        output.push((title_x, 5, "╚═══════════════════════════════════════╝".into(), colors::ACCENT));

        // Menu items
        let items = [
            ("📝", "Documents", "Markdown editor with version history"),
            ("📊", "Spreadsheet", "Grid with formulas and cell tracking"),
            ("📁", "Files", "CAS-backed file manager"),
            ("📅", "Calendar", "Event scheduling with audit trail"),
        ];

        for (i, (icon, name, desc)) in items.iter().enumerate() {
            let y = 8 + (i as u16) * 2;
            let selected = i == self.menu_selection;
            let prefix = if selected { "▶ " } else { "  " };
            let color = if selected { colors::HIGHLIGHT } else { colors::TEXT };

            output.push((title_x + 2, y, format!("{}{} {}", prefix, icon, name), color));
            output.push((title_x + 6, y + 1, desc.to_string(), colors::MUTED));
        }

        // Help
        output.push((title_x, 18, "↑↓: Navigate | Enter: Select | Q: Quit".into(), colors::MUTED));

        output
    }

    fn render_documents(&self, area: &Rect) -> Vec<(u16, u16, String, Color)> {
        let mut output = draw_box(area, Some("Documents"));
        let inner = area.inner(1);
        output.extend(render_editor(&self.editor_state, &inner));
        output
    }

    fn render_spreadsheet(&self, area: &Rect) -> Vec<(u16, u16, String, Color)> {
        let mut output = draw_box(area, Some("Spreadsheet"));
        let inner = area.inner(1);

        let get_cell = |col: u32, row: u32| {
            let sheets = self.sheet_app.list_sheets();
            if let Some(sheet) = sheets.first() {
                sheet.get_cell(col, row).value.clone()
            } else {
                ledger_office::events::CellValue::Empty
            }
        };

        output.extend(render_grid(&self.grid_state, &inner, &get_cell));
        output
    }

    fn render_files(&self, area: &Rect) -> Vec<(u16, u16, String, Color)> {
        let mut output = draw_box(area, Some("Files"));
        let inner = area.inner(1);
        output.extend(render_tree(&self.tree_state, &inner));
        output
    }

    fn render_calendar(&self, area: &Rect) -> Vec<(u16, u16, String, Color)> {
        let mut output = draw_box(area, Some("Calendar"));
        let inner = area.inner(1);

        // Get events for display
        let events: Vec<EventMarker> = self.cal_app.list_active_events()
            .iter()
            .filter_map(|e| {
                use chrono::{TimeZone, Utc};
                let date = Utc.timestamp_millis_opt(e.start as i64).single()?;
                Some(EventMarker {
                    date: date.date_naive(),
                    title: e.title.clone(),
                    color: colors::WARNING,
                })
            })
            .collect();

        output.extend(render_month(&self.calendar_state, &inner, &events));
        output
    }
}

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    // Setup terminal
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let mut app = OfficeTui::new();
    let frame_duration = Duration::from_millis(50);

    while app.running {
        // Handle input
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                app.handle_input(key.code, key.modifiers);
            }
        }

        // Get terminal size
        let (width, height) = terminal::size()?;

        // Clear and render
        execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

        for (x, y, text, color) in app.render(width, height) {
            execute!(
                stdout,
                MoveTo(x, y),
                SetForegroundColor(color),
                Print(&text),
                ResetColor
            )?;
        }

        stdout.flush()?;
        std::thread::sleep(frame_duration);
    }

    // Cleanup
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    println!("🧬 Eä Office Suite closed");
    Ok(())
}
