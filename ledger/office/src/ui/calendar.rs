//! Calendar view widget.

use chrono::{Datelike, NaiveDate};
use crossterm::style::Color;
use super::{Rect, colors, pad};

/// Calendar view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarView {
    /// Month view (grid of days).
    Month,
    /// Week view (7 columns with hours).
    Week,
    /// Day view (single day with hours).
    Day,
}

impl Default for CalendarView {
    fn default() -> Self {
        Self::Month
    }
}

/// Calendar widget state.
#[derive(Debug, Clone)]
pub struct CalendarState {
    /// Current view mode.
    pub view: CalendarView,
    /// Currently focused year.
    pub year: i32,
    /// Currently focused month (1-12).
    pub month: u32,
    /// Currently focused day (1-31).
    pub day: u32,
    /// Selected date (if any).
    pub selected: Option<NaiveDate>,
}

impl Default for CalendarState {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            view: CalendarView::Month,
            year: today.year(),
            month: today.month(),
            day: today.day(),
            selected: Some(today),
        }
    }
}

impl CalendarState {
    /// Move to previous month.
    pub fn prev_month(&mut self) {
        if self.month == 1 {
            self.month = 12;
            self.year -= 1;
        } else {
            self.month -= 1;
        }
        self.clamp_day();
    }

    /// Move to next month.
    pub fn next_month(&mut self) {
        if self.month == 12 {
            self.month = 1;
            self.year += 1;
        } else {
            self.month += 1;
        }
        self.clamp_day();
    }

    /// Move to previous day.
    pub fn prev_day(&mut self) {
        if let Some(date) = self.current_date() {
            if let Some(prev) = date.pred_opt() {
                self.year = prev.year();
                self.month = prev.month();
                self.day = prev.day();
                self.selected = Some(prev);
            }
        }
    }

    /// Move to next day.
    pub fn next_day(&mut self) {
        if let Some(date) = self.current_date() {
            if let Some(next) = date.succ_opt() {
                self.year = next.year();
                self.month = next.month();
                self.day = next.day();
                self.selected = Some(next);
            }
        }
    }

    /// Move to previous week.
    pub fn prev_week(&mut self) {
        for _ in 0..7 {
            self.prev_day();
        }
    }

    /// Move to next week.
    pub fn next_week(&mut self) {
        for _ in 0..7 {
            self.next_day();
        }
    }

    /// Get the current date.
    pub fn current_date(&self) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(self.year, self.month, self.day)
    }

    /// Clamp day to valid range for current month.
    fn clamp_day(&mut self) {
        let max_day = days_in_month(self.year, self.month);
        if self.day > max_day {
            self.day = max_day;
        }
    }

    /// Select current date.
    pub fn select(&mut self) {
        self.selected = self.current_date();
    }

    /// Toggle view mode.
    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            CalendarView::Month => CalendarView::Week,
            CalendarView::Week => CalendarView::Day,
            CalendarView::Day => CalendarView::Month,
        };
    }
}

/// Get number of days in a month.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Event marker for calendar rendering.
#[derive(Debug, Clone)]
pub struct EventMarker {
    /// Date of the event.
    pub date: NaiveDate,
    /// Short title.
    pub title: String,
    /// Color for the marker.
    pub color: Color,
}

/// Render the month view.
pub fn render_month(
    state: &CalendarState,
    area: &Rect,
    events: &[EventMarker],
) -> Vec<(u16, u16, String, Color)> {
    let mut output = Vec::new();

    if area.height < 10 || area.width < 28 {
        return output;
    }

    let today = chrono::Local::now().date_naive();

    // Header with month/year
    let month_names = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December"
    ];
    let header = format!("{} {}", month_names[(state.month - 1) as usize], state.year);
    let header_x = area.x + (area.width - header.len() as u16) / 2;
    output.push((header_x, area.y + 1, header, colors::ACCENT));

    // Day of week headers
    let days = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
    let cell_width = 4u16;
    let grid_width = cell_width * 7;
    let grid_x = area.x + (area.width - grid_width) / 2;

    for (i, day) in days.iter().enumerate() {
        let x = grid_x + (i as u16) * cell_width;
        output.push((x, area.y + 3, pad(day, cell_width as usize), colors::MUTED));
    }

    // Calendar grid
    let first_day = NaiveDate::from_ymd_opt(state.year, state.month, 1).unwrap();
    let first_weekday = first_day.weekday().num_days_from_sunday() as u16;
    let num_days = days_in_month(state.year, state.month);

    let mut current_day = 1u32;
    let mut row = 0u16;

    while current_day <= num_days {
        let y = area.y + 4 + row;

        for col in 0..7u16 {
            let x = grid_x + col * cell_width;

            if row == 0 && col < first_weekday {
                // Empty cell before first day
                output.push((x, y, "    ".to_string(), colors::MUTED));
            } else if current_day <= num_days {
                let date = NaiveDate::from_ymd_opt(state.year, state.month, current_day);
                let day_str = format!("{:>2}", current_day);

                // Determine color
                let color = if date == state.selected {
                    colors::HIGHLIGHT
                } else if date == Some(today) {
                    colors::SUCCESS
                } else if events.iter().any(|e| Some(e.date) == date) {
                    colors::WARNING
                } else if col == 0 || col == 6 {
                    colors::MUTED
                } else {
                    colors::TEXT
                };

                // Add marker if there are events
                let has_event = events.iter().any(|e| Some(e.date) == date);
                let display = if has_event {
                    format!("{}*", day_str)
                } else {
                    format!("{} ", day_str)
                };

                output.push((x, y, pad(&display, cell_width as usize), color));
                current_day += 1;
            }
        }

        row += 1;
    }

    // Navigation hints
    let hint_y = area.y + area.height - 1;
    let hint = "← → Month | ↑ ↓ Week | Enter: Select";
    output.push((area.x + 1, hint_y, hint.to_string(), colors::MUTED));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_navigation() {
        let mut cal = CalendarState::default();
        let initial_month = cal.month;

        cal.next_month();
        if initial_month == 12 {
            assert_eq!(cal.month, 1);
        } else {
            assert_eq!(cal.month, initial_month + 1);
        }

        cal.prev_month();
        assert_eq!(cal.month, initial_month);
    }

    #[test]
    fn days_in_month_test() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 2), 29); // Leap year
        assert_eq!(days_in_month(2023, 2), 28); // Non-leap
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn day_clamping() {
        let mut cal = CalendarState {
            year: 2024,
            month: 3,
            day: 31,
            view: CalendarView::Month,
            selected: None,
        };

        // March has 31 days, April has 30
        cal.next_month();
        assert_eq!(cal.day, 30);

        // Go to February (29 days in 2024)
        cal.month = 2;
        cal.day = 31;
        cal.clamp_day();
        assert_eq!(cal.day, 29);
    }

    #[test]
    fn view_toggle() {
        let mut cal = CalendarState::default();
        assert_eq!(cal.view, CalendarView::Month);

        cal.toggle_view();
        assert_eq!(cal.view, CalendarView::Week);

        cal.toggle_view();
        assert_eq!(cal.view, CalendarView::Day);

        cal.toggle_view();
        assert_eq!(cal.view, CalendarView::Month);
    }
}
