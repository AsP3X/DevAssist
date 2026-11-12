# UI Atoms (Drawing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract DevAssist's stateless drawing helpers (text fitting, hit testing, progress marks, dialog frame and buttons) into `src/atoms/ui/` with no change in what is drawn.

**Architecture:** This is the first half of step 3 in `docs/superpowers/specs/2026-11-12-atom-module-split-design.md`. These four atoms are plain functions and one small generic struct; none holds dialog state. `ui.rs` imports them under their old names where it can, so its roughly 120 call sites stay as they are. The stateful atoms (input, list, menu, confirm, path prompt) change how `App` stores state and handles keys, and get their own plan after this one.

**Tech Stack:** Rust 2024 (MSRV 1.88), ratatui 0.30, unicode-width 0.2.

---

## Execution notes (2026-11-12)

Executed on branch `refactor/ui-atoms-drawing`; final result `161 passed; 0 failed; 1 ignored`, `ui.rs` at 2,802 lines.

- **Task 2:** `use ratatui::layout::Rect;` in `src/app.rs` became unused and was removed.
- **Task 4:** in `draw_group` and `draw_path` the title is already a `&str`, so it is passed as `title`, not `&title`.
- **Not done:** the look at the colours in Task 5 Step 5. The replacement was mechanical and kept each dialog's colour argument, but no test reads colour.

## Why step 3 is split in two

The spec lists ten UI atoms. They fall into two groups with very different risk:

| Group | Atoms | What extraction touches |
|---|---|---|
| **Drawing (this plan)** | `text`, `hits`, `progress`, `dialog` | Pure functions move out of `ui.rs`. No state moves. The 40 drawing tests in `ui.rs` check the result on screen. |
| **Stateful (next plan)** | `input`, `list`, `menu`, `confirm`, `path` | Fields leave `App`, key handling leaves `App::on_event`, each dialog is rebuilt on an atom. |

Two names differ from the spec, which is updated to match:

- `table` is called `text`: it holds `fit`, `fit_end`, `padded_row` and `join_facts`. `column_widths` stays in `ui.rs` because its six columns (marker, name, branch, state, sync, about) are the repository table's own.
- `align_scroll` waits for the `list` atom in the next plan.

## Ground rules for every task

- Baseline: `cargo test --locked` → `148 passed; 0 failed; 1 ignored` (master at v0.3.3).
- Before each commit all three must pass:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
- This is a binary crate: an unused `pub` item is a `dead_code` error under `-D warnings`. Each task adds an atom and switches its callers together.
- Line numbers refer to master at v0.3.3. Locate code by name once they drift.
- The drawing tests in `src/ui.rs` (`mod tests`, from line 2394) render whole screens and assert on the text. They are the proof that nothing on screen changed; none of them should need an edit.

## File map

| File | Responsibility | Task |
|---|---|---|
| `src/atoms/ui/mod.rs` (new) | declares the four atoms | 1–4 |
| `src/atoms/ui/text.rs` (new) | `fit`, `fit_end`, `padded_row`, `join_facts` | 1 |
| `src/atoms/ui/hits.rs` (new) | `Hits<A>`, `contains` | 2 |
| `src/atoms/ui/progress.rs` (new) | `spinner`, `icon`, `bar` | 3 |
| `src/atoms/ui/dialog.rs` (new) | `centered`, `window`, `buttons` | 4 |
| `src/ui.rs`, `src/app.rs` | callers switched | 1–4 |

---

### Task 0: Branch

- [ ] **Step 1: Create the work branch**

```bash
git switch master
git switch -c refactor/ui-atoms-drawing
```

- [ ] **Step 2: Confirm the baseline**

Run: `cargo test --locked`
Expected: `test result: ok. 148 passed; 0 failed; 1 ignored`

---

### Task 1: `text` atom

**Files:**
- Create: `src/atoms/ui/mod.rs`, `src/atoms/ui/text.rs`
- Modify: `src/atoms/mod.rs`, `src/ui.rs:7,2086-2093,2296-2305,2341-2392`

- [ ] **Step 1: Declare the module and write the failing tests**

`src/atoms/mod.rs` becomes:

```rust
//! Reusable building blocks. Nothing in here names a feature type.

pub mod sys;
pub mod task;
pub mod ui;
```

`src/atoms/ui/mod.rs`:

```rust
//! Atoms that draw, or that know where something was drawn.

pub mod text;
```

`src/atoms/ui/text.rs`:

```rust
//! Fitting text into a number of terminal cells.

use unicode_width::UnicodeWidthStr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_short_text_and_cuts_long_text() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 1), "…");
        assert_eq!(fit("abc", 0), "");
        // A wide character that does not fit leaves a padded gap.
        assert_eq!(fit("中文字", 4), "中… ");
    }

    #[test]
    fn fit_end_keeps_the_end_and_does_not_pad() {
        assert_eq!(fit_end("ab", 4), "ab");
        assert_eq!(fit_end("abcdef", 4), "…def");
        assert_eq!(fit_end("abc", 1), "…");
        assert_eq!(fit_end("abc", 0), "");
    }

    #[test]
    fn padded_row_skips_columns_of_no_width() {
        assert_eq!(padded_row(&["a", "bb", "c"], &[2, 0, 3], 6), "a c   ");
        assert_eq!(padded_row(&["abcdef", "x"], &[4, 2], 6), "abc…x ");
    }

    #[test]
    fn join_facts_leaves_no_separator_behind() {
        assert_eq!(join_facts("a", "b"), "a  ·  b");
        assert_eq!(join_facts("a", ""), "a");
        assert_eq!(join_facts("", "b"), "b");
        assert_eq!(join_facts("", ""), "");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::text`
Expected: compile errors `cannot find function fit`, `fit_end`, `padded_row`, `join_facts`

- [ ] **Step 3: Move the four functions**

Cut `join_facts` (with its doc comment), `padded_row`, `fit_end` and `fit` from `src/ui.rs` and paste them into `src/atoms/ui/text.rs` between the `use` line and the `tests` module. Make each `pub`. The bodies do not change, with one exception: `padded_row` takes any number of columns.

```rust
/// Each cell fitted to its column and the whole fitted to `total`. A
/// column of width 0 is left out.
pub fn padded_row(cells: &[&str], widths: &[usize], total: usize) -> String {
```

(The old parameter was `widths: &[usize; 6]`. The two callers pass `&widths` where `widths` is a `[usize; 6]`, which still fits.)

Give the other three a doc comment each:

```rust
/// Two facts on one line. An empty one leaves no separator behind.
pub fn join_facts(first: &str, second: &str) -> String {
```

```rust
/// `text` in at most `width` cells, keeping its end. Not padded.
pub fn fit_end(text: &str, width: usize) -> String {
```

```rust
/// `text` in exactly `width` cells: padded with spaces, or cut with "…".
pub fn fit(text: &str, width: usize) -> String {
```

- [ ] **Step 4: Import them in `ui.rs` under the same names**

In `src/ui.rs`, add next to the other `use crate::…` lines:

```rust
use crate::atoms::ui::text::{fit, fit_end, join_facts, padded_row};
```

`use unicode_width::UnicodeWidthStr;` (line 7) stays: `draw_menu` still measures its labels.

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `152 passed; 0 failed; 1 ignored`

- [ ] **Step 6: Commit**

```bash
git add -A src
git commit -m "refactor: move text fitting to atoms/ui/text"
```

---

### Task 2: `hits` atom

`contains` exists twice (`src/ui.rs:2334`, `src/app.rs:3233`). `Hits` records where rows and buttons were drawn; it is tied to the app's `Action` type today and becomes generic.

**Files:**
- Create: `src/atoms/ui/hits.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/app.rs:86-90,345,1845-1860,3233-3238`, `src/ui.rs:21-22,2334-2339`

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod hits;` to `src/atoms/ui/mod.rs` (alphabetical: `hits`, `text`).

`src/atoms/ui/hits.rs`:

```rust
//! Where things were drawn, so a click can be matched to them.

use ratatui::layout::Rect;

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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::hits`
Expected: compile errors `cannot find type Hits`, `cannot find function contains`

- [ ] **Step 3: Write the atom**

Insert between the `use` line and the `tests` module:

```rust
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
```

`Default` is written by hand because `#[derive(Default)]` would demand `A: Default`.

- [ ] **Step 4: Switch `app.rs`**

Delete the `Hits` struct with its `#[derive(Default)]` (lines 86–90) and the private `fn contains` (lines 3233–3238). Add next to the other `use crate::…` lines:

```rust
use crate::atoms::ui::hits::Hits;
```

Change the field (line 345):

```rust
    pub hits: Hits<Action>,
```

Replace the body of `click`:

```rust
    fn click(&mut self, x: u16, y: u16) {
        if let Some(action) = self.hits.button_at(x, y) {
            self.run(action);
            return;
        }
        if let Some(name) = self.hits.row_at(x, y) {
            self.click_row(name);
        }
    }
```

The order is the same as before: buttons first, then rows, the first match wins.

- [ ] **Step 5: Switch `ui.rs`**

Delete its private `fn contains` (lines 2334–2339). Add:

```rust
use crate::atoms::ui::hits::contains;
```

At the top of `draw` (lines 21–22), replace

```rust
    app.hits.rows.clear();
    app.hits.buttons.clear();
```

with

```rust
    app.hits.clear();
```

The lines that push (`app.hits.rows.push(…)`, `app.hits.buttons.push(…)`) stay as they are.

- [ ] **Step 6: Check for leftovers**

Run: `grep -n 'fn contains' src/*.rs`
Expected: no output

- [ ] **Step 7: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `155 passed; 0 failed; 1 ignored`. If clippy reports an import in `src/app.rs` as unused after this (for example `Rect`), remove that import.

- [ ] **Step 8: Commit**

```bash
git add -A src
git commit -m "refactor: add generic Hits atom and remove duplicate contains"
```

---

### Task 3: `progress` atom

**Files:**
- Create: `src/atoms/ui/progress.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/ui.rs:17,393,588,927,980,987,1695,1739,1838,1930,2005,2095-2130`

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod progress;` to `src/atoms/ui/mod.rs` (alphabetical: `hits`, `progress`, `text`).

`src/atoms/ui/progress.rs`:

```rust
//! Marks for work that is running: a spinner, a state icon, a bar.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::atoms::task::job::ItemState;

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(spans: &[Span]) -> Vec<usize> {
        spans
            .iter()
            .map(|span| span.content.chars().count())
            .collect()
    }

    #[test]
    fn the_spinner_turns_every_second_tick_and_comes_round() {
        assert_eq!(spinner(0), "⠋");
        assert_eq!(spinner(1), "⠋");
        assert_eq!(spinner(2), "⠙");
        assert_eq!(spinner(20), "⠋");
    }

    #[test]
    fn every_state_has_its_icon() {
        assert_eq!(icon(ItemState::Waiting, 0).0, "·");
        assert_eq!(icon(ItemState::Done, 0).0, "✓");
        assert_eq!(icon(ItemState::Failed, 0).0, "✗");
        assert_eq!(icon(ItemState::Running, 2).0, spinner(2));
    }

    #[test]
    fn a_bar_fills_to_its_fraction_or_moves_a_block() {
        assert_eq!(lengths(&bar(10, Some(0.5), Color::Cyan, 0)), [0, 5, 5]);
        assert_eq!(lengths(&bar(10, Some(7.0), Color::Cyan, 0)), [0, 10, 0]);
        // No width still draws one cell.
        assert_eq!(lengths(&bar(0, Some(1.0), Color::Cyan, 0)), [0, 1, 0]);
        // Without a fraction a block of a fifth goes there and back.
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 0)), [0, 2, 8]);
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 8)), [8, 2, 0]);
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 12)), [4, 2, 4]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::progress`
Expected: compile errors `cannot find function spinner`, `icon`, `bar`

- [ ] **Step 3: Move the code**

Cut the `SPINNER` constant (line 17), `item_icon` and `bar_spans` (with its doc comment) from `src/ui.rs` and paste them into `src/atoms/ui/progress.rs` between the `use` lines and the `tests` module, renamed, with a new `spinner` function:

```rust
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The spinner's frame at this tick. It turns every second tick.
pub fn spinner(tick: usize) -> &'static str {
    SPINNER[(tick / 2) % SPINNER.len()]
}

/// The mark and colour for an item in this state.
pub fn icon(state: ItemState, tick: usize) -> (&'static str, Style) {
    match state {
        ItemState::Waiting => ("·", Style::default().fg(Color::DarkGray)),
        ItemState::Running => (spinner(tick), Style::default().fg(Color::Cyan)),
        ItemState::Done => ("✓", Style::default().fg(Color::Green)),
        ItemState::Failed => ("✗", Style::default().fg(Color::Red)),
    }
}

/// A line of `width` cells, lit up to `fraction`. Without a fraction a
/// short block moves back and forth.
pub fn bar(width: usize, fraction: Option<f64>, color: Color, tick: usize) -> Vec<Span<'static>> {
    let width = width.max(1);
    let (from, to) = match fraction {
        Some(fraction) => (
            0,
            (fraction.clamp(0.0, 1.0) * width as f64).round() as usize,
        ),
        None => {
            let block = (width / 5).clamp(1, 12);
            let span = width - block;
            let step = if span == 0 { 0 } else { tick % (span * 2) };
            let at = if step > span { span * 2 - step } else { step };
            (at, at + block)
        }
    };
    let dim = Style::default().fg(Color::DarkGray);
    vec![
        Span::styled("━".repeat(from), dim),
        Span::styled("━".repeat(to - from), Style::default().fg(color)),
        Span::styled("━".repeat(width - to), dim),
    ]
}
```

- [ ] **Step 4: Switch the callers in `ui.rs`**

Run (Git Bash):

```bash
sed -i -E 's/SPINNER\[\(([^)]*) \/ 2\) % SPINNER\.len\(\)\]/progress::spinner(\1)/g; s/\bbar_spans\(/progress::bar(/g; s/\bitem_icon\(/progress::icon(/g' src/ui.rs
```

Add next to the other `use crate::…` lines:

```rust
use crate::atoms::ui::progress;
```

Run: `grep -nE 'SPINNER|bar_spans|item_icon' src/ui.rs`
Expected: no output. (Five spinner uses, three bars and two icons were replaced.)

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `158 passed; 0 failed; 1 ignored`

- [ ] **Step 6: Commit**

```bash
git add -A src
git commit -m "refactor: move spinner, icon and bar to atoms/ui/progress"
```

---

### Task 4: `dialog` atom

Nine dialogs build the same rounded, titled, cleared box by hand, sixteen lines each. The atom draws it in one call. `draw_dialog_buttons` moves too, made generic; `ui.rs` keeps a three-line wrapper so its nine callers do not change.

**Files:**
- Create: `src/atoms/ui/dialog.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/ui.rs:6` and the nine dialogs at lines 553, 785, 901, 1052, 1178, 1318, 1405, 1639, 2138, plus `draw_dialog_buttons` (2197–2234) and `centered` (2323–2332)

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod dialog;` to `src/atoms/ui/mod.rs` (alphabetical: `dialog`, `hits`, `progress`, `text`).

`src/atoms/ui/dialog.rs`:

```rust
//! The box a dialog is drawn in, and the row of buttons under it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::hits::{Hits, contains};

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn centered_sits_in_the_middle_and_stays_inside() {
        let screen = Rect::new(0, 0, 80, 24);
        assert_eq!(centered(screen, 40, 10), Rect::new(20, 7, 40, 10));
        // Never smaller than 8 by 4, never larger than the screen.
        assert_eq!(centered(screen, 2, 1), Rect::new(36, 10, 8, 4));
        assert_eq!(
            centered(Rect::new(0, 0, 6, 3), 40, 10),
            Rect::new(0, 0, 6, 3)
        );
    }

    #[test]
    fn a_window_has_a_rounded_titled_border_and_hands_back_its_inside() {
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        let mut inner = Rect::default();
        terminal
            .draw(|frame| {
                inner = window(frame, Rect::new(0, 0, 20, 5), " Keys ", Color::Cyan);
            })
            .unwrap();
        assert_eq!(inner, Rect::new(1, 1, 18, 3));
        assert!(row(&terminal, 0).starts_with("╭ Keys ─"));
        assert!(row(&terminal, 4).starts_with("╰─"));
    }

    #[test]
    fn buttons_are_drawn_left_to_right_and_remembered() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
        let mut hits: Hits<u8> = Hits::default();
        let mut end = 0;
        terminal
            .draw(|frame| {
                end = buttons(
                    frame,
                    Rect::new(0, 0, 20, 1),
                    &[("Save", 1), ("Cancel", 2)],
                    1,
                    None,
                    &mut hits,
                );
            })
            .unwrap();
        assert_eq!(row(&terminal, 0), " Save   Cancel      ");
        assert_eq!(end, 16);
        assert_eq!(
            hits.buttons,
            vec![(Rect::new(0, 0, 6, 1), 1), (Rect::new(7, 0, 8, 1), 2)]
        );

        // A button that does not fit is left out, with the ones after it.
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| {
                buttons(
                    frame,
                    Rect::new(0, 0, 10, 1),
                    &[("Save", 1), ("Cancel", 2)],
                    1,
                    None,
                    &mut hits,
                );
            })
            .unwrap();
        assert_eq!(hits.buttons.len(), 1);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::dialog`
Expected: compile errors `cannot find function centered`, `window`, `buttons`

- [ ] **Step 3: Write the atom**

Cut `centered` from `src/ui.rs` (lines 2323–2332) and insert it, with the two new functions, between the `use` lines and the `tests` module:

```rust
/// A box of this size in the middle of `area`: at least 8 by 4, and never
/// larger than the area.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(8).min(area.width);
    let height = height.max(4).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Clears `popup`, draws a rounded border with `title` in the `accent`
/// colour, and returns the area inside the border.
pub fn window(frame: &mut Frame, popup: Rect, title: &str, accent: Color) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    inner
}

/// Draws the buttons from the left of `area`, records them in `hits`, and
/// returns the column after the last one. The button that does `primary`
/// stands out; the one under `mouse` is lit.
pub fn buttons<A: Copy + PartialEq>(
    frame: &mut Frame,
    area: Rect,
    buttons: &[(&str, A)],
    primary: A,
    mouse: Option<(u16, u16)>,
    hits: &mut Hits<A>,
) -> u16 {
    let mut x = area.x;
    for (label, action) in buttons {
        let text = format!(" {label} ");
        let width = text.len() as u16;
        if x + width > area.x + area.width {
            break;
        }
        let rect = Rect {
            x,
            y: area.y,
            width,
            height: 1,
        };
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if hovered {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if *action == primary {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        hits.buttons.push((rect, *action));
        x += width + 1;
    }
    x
}
```

- [ ] **Step 4: Run the atom's tests**

Run: `cargo test --locked atoms::ui::dialog`
Expected: 3 passed. (Clippy still fails here: `window` and `buttons` have no caller yet.)

- [ ] **Step 5: Reduce `draw_dialog_buttons` to a wrapper**

In `src/ui.rs`, add:

```rust
use crate::atoms::ui::dialog::{self, centered};
```

Replace the whole of `draw_dialog_buttons` (lines 2197–2234) with:

```rust
/// Draws the buttons and returns the column after the last one.
fn draw_dialog_buttons(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    buttons: &[(&str, Action)],
) -> u16 {
    dialog::buttons(
        frame,
        area,
        buttons,
        Action::Confirm,
        app.mouse,
        &mut app.hits,
    )
}
```

Its nine callers do not change.

- [ ] **Step 6: Replace the nine hand-built boxes**

Each dialog has this shape, differing only in the title expression and the colour:

```rust
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOUR))
        .title(Span::styled(
            TITLE,
            Style::default().fg(COLOUR).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
```

Replace everything from `let block = Block::default()` through `frame.render_widget(block, popup);` with the line given here:

| Function | Line | Replacement |
|---|---|---|
| `draw_add` | 553 | `let inner = dialog::window(frame, popup, &format!(" Add from {} ", add_source(app)), Color::Cyan);` |
| `draw_confirm` | 785 | `let inner = dialog::window(frame, popup, &title, Color::Yellow);` |
| `draw_update` | 901 | `let inner = dialog::window(frame, popup, &format!(" DevAssist {} ", crate::update::VERSION), accent);` |
| `draw_menu` | 1052 | `let inner = dialog::window(frame, popup, &fit_end(&title, popup.width.saturating_sub(4) as usize), Color::Cyan);` |
| `draw_group` | 1178 | `let inner = dialog::window(frame, popup, &title, Color::Cyan);` |
| `draw_path` | 1318 | `let inner = dialog::window(frame, popup, &title, Color::Cyan);` |
| `draw_settings` | 1405 | `let inner = dialog::window(frame, popup, heading, Color::Cyan);` |
| `draw_help` | 2138 | `let inner = dialog::window(frame, popup, " Keys ", Color::Cyan);` |

`draw_progress` (line 1639) has a `match` as its title; give it a name first:

```rust
    let heading = match queued {
        0 => format!(" {} ", job.title),
        _ => format!(" {} · {queued} queued ", job.title),
    };
    let inner = dialog::window(frame, popup, &heading, accent);
```

Do **not** touch the two `let pane = Block::default()` boxes inside `draw_progress` (lines 1816 and 1910). They are panes inside a dialog, not dialogs: they are not cleared and their titles are not bold.

In the import on line 6, `Clear` is no longer used; remove it:

```rust
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
```

- [ ] **Step 7: Check for leftovers**

Run: `grep -nE 'let block = Block::default|render_widget\(Clear|fn centered' src/ui.rs`
Expected: no output

- [ ] **Step 8: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `161 passed; 0 failed; 1 ignored`. The drawing tests assert on every dialog's top border (`╭ Clone into `, `╭ Move to a group `, `╭ docs `, `╭ Cloning 3 repositories · 3 queued `, …) and on every button row (` Delete   Forget   Cancel `, ` Install   Later `, …), so a wrong title, colour-independent layout or button order fails here.

- [ ] **Step 9: Commit**

```bash
git add -A src
git commit -m "refactor: draw dialog frames and buttons through atoms/ui/dialog"
```

---

### Task 5: Final check

- [ ] **Step 1: Confirm the atoms name no feature**

Run: `grep -rnE 'gitops|github|gitprogress|config::|LocalRepo|RemoteRepo|crate::app|crate::ui|Action\b' src/atoms`
Expected: no output

- [ ] **Step 2: Confirm the layout**

Run: `ls src/atoms/ui`
Expected: `dialog.rs  hits.rs  mod.rs  progress.rs  text.rs`

- [ ] **Step 3: Confirm `ui.rs` shrank**

Run: `wc -l src/ui.rs`
Expected: about 2,800 lines, down from 3,034 (nine hand-built boxes became one call each; eight helper functions left).

- [ ] **Step 4: Full CI run**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: all three pass; `161 passed; 0 failed; 1 ignored`

- [ ] **Step 5: Look at the colours**

The drawing tests read text, not colour. Run `cargo run` and open each dialog once (keys are in `docs/USAGE.md`): Add, the actions menu, Move to a group, Settings and Help should have a cyan border and bold cyan title; Remove should be yellow; the version dialog is cyan, green when an update is available, red after a failure. Hovering a button should light it black on cyan.

No changelog entry: nothing a user can see has changed.
