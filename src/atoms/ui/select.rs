//! Screen text selection. A drag copies the cells the user can see.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSel {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub origin: Option<(u16, u16)>,
    pub selection: Option<TextSel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEvent {
    Down,
    Drag,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEffect {
    None,
    Click(u16, u16),
    Copy,
}

impl Pointer {
    pub fn on(&mut self, event: PointerEvent, at: (u16, u16)) -> PointerEffect {
        match event {
            PointerEvent::Down => {
                self.origin = Some(at);
                self.selection = None;
                PointerEffect::None
            }
            PointerEvent::Drag => {
                self.extend(at);
                PointerEffect::None
            }
            PointerEvent::Up => {
                let Some(origin) = self.origin else {
                    return PointerEffect::None;
                };
                let selecting = self.extend(at);
                self.origin = None;
                if selecting {
                    PointerEffect::Copy
                } else {
                    self.selection = None;
                    PointerEffect::Click(origin.0, origin.1)
                }
            }
        }
    }

    /// Keeps a selection once the pointer has moved enough to be a drag.
    /// A one-column slip is still a click, so buttons keep working.
    fn extend(&mut self, at: (u16, u16)) -> bool {
        let Some(origin) = self.origin else {
            return self.selection.is_some();
        };
        if self.selection.is_some() || far(origin, at) {
            self.selection = Some(TextSel {
                anchor: origin,
                head: at,
            });
        }
        self.selection.is_some()
    }
}

fn far(origin: (u16, u16), at: (u16, u16)) -> bool {
    origin.1.abs_diff(at.1) >= 1 || origin.0.abs_diff(at.0) >= 2
}

pub fn range(anchor: (u16, u16), head: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    if anchor.1 < head.1 || (anchor.1 == head.1 && anchor.0 <= head.0) {
        (anchor, head)
    } else {
        (head, anchor)
    }
}

/// Text covered by a selection. Each entry of `rows` is one screen row of
/// cell symbols. An empty string is the tail of a wide character.
pub fn selected_text(rows: &[Vec<String>], anchor: (u16, u16), head: (u16, u16)) -> String {
    let (start, end) = range(anchor, head);
    let mut lines = Vec::new();
    for y in start.1..=end.1 {
        let row = rows.get(y as usize);
        let x0 = if y == start.1 { start.0 } else { 0 };
        let x1 = if y == end.1 { end.0 } else { u16::MAX };
        let line = row
            .map(|cells| slice_row(cells, x0, x1))
            .unwrap_or_default();
        lines.push(line.trim_end().to_string());
    }
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        String::new()
    } else {
        text
    }
}

fn slice_row(row: &[String], x0: u16, x1: u16) -> String {
    let mut out = String::new();
    let mut x = 0u16;
    while (x as usize) < row.len() {
        let sym = &row[x as usize];
        if sym.is_empty() {
            x += 1;
            continue;
        }
        let width = unicode_width::UnicodeWidthStr::width(sym.as_str()).max(1) as u16;
        let last = x.saturating_add(width).saturating_sub(1);
        if last >= x0 && x <= x1 {
            out.push_str(sym);
        }
        x = x.saturating_add(width);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Pointer, PointerEffect, PointerEvent, TextSel, selected_text};

    fn grid(lines: &[&str]) -> Vec<Vec<String>> {
        lines
            .iter()
            .map(|line| line.chars().map(|ch| ch.to_string()).collect())
            .collect()
    }

    #[test]
    fn copies_the_dragged_span_and_trims_padding() {
        let rows = grid(&["name  main   "]);
        assert_eq!(selected_text(&rows, (0, 0), (8, 0)), "name  mai");
    }

    #[test]
    fn copies_lines_in_reading_order_when_the_drag_goes_up() {
        let rows = grid(&["abcdef", "ghijkl"]);
        assert_eq!(selected_text(&rows, (2, 1), (4, 0)), "ef\nghi");
    }

    #[test]
    fn keeps_internal_blank_lines_and_drops_blank_edges() {
        let rows = grid(&["   ", "alpha", " ", "beta", "    "]);
        assert_eq!(selected_text(&rows, (0, 0), (3, 4)), "alpha\n\nbeta");
    }

    #[test]
    fn includes_a_wide_character_once() {
        let rows = vec![vec![
            "A".to_string(),
            "中".to_string(),
            String::new(),
            "B".to_string(),
        ]];
        assert_eq!(selected_text(&rows, (1, 0), (2, 0)), "中");
        assert_eq!(selected_text(&rows, (0, 0), (3, 0)), "A中B");
        assert_eq!(selected_text(&rows, (2, 0), (3, 0)), "中B");
    }

    #[test]
    fn a_click_or_a_one_column_slip_does_not_select() {
        let mut pointer = Pointer::default();
        assert_eq!(pointer.on(PointerEvent::Down, (3, 4)), PointerEffect::None);
        assert_eq!(pointer.on(PointerEvent::Drag, (4, 4)), PointerEffect::None);
        assert!(pointer.selection.is_none());
        assert_eq!(
            pointer.on(PointerEvent::Up, (4, 4)),
            PointerEffect::Click(3, 4)
        );
    }

    #[test]
    fn dragging_selects_and_copies_on_release() {
        let mut pointer = Pointer::default();
        pointer.on(PointerEvent::Down, (1, 2));
        pointer.on(PointerEvent::Drag, (6, 2));
        assert_eq!(
            pointer.selection,
            Some(TextSel {
                anchor: (1, 2),
                head: (6, 2)
            })
        );
        assert_eq!(pointer.on(PointerEvent::Up, (8, 4)), PointerEffect::Copy);
        assert_eq!(
            pointer.selection,
            Some(TextSel {
                anchor: (1, 2),
                head: (8, 4)
            })
        );
    }

    #[test]
    fn a_release_on_another_row_copies_without_a_drag_event() {
        let mut pointer = Pointer::default();
        pointer.on(PointerEvent::Down, (2, 1));
        assert_eq!(pointer.on(PointerEvent::Up, (2, 3)), PointerEffect::Copy);
    }
}
