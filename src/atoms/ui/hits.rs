//! Where things were drawn, so a click can be matched to them.

use ratatui::layout::Rect;

/// What the last frame drew that can be clicked. `A` is what a button does.
pub struct Hits<A> {
    /// A row and the name it goes by.
    pub rows: Vec<(Rect, String)>,
    pub buttons: Vec<(Rect, A)>,
}

impl<A> Default for Hits<A> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            buttons: Vec::new(),
        }
    }
}

impl<A: Copy> Hits<A> {
    /// Forgets the last frame. Call before drawing the next one.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.buttons.clear();
    }

    /// What the button at this cell does.
    pub fn button_at(&self, x: u16, y: u16) -> Option<A> {
        self.buttons
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, action)| *action)
    }

    /// The name of the row at this cell.
    pub fn row_at(&self, x: u16, y: u16) -> Option<String> {
        self.rows
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, name)| name.clone())
    }
}

/// Whether the cell is inside `rect`.
pub fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_takes_the_first_cell_and_not_the_one_past_the_last() {
        let rect = Rect::new(2, 3, 4, 1);
        assert!(contains(rect, 2, 3));
        assert!(contains(rect, 5, 3));
        assert!(!contains(rect, 6, 3));
        assert!(!contains(rect, 2, 4));
        assert!(!contains(rect, 1, 3));
    }

    #[test]
    fn a_click_finds_the_button_and_the_row_under_it() {
        let mut hits: Hits<u8> = Hits::default();
        hits.buttons.push((Rect::new(0, 0, 6, 1), 1));
        hits.buttons.push((Rect::new(7, 0, 8, 1), 2));
        hits.rows.push((Rect::new(0, 2, 20, 1), "api".to_string()));
        assert_eq!(hits.button_at(8, 0), Some(2));
        assert_eq!(hits.button_at(6, 0), None);
        assert_eq!(hits.row_at(19, 2).as_deref(), Some("api"));
        assert_eq!(hits.row_at(0, 1), None);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut hits: Hits<u8> = Hits::default();
        hits.buttons.push((Rect::new(0, 0, 6, 1), 1));
        hits.rows.push((Rect::new(0, 2, 20, 1), "api".to_string()));
        hits.clear();
        assert_eq!(hits.button_at(0, 0), None);
        assert_eq!(hits.row_at(0, 2), None);
    }
}
