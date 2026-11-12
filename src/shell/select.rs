//! Selecting text on the screen with the mouse and copying it. This works
//! over whatever is drawn, so every tool has it.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;
use ratatui::style::{Color, Style};

#[cfg(not(test))]
use crate::atoms::sys::clipboard;
use crate::atoms::ui::select::{self, Pointer, PointerEffect, PointerEvent};

/// Whether an event is still for the tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    On,
    Swallow,
}

/// The drag in progress, what the screen showed, and what came of a copy.
#[derive(Default)]
pub struct Selection {
    pointer: Pointer,
    /// The last frame, as rows of cell symbols.
    screen: Vec<Vec<String>>,
    note: Option<String>,
    failed: bool,
    /// What tests copied, instead of the clipboard.
    #[cfg(test)]
    pub copied: Vec<String>,
}

impl Selection {
    /// A mouse event. A drag with the left button selects and is kept
    /// from the tool; a press and release in one place reaches the tool
    /// as a click, with the release moved to where the press was.
    pub fn on_mouse(&mut self, mouse: &mut MouseEvent) -> Pass {
        let at = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.note = None;
                self.failed = false;
                self.pointer.on(PointerEvent::Down, at);
                Pass::On
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.pointer.on(PointerEvent::Drag, at);
                Pass::Swallow
            }
            MouseEventKind::Up(MouseButton::Left) => match self.pointer.on(PointerEvent::Up, at) {
                PointerEffect::Click(x, y) => {
                    mouse.column = x;
                    mouse.row = y;
                    Pass::On
                }
                PointerEffect::Copy => {
                    self.copy();
                    Pass::Swallow
                }
                PointerEffect::None => Pass::Swallow,
            },
            MouseEventKind::Down(MouseButton::Right)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown => {
                self.dismiss();
                Pass::On
            }
            _ => Pass::On,
        }
    }

    /// A key. Ctrl-C copies a selection and Esc drops one; neither then
    /// reaches the tool. Any other key drops the selection and goes on.
    pub fn on_key(&mut self, key: KeyEvent) -> Pass {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let selected = self.pointer.selection.is_some();
        if selected && ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.copy();
            return Pass::Swallow;
        }
        if selected && key.code == KeyCode::Esc {
            self.dismiss();
            return Pass::Swallow;
        }
        self.pointer.selection = None;
        self.note = None;
        self.failed = false;
        Pass::On
    }

    /// What to tell the user, and whether it is an error.
    pub fn note(&self) -> Option<(&str, bool)> {
        if let Some(note) = &self.note {
            return Some((note, self.failed));
        }
        self.pointer
            .selection
            .is_some()
            .then_some(("release to copy", false))
    }

    /// Call when the frame is drawn: remembers its text and marks what is
    /// selected.
    pub fn finish(&mut self, buf: &mut Buffer) {
        self.screen = capture(buf);
        if let Some(sel) = self.pointer.selection {
            paint(buf, sel);
        }
    }

    fn dismiss(&mut self) {
        self.pointer = Pointer::default();
        self.note = None;
        self.failed = false;
    }

    fn copy(&mut self) {
        let Some(sel) = self.pointer.selection else {
            return;
        };
        let text = select::selected_text(&self.screen, sel.anchor, sel.head);
        if text.is_empty() {
            self.pointer.selection = None;
            self.note = None;
            self.failed = false;
            return;
        }
        match self.put(&text) {
            Ok(()) => {
                self.note = Some("Copied to the clipboard".to_string());
                self.failed = false;
            }
            Err(err) => {
                self.note = Some(err);
                self.failed = true;
            }
        }
    }

    #[cfg(not(test))]
    fn put(&mut self, text: &str) -> Result<(), String> {
        clipboard::set_text(text)
    }

    /// Tests keep what was copied instead of touching the clipboard.
    #[cfg(test)]
    fn put(&mut self, text: &str) -> Result<(), String> {
        self.copied.push(text.to_string());
        Ok(())
    }
}

fn capture(buf: &Buffer) -> Vec<Vec<String>> {
    let area = buf.area;
    let rows_n = area.y.saturating_add(area.height) as usize;
    let cols_n = area.x.saturating_add(area.width) as usize;
    let mut rows = vec![Vec::new(); rows_n];
    for y in area.y..area.y.saturating_add(area.height) {
        let mut row = vec![String::new(); cols_n];
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell(Position::new(x, y)) {
                row[x as usize] = cell.symbol().to_string();
            }
        }
        rows[y as usize] = row;
    }
    rows
}

fn paint(buf: &mut Buffer, sel: select::TextSel) {
    let (start, end) = select::range(sel.anchor, sel.head);
    let area = buf.area;
    let last_x = area.x.saturating_add(area.width).saturating_sub(1);
    let style = Style::default().fg(Color::White).bg(Color::Blue);
    for y in start.1..=end.1 {
        if y < area.y || y >= area.y.saturating_add(area.height) {
            continue;
        }
        let x0 = if y == start.1 { start.0 } else { area.x };
        let x1 = if y == end.1 { end.0 } else { last_x };
        if x1 < x0 {
            continue;
        }
        for x in x0..=x1 {
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// A selection over a screen that says "hello world".
    fn drawn() -> (Selection, Buffer) {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        buf.set_string(0, 0, "hello world", Style::default());
        let mut selection = Selection::default();
        selection.finish(&mut buf);
        (selection, buf)
    }

    #[test]
    fn a_drag_selects_and_releasing_copies() {
        let (mut selection, mut buf) = drawn();
        let left = MouseButton::Left;
        let mut down = mouse(MouseEventKind::Down(left), 0, 0);
        assert_eq!(selection.on_mouse(&mut down), Pass::On);
        let mut drag = mouse(MouseEventKind::Drag(left), 4, 0);
        assert_eq!(selection.on_mouse(&mut drag), Pass::Swallow);
        assert_eq!(selection.note(), Some(("release to copy", false)));

        // The selected cells are marked on the next frame.
        selection.finish(&mut buf);
        assert_eq!(buf.cell((2, 0)).unwrap().bg, Color::Blue);
        assert_ne!(buf.cell((6, 0)).unwrap().bg, Color::Blue);

        let mut up = mouse(MouseEventKind::Up(left), 4, 0);
        assert_eq!(selection.on_mouse(&mut up), Pass::Swallow);
        assert_eq!(selection.copied, ["hello"]);
        assert_eq!(selection.note(), Some(("Copied to the clipboard", false)));
    }

    #[test]
    fn a_press_and_release_in_one_place_is_a_click_for_the_tool() {
        let (mut selection, _) = drawn();
        let left = MouseButton::Left;
        let mut down = mouse(MouseEventKind::Down(left), 5, 1);
        assert_eq!(selection.on_mouse(&mut down), Pass::On);
        // A slip of one column is still a click, at the place of the press.
        let mut up = mouse(MouseEventKind::Up(left), 6, 1);
        assert_eq!(selection.on_mouse(&mut up), Pass::On);
        assert_eq!((up.column, up.row), (5, 1));
        assert!(selection.copied.is_empty());
        assert_eq!(selection.note(), None);
    }

    #[test]
    fn ctrl_c_copies_a_selection_and_is_left_to_the_tool_without_one() {
        let (mut selection, _) = drawn();
        let ctrl_c = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(selection.on_key(ctrl_c), Pass::On);

        let left = MouseButton::Left;
        selection.on_mouse(&mut mouse(MouseEventKind::Down(left), 6, 0));
        selection.on_mouse(&mut mouse(MouseEventKind::Drag(left), 10, 0));
        assert_eq!(selection.on_key(ctrl_c), Pass::Swallow);
        assert_eq!(selection.copied, ["world"]);
    }

    #[test]
    fn esc_drops_a_selection_and_another_key_drops_it_and_goes_on() {
        let (mut selection, _) = drawn();
        let left = MouseButton::Left;
        let esc = key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(selection.on_key(esc), Pass::On);

        selection.on_mouse(&mut mouse(MouseEventKind::Down(left), 0, 0));
        selection.on_mouse(&mut mouse(MouseEventKind::Drag(left), 4, 0));
        assert_eq!(selection.on_key(esc), Pass::Swallow);
        assert_eq!(selection.note(), None);

        selection.on_mouse(&mut mouse(MouseEventKind::Down(left), 0, 0));
        selection.on_mouse(&mut mouse(MouseEventKind::Drag(left), 4, 0));
        let j = key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(selection.on_key(j), Pass::On);
        assert_eq!(selection.note(), None);
    }

    #[test]
    fn the_wheel_drops_a_selection_and_still_scrolls() {
        let (mut selection, _) = drawn();
        let left = MouseButton::Left;
        selection.on_mouse(&mut mouse(MouseEventKind::Down(left), 0, 0));
        selection.on_mouse(&mut mouse(MouseEventKind::Drag(left), 4, 0));
        let mut wheel = mouse(MouseEventKind::ScrollDown, 0, 0);
        assert_eq!(selection.on_mouse(&mut wheel), Pass::On);
        assert_eq!(selection.note(), None);
    }
}
