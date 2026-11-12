# UI Atoms (Stateful) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the reusable behaviour behind DevAssist's text fields, lists, keyed menu and folder prompt into `src/atoms/ui/`, so a new feature gets them without copying key handling.

**Architecture:** This is the second half of step 3 in `docs/superpowers/specs/2026-11-12-atom-module-split-design.md`. Three of the four atoms (`input`, `list`, `menu`) are functions that work on state the caller owns (`&mut String`, `&mut Option<String>`, `&mut usize`), because that state is a plain value that `app.rs`, `ui.rs` and about 60 test lines read and assign directly. The fourth (`path`) is today's `pathprompt.rs`, made generic so it no longer names `RemoteRepo`.

**Tech Stack:** Rust 2024 (MSRV 1.88), ratatui 0.30 (crossterm events), unicode-width 0.2.

---

## Execution notes (2026-11-12)

Executed on branch `refactor/ui-atoms-stateful` as written; final result `175 passed; 0 failed; 1 ignored`, clippy and format clean at every commit, `--doctor` exit 0.

- **Not done:** trying the keys by hand (Task 5 Step 5). The two intended behaviour changes are covered by the `input` atom's tests, not by a test that drives the group dialog or the filter.

## What reading the code changed

1. **Atoms as functions, not owning structs.** The spec said each UI atom "owns its state". For a text field the state is one `String`, and there is no cursor: text is only added and removed at the end. Wrapping `list_filter`, `add_filter`, `selected` and `add_selected` in atom types would rewrite about 130 call sites and 60 test lines to gain nothing. The behaviour is what is duplicated, so the behaviour is what moves.
2. **No `confirm` atom.** `on_confirm_key` is six lines with a third choice (`f` forgets), and `draw_confirm` is 140 lines that list which repositories have uncommitted changes. Nothing generic is in there. The spec is updated.
3. **Text editing exists five times with different rules.** The table shows today's behaviour:

   | Field | Types a character when | Ctrl-U | Ctrl-W |
   |---|---|---|---|
   | List filter | no Ctrl, no Alt | `u` only | deletes a word |
   | Add search | no Ctrl, no Alt | `u` only | deletes a word |
   | Settings fields | no Ctrl, no Alt | `u` only | deletes a word |
   | Group name | no Ctrl (Alt types) | `u` or `U` | nothing |
   | Folder path | no Ctrl (Alt types) | `u` or `U` | deletes a path segment |

   The atom keeps the Alt difference as a parameter, because it may matter: on a German keyboard `\` and `@` need AltGr, and a path needs `\`.

### Behaviour that changes (small, intended)

- **Group dialog:** Ctrl-W deletes the last word. It did nothing before.
- **List filter, Add search, Settings:** Ctrl-Shift-U and Ctrl-Shift-W work like Ctrl-U and Ctrl-W.

Everything else is unchanged. These two changes get a changelog line when this is released.

### Quirks found and deliberately kept

Not fixed here; each is a decision for its own change:

- In the list filter, `j` and `k` move the selection instead of being typed, because the navigation keys are matched before typing. A filter such as "pumpkin" cannot be typed in full.
- In Add, Space ticks a repository and cannot be typed.

## Ground rules for every task

- Baseline: `cargo test --locked` → `162 passed; 0 failed; 1 ignored` (master at v0.3.4).
- Before each commit all three must pass:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
- Check clippy's exit code on its own. Do not pipe it into `tail` and trust the pipeline's status.
- This is a binary crate: an unused `pub` item is a `dead_code` error under `-D warnings`. Each task adds an atom and switches its callers together.
- Line numbers refer to master at v0.3.4. Locate code by name once they drift.

## File map

| File | Responsibility | Task |
|---|---|---|
| `src/atoms/ui/input.rs` (new) | `edit`, `Alt` | 1 |
| `src/atoms/ui/list.rs` (new) | `move_by`, `move_to_end`, `snap`, `align_scroll` | 2 |
| `src/atoms/ui/menu.rs` (new) | `on_key`, `Outcome`, `Entry`, `draw` | 3 |
| `src/atoms/ui/path.rs` (moved from `src/pathprompt.rs`) | `PathPrompt<P>`, `Outcome`, `resolve` | 4 |
| `src/app.rs`, `src/ui.rs`, `src/settings.rs`, `src/main.rs` | callers switched | 1–4 |

---

### Task 0: Branch

- [ ] **Step 1: Create the work branch**

```bash
git switch master
git switch -c refactor/ui-atoms-stateful
```

- [ ] **Step 2: Confirm the baseline**

Run: `cargo test --locked`
Expected: `test result: ok. 162 passed; 0 failed; 1 ignored`

---

### Task 1: `input` atom

**Files:**
- Create: `src/atoms/ui/input.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/app.rs` (`on_group_key` 1282–1320 and its call at 894, `on_filter_key` 1435–1471, `on_add_key` 1473–1503, `delete_word` 3196–3218 and its test 4312–4317), `src/settings.rs:102-134`, `src/pathprompt.rs:46-67`

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod input;` to `src/atoms/ui/mod.rs` (alphabetical: `dialog`, `hits`, `input`, `progress`, `text`).

`src/atoms/ui/input.rs`:

```rust
//! Typing into one line of text. Text is added and removed at its end;
//! there is no cursor to move.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::NONE)
    }

    #[test]
    fn characters_are_added_and_backspace_takes_the_last_one() {
        let mut text = String::new();
        assert!(edit(&mut text, plain(KeyCode::Char('a')), Alt::Ignored));
        assert!(edit(&mut text, key(KeyCode::Char('B'), KeyModifiers::SHIFT), Alt::Ignored));
        assert_eq!(text, "aB");
        assert!(edit(&mut text, plain(KeyCode::Backspace), Alt::Ignored));
        assert_eq!(text, "a");
        // Backspace on nothing is still an edit: the caller reacts to it.
        text.clear();
        assert!(edit(&mut text, plain(KeyCode::Backspace), Alt::Ignored));
        assert_eq!(text, "");
    }

    #[test]
    fn ctrl_u_clears_the_line_in_either_case() {
        for letter in ['u', 'U'] {
            let mut text = "hello world".to_string();
            assert!(edit(&mut text, key(KeyCode::Char(letter), KeyModifiers::CONTROL), Alt::Ignored));
            assert_eq!(text, "");
        }
    }

    #[test]
    fn ctrl_w_removes_the_last_word_and_the_space_before_it() {
        for letter in ['w', 'W'] {
            let mut text = "hello world".to_string();
            assert!(edit(&mut text, key(KeyCode::Char(letter), KeyModifiers::CONTROL), Alt::Ignored));
            assert_eq!(text, "hello");
        }
        let mut text = "one  two   ".to_string();
        edit(&mut text, key(KeyCode::Char('w'), KeyModifiers::CONTROL), Alt::Ignored);
        assert_eq!(text, "one");
    }

    #[test]
    fn alt_types_only_where_the_field_asks_for_it() {
        let alt = key(KeyCode::Char('\\'), KeyModifiers::ALT);
        let mut text = String::new();
        assert!(!edit(&mut text, alt, Alt::Ignored));
        assert_eq!(text, "");
        assert!(edit(&mut text, alt, Alt::Types));
        assert_eq!(text, "\\");
        // Ctrl never types, with or without Alt.
        let both = key(KeyCode::Char('x'), KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert!(!edit(&mut text, both, Alt::Types));
        assert_eq!(text, "\\");
    }

    #[test]
    fn other_keys_are_left_to_the_caller() {
        let mut text = "abc".to_string();
        for code in [KeyCode::Enter, KeyCode::Esc, KeyCode::Tab, KeyCode::Up, KeyCode::Left] {
            assert!(!edit(&mut text, plain(code), Alt::Types));
        }
        assert!(!edit(&mut text, key(KeyCode::Char('s'), KeyModifiers::CONTROL), Alt::Types));
        assert_eq!(text, "abc");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::input`
Expected: compile errors `cannot find function edit`, `cannot find type Alt`

- [ ] **Step 3: Write the atom**

Cut `delete_word` from `src/app.rs` (lines 3196–3218) and insert it, private, with the new code between the `use` line and the `tests` module:

```rust
/// What a character typed with Alt held does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Alt {
    /// It is typed. For fields that may need AltGr characters, such as `\`.
    Types,
    /// It is left to the caller.
    Ignored,
}

/// Applies an editing key to `text`. True when the key was an edit, even
/// one that changed nothing, such as Backspace on an empty line.
///
/// Backspace removes the last character, Ctrl-U clears the line, Ctrl-W
/// removes the last word, and a character typed without Ctrl is added.
pub fn edit(text: &mut String, key: KeyEvent, alt: Alt) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt_held = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('u' | 'U') if ctrl => text.clear(),
        KeyCode::Char('w' | 'W') if ctrl => delete_word(text),
        KeyCode::Char(ch) if !ctrl && (alt == Alt::Types || !alt_held) => text.push(ch),
        _ => return false,
    }
    true
}

fn delete_word(text: &mut String) {
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
}
```

In the `tests` module of `src/app.rs`, delete the test `delete_word_removes_the_last_word` (lines 4312–4317); `ctrl_w_removes_the_last_word_and_the_space_before_it` replaces it.

- [ ] **Step 4: Switch the list filter and the Add search (`src/app.rs`)**

Add next to the other `use crate::…` lines:

```rust
use crate::atoms::ui::input::{self, Alt};
```

In `on_filter_key`, delete the `KeyCode::Backspace`, `KeyCode::Char('u') if ctrl`, `KeyCode::Char('w') if ctrl` and `KeyCode::Char(ch) if …` arms, and replace the final `_ => {}` with the atom. The navigation arms stay, in their order, **before** it:

```rust
    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.list_filter.clear();
                self.list_scroll = 0;
            }
            KeyCode::Enter => self.enter_row(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            _ => {
                if input::edit(&mut self.list_filter, key, Alt::Ignored) {
                    self.list_scroll = 0;
                    self.snap_visible_selection();
                }
            }
        }
    }
```

`on_add_key` the same way:

```rust
    fn on_add_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => self.clone_selected(),
            KeyCode::Tab => self.list_typed_account(),
            KeyCode::Char(' ') if !ctrl => self.toggle_mark(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            _ => {
                if input::edit(&mut self.add_filter, key, Alt::Ignored) {
                    self.note_add_filter_changed();
                }
            }
        }
    }
```

`on_filter_key` no longer takes `ctrl`. In `on_key` (line 898) change the call:

```rust
            Mode::Filter => self.on_filter_key(key),
```

- [ ] **Step 5: Switch the group dialog (`src/app.rs`)**

In `on_group_key`, delete the `Backspace`, `Char('u') | Char('U') if ctrl` and `Char(ch) if !ctrl` arms and replace `_ => {}`:

```rust
    fn on_group_key(&mut self, key: KeyEvent) {
        let groups = self.groups();
        let Some(prompt) = self.group_prompt.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_group(),
            KeyCode::Enter => self.accept_group(),
            // Walks through the groups that exist.
            KeyCode::Up | KeyCode::Down if !groups.is_empty() => {
                let at = groups
                    .iter()
                    .position(|group| group.eq_ignore_ascii_case(prompt.input.trim()));
                let last = groups.len() - 1;
                let next = match (at, key.code == KeyCode::Down) {
                    (Some(index), true) if index < last => index + 1,
                    (Some(index), false) if index > 0 => index - 1,
                    (_, true) => 0,
                    (_, false) => last,
                };
                prompt.input = groups[next].clone();
                prompt.error = None;
            }
            _ => {
                if input::edit(&mut prompt.input, key, Alt::Types) {
                    prompt.error = None;
                }
            }
        }
    }
```

It no longer takes `ctrl`. In `on_key` (line 894):

```rust
            Mode::Group => self.on_group_key(key),
```

If `ctrl` in `on_key` is now used only by `on_add_key` and the list keys, leave it; if clippy reports it unused, remove the binding.

- [ ] **Step 6: Switch the settings form (`src/settings.rs`)**

Add:

```rust
use crate::atoms::ui::input::{self, Alt};
```

Replace `Form::on_key`:

```rust
    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => return Outcome::Save,
            KeyCode::Char('s') if ctrl => return Outcome::Save,
            KeyCode::Tab | KeyCode::Down => self.step(1),
            KeyCode::BackTab | KeyCode::Up => self.step(-1),
            KeyCode::Left if self.focus == Field::Protocol => self.cycle_protocol(-1),
            KeyCode::Right if self.focus == Field::Protocol => self.cycle_protocol(1),
            KeyCode::Char(ch) if !ctrl && !alt && self.focus == Field::Protocol => {
                match ch.to_ascii_lowercase() {
                    ' ' => self.cycle_protocol(1),
                    'a' => self.protocol = Protocol::Auto,
                    'h' => self.protocol = Protocol::Https,
                    's' => self.protocol = Protocol::Ssh,
                    _ => {}
                }
            }
            _ => self.type_key(key),
        }
        Outcome::Stay
    }

    /// An editing key for the field that has the focus.
    fn type_key(&mut self, key: KeyEvent) {
        let text = match self.focus {
            Field::Accounts => &mut self.accounts,
            Field::Pinned => &mut self.pinned,
            Field::Title => &mut self.title,
            Field::Protocol => return,
        };
        if input::edit(text, key, Alt::Ignored) {
            if self.focus == Field::Accounts {
                self.accounts_edited = true;
            }
            self.error = None;
        }
    }
```

`Form::edit` stays: `paste` uses it.

- [ ] **Step 7: Switch the folder prompt (`src/pathprompt.rs`)**

Add:

```rust
use crate::atoms::ui::input::{self, Alt};
```

Replace `PathPrompt::on_key`. Ctrl-W keeps its own meaning here and is matched first:

```rust
    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => match self.folder() {
                Ok(folder) => return Outcome::Accept(folder),
                Err(err) => {
                    self.matches.clear();
                    self.error = Some(err);
                }
            },
            KeyCode::Tab => self.complete(),
            KeyCode::Char('w') | KeyCode::Char('W') if ctrl => self.edit(delete_segment),
            _ => {
                if input::edit(&mut self.input, key, Alt::Types) {
                    self.error = None;
                    self.matches.clear();
                }
            }
        }
        Outcome::Stay
    }
```

- [ ] **Step 8: Check for leftovers**

Run: `grep -nE 'delete_word|\.input\.pop\(\)|_filter\.pop\(\)|_filter\.push\(ch\)' src/app.rs src/settings.rs src/pathprompt.rs`
Expected: no output

- [ ] **Step 9: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `166 passed; 0 failed; 1 ignored` (five new tests, one removed). The existing tests that type into the filter, the Add search, the group dialog, the settings form and the folder prompt pass unchanged.

- [ ] **Step 10: Commit**

```bash
git add -A src
git commit -m "refactor: edit every text field through atoms/ui/input"
```

---

### Task 2: `list` atom

**Files:**
- Create: `src/atoms/ui/list.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/app.rs` (`move_by` 1932–1956, `move_to_end` 1958–1968, `current_selected` 1978–1984, `snap_visible_selection` 2022–2034), `src/ui.rs:323,619,2146-2160`

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod list;` to `src/atoms/ui/mod.rs` (alphabetical: after `input`).

`src/atoms/ui/list.rs`:

```rust
//! Moving through a list whose rows go by name, and keeping the chosen
//! row on screen.

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn moving_steps_from_the_chosen_row_and_stops_at_the_ends() {
        let rows = names(&["a", "b", "c"]);
        let mut chosen = Some("b".to_string());
        move_by(&mut chosen, &rows, 1);
        assert_eq!(chosen.as_deref(), Some("c"));
        move_by(&mut chosen, &rows, 10);
        assert_eq!(chosen.as_deref(), Some("c"));
        move_by(&mut chosen, &rows, -10);
        assert_eq!(chosen.as_deref(), Some("a"));

        // The chosen row is found whatever its case.
        let mut chosen = Some("B".to_string());
        move_by(&mut chosen, &rows, 1);
        assert_eq!(chosen.as_deref(), Some("c"));

        // Nothing chosen counts from the first row.
        let mut chosen = None;
        move_by(&mut chosen, &rows, 1);
        assert_eq!(chosen.as_deref(), Some("b"));

        // An empty list leaves the choice alone.
        let mut chosen = Some("gone".to_string());
        move_by(&mut chosen, &[], 1);
        assert_eq!(chosen.as_deref(), Some("gone"));
    }

    #[test]
    fn moving_to_an_end_takes_the_first_or_the_last_row() {
        let rows = names(&["a", "b", "c"]);
        let mut chosen = Some("b".to_string());
        move_to_end(&mut chosen, &rows, true);
        assert_eq!(chosen.as_deref(), Some("a"));
        move_to_end(&mut chosen, &rows, false);
        assert_eq!(chosen.as_deref(), Some("c"));
        move_to_end(&mut chosen, &[], true);
        assert_eq!(chosen.as_deref(), Some("c"));
    }

    #[test]
    fn snapping_takes_the_first_row_when_the_chosen_one_is_not_shown() {
        let rows = names(&["a", "b"]);
        let mut chosen = Some("b".to_string());
        snap(&mut chosen, &rows);
        assert_eq!(chosen.as_deref(), Some("b"));
        let mut chosen = Some("gone".to_string());
        snap(&mut chosen, &rows);
        assert_eq!(chosen.as_deref(), Some("a"));
        let mut chosen = None;
        snap(&mut chosen, &rows);
        assert_eq!(chosen.as_deref(), Some("a"));
        // Snapping compares names exactly.
        let mut chosen = Some("B".to_string());
        snap(&mut chosen, &rows);
        assert_eq!(chosen.as_deref(), Some("a"));
        let mut chosen = Some("gone".to_string());
        snap(&mut chosen, &[]);
        assert_eq!(chosen.as_deref(), Some("gone"));
    }

    #[test]
    fn scrolling_follows_the_chosen_row() {
        let mut offset = 0;
        align_scroll(&mut offset, 12, 20, 5);
        assert_eq!(offset, 8);
        align_scroll(&mut offset, 3, 20, 5);
        assert_eq!(offset, 3);
        // An offset past the end comes back.
        let mut offset = 30;
        align_scroll(&mut offset, 16, 20, 5);
        assert_eq!(offset, 15);
        align_scroll(&mut offset, 16, 20, 0);
        assert_eq!(offset, 0);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::list`
Expected: compile errors `cannot find function move_by`, `move_to_end`, `snap`, `align_scroll`

- [ ] **Step 3: Write the atom**

Cut `align_scroll` from `src/ui.rs` (lines 2146–2160) and insert it, with the three functions taken from `App`, above the `tests` module:

```rust
/// Moves the choice `delta` rows. The chosen row is found whatever its
/// case; with nothing chosen it counts from the first row.
pub fn move_by(chosen: &mut Option<String>, names: &[String], delta: isize) {
    if names.is_empty() {
        return;
    }
    let index = chosen
        .as_ref()
        .and_then(|name| {
            names
                .iter()
                .position(|item| item.eq_ignore_ascii_case(name))
        })
        .unwrap_or(0);
    let next = (index as isize + delta).clamp(0, names.len() as isize - 1) as usize;
    *chosen = Some(names[next].clone());
}

/// Chooses the first row, or the last.
pub fn move_to_end(chosen: &mut Option<String>, names: &[String], start: bool) {
    if let Some(name) = if start { names.first() } else { names.last() } {
        *chosen = Some(name.clone());
    }
}

/// Chooses the first row when the chosen one is not among `names`.
pub fn snap(chosen: &mut Option<String>, names: &[String]) {
    if names.is_empty() {
        return;
    }
    if chosen
        .as_ref()
        .is_none_or(|name| !names.iter().any(|item| item == name))
    {
        *chosen = Some(names[0].clone());
    }
}

/// Moves `offset` so that row `selected` of `len` is within the `height`
/// rows that are shown.
pub fn align_scroll(offset: &mut usize, selected: usize, len: usize, height: usize) {
    if height == 0 {
        *offset = 0;
        return;
    }
    let max = len.saturating_sub(height);
    if *offset > max {
        *offset = max;
    }
    if selected < *offset {
        *offset = selected;
    } else if selected >= *offset + height {
        *offset = selected + 1 - height;
    }
}
```

- [ ] **Step 4: Switch `ui.rs`**

Add:

```rust
use crate::atoms::ui::list::align_scroll;
```

The two callers (lines 323 and 619) do not change.

- [ ] **Step 5: Switch `app.rs`**

Add:

```rust
use crate::atoms::ui::list;
```

Replace `move_by`, `move_to_end`, `current_selected` and `snap_visible_selection` with:

```rust
    fn move_by(&mut self, delta: isize) {
        if !matches!(self.mode, Mode::List | Mode::Filter | Mode::Add) {
            return;
        }
        let names = self.current_names();
        list::move_by(self.chosen_slot(), &names, delta);
    }

    fn move_to_end(&mut self, start: bool) {
        let names = self.current_names();
        list::move_to_end(self.chosen_slot(), &names, start);
    }

    /// The choice of the list that is on screen: Add's, or the overview's.
    fn chosen_slot(&mut self) -> &mut Option<String> {
        if self.mode == Mode::Add {
            &mut self.add_selected
        } else {
            &mut self.selected
        }
    }
```

and, where `snap_visible_selection` was:

```rust
    fn snap_visible_selection(&mut self) {
        let names = self.current_names();
        list::snap(&mut self.selected, &names);
    }
```

`snap_visible_selection` always works on `self.selected`, in every mode, as before. `current_selected` is gone: `move_by` was its only caller. `current_names` stays.

- [ ] **Step 6: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `170 passed; 0 failed; 1 ignored`

- [ ] **Step 7: Commit**

```bash
git add -A src
git commit -m "refactor: move list navigation and scrolling to atoms/ui/list"
```

---

### Task 3: `menu` atom

**Files:**
- Create: `src/atoms/ui/menu.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/app.rs` (`on_menu_key` 1108–1132), `src/ui.rs` (`draw_menu`, the row loop at 1083–1120 in v0.3.3 numbering; find it by `format!("menu:{index}")`)

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod menu;` to `src/atoms/ui/mod.rs` (alphabetical: after `list`).

`src/atoms/ui/menu.rs`:

```rust
//! A menu of lines, each with a key that runs it.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::hits::{Hits, contains};
use super::text::fit;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    const KEYS: [char; 3] = ['u', 'o', 'd'];

    fn press(selected: &mut usize, code: KeyCode) -> Outcome {
        on_key(selected, KeyEvent::new(code, KeyModifiers::NONE), &KEYS)
    }

    #[test]
    fn arrows_move_the_highlight_and_stop_at_the_ends() {
        let mut selected = 0;
        assert_eq!(press(&mut selected, KeyCode::Up), Outcome::Stay);
        assert_eq!(selected, 0);
        press(&mut selected, KeyCode::Down);
        press(&mut selected, KeyCode::Char('j'));
        press(&mut selected, KeyCode::Down);
        assert_eq!(selected, 2);
        press(&mut selected, KeyCode::Char('k'));
        assert_eq!(selected, 1);
        press(&mut selected, KeyCode::Home);
        assert_eq!(selected, 0);
        press(&mut selected, KeyCode::End);
        assert_eq!(selected, 2);
    }

    #[test]
    fn enter_runs_the_highlighted_line_and_a_key_runs_its_own() {
        let mut selected = 1;
        assert_eq!(press(&mut selected, KeyCode::Enter), Outcome::Run(1));
        assert_eq!(press(&mut selected, KeyCode::Char('d')), Outcome::Run(2));
        assert_eq!(press(&mut selected, KeyCode::Char('O')), Outcome::Run(1));
        assert_eq!(press(&mut selected, KeyCode::Char('x')), Outcome::Stay);
        // A highlight past the end runs the last line.
        let mut selected = 9;
        assert_eq!(press(&mut selected, KeyCode::Enter), Outcome::Run(2));
    }

    #[test]
    fn esc_and_q_close_and_an_empty_menu_runs_nothing() {
        let mut selected = 0;
        assert_eq!(press(&mut selected, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut selected, KeyCode::Char('q')), Outcome::Close);
        let none = on_key(
            &mut selected,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &[],
        );
        assert_eq!(none, Outcome::Stay);
    }

    #[test]
    fn lines_are_drawn_with_their_key_and_remembered() {
        let entries = [
            Entry {
                label: "Update",
                key: 'u',
                warn: false,
            },
            Entry {
                label: "Remove…",
                key: 'd',
                warn: true,
            },
        ];
        let mut terminal = Terminal::new(TestBackend::new(24, 3)).unwrap();
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| draw(frame, Rect::new(0, 0, 24, 3), &entries, 1, None, &mut hits))
            .unwrap();
        let row = |y: u16| -> String {
            let buffer = terminal.backend().buffer();
            (0..24)
                .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                .collect()
        };
        assert_eq!(row(0).trim_end(), "  Update            u");
        assert_eq!(row(1).trim_end(), "▌ Remove…           d");
        assert_eq!(
            hits.rows,
            vec![
                (Rect::new(0, 0, 24, 1), "menu:0".to_string()),
                (Rect::new(0, 1, 24, 1), "menu:1".to_string()),
            ]
        );

        // Lines that do not fit in the area are left out.
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| draw(frame, Rect::new(0, 0, 24, 1), &entries, 0, None, &mut hits))
            .unwrap();
        assert_eq!(hits.rows.len(), 1);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked atoms::ui::menu`
Expected: compile errors `cannot find function on_key`, `draw`, `cannot find type Outcome`, `Entry`

- [ ] **Step 3: Write the atom**

Insert between the `use` lines and the `tests` module:

```rust
/// What a key did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Run the line with this index.
    Run(usize),
}

/// One line of a menu.
pub struct Entry<'a> {
    pub label: &'a str,
    /// The key that runs it, in lowercase.
    pub key: char,
    /// Shown as a warning, for a line that removes something.
    pub warn: bool,
}

/// Handles a key. `keys` holds each line's key, in lowercase and in order.
/// Esc and `q` close; arrows, `j`, `k`, Home and End move the highlight;
/// Enter runs the highlighted line; a line's own key runs that line.
pub fn on_key(selected: &mut usize, key: KeyEvent, keys: &[char]) -> Outcome {
    let last = keys.len().saturating_sub(1);
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Outcome::Close,
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1).min(last),
        KeyCode::Home => *selected = 0,
        KeyCode::End => *selected = last,
        KeyCode::Enter if !keys.is_empty() => return Outcome::Run((*selected).min(last)),
        KeyCode::Char(ch) => {
            let wanted = ch.to_ascii_lowercase();
            if let Some(index) = keys.iter().position(|key| *key == wanted) {
                return Outcome::Run(index);
            }
        }
        _ => {}
    }
    Outcome::Stay
}

/// Draws one line per entry from the top of `area` and records each in
/// `hits` under the name `menu:<index>`. Lines that do not fit are left out.
pub fn draw<A>(
    frame: &mut Frame,
    area: Rect,
    entries: &[Entry],
    selected: usize,
    mouse: Option<(u16, u16)>,
    hits: &mut Hits<A>,
) {
    let width = area.width as usize;
    for (index, entry) in entries.iter().enumerate() {
        let y = area.y + index as u16;
        if y >= area.y + area.height {
            break;
        }
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        let chosen = index == selected;
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let gap = width.saturating_sub(UnicodeWidthStr::width(entry.label) + 6);
        let line = format!(
            "{} {}{}{}  ",
            if chosen { "▌" } else { " " },
            entry.label,
            " ".repeat(gap),
            entry.key
        );
        let style = if chosen {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if entry.warn {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        frame.render_widget(Paragraph::new(Span::styled(fit(&line, width), style)), rect);
        hits.rows.push((rect, format!("menu:{index}")));
    }
}
```

- [ ] **Step 4: Switch `on_menu_key` (`src/app.rs`)**

Add:

```rust
use crate::atoms::ui::menu;
```

Replace `on_menu_key`:

```rust
    fn on_menu_key(&mut self, key: KeyEvent) {
        let Some(state) = self.menu.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        let keys = MenuItem::ALL.map(MenuItem::key);
        match menu::on_key(&mut state.selected, key, &keys) {
            menu::Outcome::Stay => {}
            menu::Outcome::Close => self.close_menu(),
            menu::Outcome::Run(index) => self.run_menu(MenuItem::ALL[index]),
        }
    }
```

- [ ] **Step 5: Switch `draw_menu` (`src/ui.rs`)**

Add:

```rust
use crate::atoms::ui::menu;
```

In `draw_menu`, replace the whole `for (index, item) in MenuItem::ALL.into_iter().enumerate() { … }` loop (it ends after `y += 1;`) with:

```rust
    let entries = MenuItem::ALL.map(|item| menu::Entry {
        label: item.label(many),
        key: item.key(),
        warn: item == MenuItem::Remove,
    });
    // The last line of the dialog is the hint.
    let lines = Rect {
        x: inner.x,
        y,
        width: inner.width,
        height: (inner.y + inner.height)
            .saturating_sub(1)
            .saturating_sub(y),
    };
    menu::draw(frame, lines, &entries, picked, app.mouse, &mut app.hits);
```

`y` may now be declared `let y = …` instead of `let mut y` if the compiler says so; if it is still reassigned in the `if many` block above, it stays `mut`.

If clippy now reports `UnicodeWidthStr` or `contains` as unused in `src/ui.rs`, remove that import.

- [ ] **Step 6: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `174 passed; 0 failed; 1 ignored`. The drawing tests for the menu (`╭ docs `, `▌ Open the folder `, `▌ Update `, every label present) pass unchanged.

- [ ] **Step 7: Commit**

```bash
git add -A src
git commit -m "refactor: handle and draw the actions menu through atoms/ui/menu"
```

---

### Task 4: `path` atom

`pathprompt.rs` is nearly an atom already. It names `RemoteRepo` in two places: `Purpose::Clone(Vec<RemoteRepo>)`, and `target()`, which joins a repository's name to the folder. `Purpose` moves to the app; the prompt carries it as an opaque tag and is told whether the folder must exist.

**Files:**
- Move: `src/pathprompt.rs` → `src/atoms/ui/path.rs`
- Modify: `src/atoms/ui/mod.rs`, `src/main.rs`, `src/app.rs:20,287,1515,2493,2517` and its test at 3287, `src/ui.rs:18,1254` and its tests at 2308–2342

- [ ] **Step 1: Move the file**

```bash
git mv src/pathprompt.rs src/atoms/ui/path.rs
```

Delete `mod pathprompt;` from `src/main.rs`. Add `pub mod path;` to `src/atoms/ui/mod.rs` (alphabetical: after `menu`).

- [ ] **Step 2: Make the prompt generic (`src/atoms/ui/path.rs`)**

Replace the module comment and imports at the top:

```rust
//! A dialog field that asks for a folder, with Tab completion.

use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input::{self, Alt};
use crate::atoms::sys::paths;
```

Delete the `Purpose` enum. Replace the struct, `new` and `folder`, and delete `target`:

```rust
/// `P` says what the folder is for. The prompt only carries it.
pub struct PathPrompt<P> {
    pub purpose: P,
    pub input: String,
    pub error: Option<String>,
    /// Folders that Tab found for what is typed.
    pub matches: Vec<String>,
    /// Refuse a folder that is not there yet.
    must_exist: bool,
}

impl<P> PathPrompt<P> {
    pub fn new(purpose: P, start: &Path, must_exist: bool) -> Self {
        Self {
            purpose,
            input: start.display().to_string(),
            error: None,
            matches: Vec::new(),
            must_exist,
        }
    }
```

```rust
    /// The folder that is typed, checked.
    pub fn folder(&self) -> Result<PathBuf, String> {
        let folder = resolve(&self.input)?;
        if self.must_exist && !folder.is_dir() {
            return Err(format!("{} is not a folder.", folder.display()));
        }
        Ok(folder)
    }
```

`on_key`, `paste`, `edit`, `complete` and the free functions stay as Task 1 left them.

- [ ] **Step 3: Adjust the atom's test and add one**

In `the_prompt_edits_checks_and_accepts`:

| Replace | With |
|---|---|
| `let mut prompt = PathPrompt::new(Purpose::Import, &root);` | `let mut prompt = PathPrompt::new((), &root, true);` |
| `// Import needs a folder that exists.` | `// This prompt needs a folder that exists.` |
| `assert!(prompt.target().is_none());` | *(delete the line)* |

Add:

```rust
    #[test]
    fn a_prompt_can_take_a_folder_that_is_not_there_yet() {
        let root = temp("fresh");
        let mut prompt = PathPrompt::new("clone", &root.join("new"), false);
        assert_eq!(prompt.purpose, "clone");
        // AltGr characters are typed: a path needs `\` on some keyboards.
        prompt.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert!(prompt.input.ends_with("newx"));
        match prompt.on_key(key(KeyCode::Enter)) {
            Outcome::Accept(folder) => assert!(paths::same_path(&folder, &root.join("newx"))),
            _ => panic!("the folder should be accepted"),
        }
        let _ = fs::remove_dir_all(&root);
    }
```

- [ ] **Step 4: Give the app the purpose (`src/app.rs`)**

Replace the import on line 20:

```rust
use crate::atoms::ui::path::{self as pathprompt, PathPrompt};
```

(The alias keeps `pathprompt::Outcome::…` in `on_path_key` as it is, and avoids a module called `path` next to the many local variables of that name.)

Add near the other public types at the top of the file (after `Queued`):

```rust
/// What the folder dialog is asking for.
#[derive(Clone)]
pub enum Purpose {
    /// Clone these repositories into the folder.
    Clone(Vec<RemoteRepo>),
    /// Add the clones that already sit in the folder to the overview.
    Import,
}

impl Purpose {
    /// The dialog for this purpose. Only an import needs a folder that is
    /// already there: cloning creates it.
    pub fn prompt(self, start: &Path) -> PathPrompt<Purpose> {
        let must_exist = matches!(self, Purpose::Import);
        PathPrompt::new(self, start, must_exist)
    }
}

/// Where a single repository ends up, to show under the field.
pub fn clone_target(prompt: &PathPrompt<Purpose>) -> Option<PathBuf> {
    match &prompt.purpose {
        Purpose::Clone(remotes) if remotes.len() == 1 => Some(
            pathprompt::resolve(&prompt.input)
                .ok()?
                .join(&remotes[0].name),
        ),
        _ => None,
    }
}
```

Change the field (line 287):

```rust
    pub prompt: Option<PathPrompt<Purpose>>,
```

Change the three places a prompt is made:

| Line | Old | New |
|---|---|---|
| 1515 | `PathPrompt::new(Purpose::Import, &self.clone_dir)` | `Purpose::Import.prompt(&self.clone_dir)` |
| 2493 | `PathPrompt::new(Purpose::Clone(remotes), &self.clone_dir)` | `Purpose::Clone(remotes).prompt(&self.clone_dir)` |
| 2517 | `PathPrompt::new(Purpose::Clone(vec![remote]), <start>)` (spread over several lines) | `Purpose::Clone(vec![remote]).prompt(<start>)` with the same `<start>` expression |

- [ ] **Step 5: Switch `ui.rs`**

Delete `use crate::pathprompt::Purpose;` (line 18) and add `Purpose` to the existing `use crate::app::{…};` list. In `draw_path` (line 1254), replace `prompt.target()` with:

```rust
crate::app::clone_target(prompt)
```

If `prompt` is not already a reference there, pass `&prompt`.

In the `tests` module, delete `use crate::pathprompt::PathPrompt;` (line 2308) and change the three constructions:

| Old | New |
|---|---|
| `PathPrompt::new(Purpose::Clone(vec![remote("portal")]), &dir)` (spread over lines 2311–2314) | `Purpose::Clone(vec![remote("portal")]).prompt(&dir)` |
| `PathPrompt::new(Purpose::Clone(several), &dir)` | `Purpose::Clone(several).prompt(&dir)` |
| `PathPrompt::new(Purpose::Import, &dir)` | `Purpose::Import.prompt(&dir)` |

- [ ] **Step 6: Check for leftovers**

Run: `grep -rnE 'crate::pathprompt|mod pathprompt|RemoteRepo|\.target\(\)' src/atoms src/main.rs src/ui.rs`
Expected: no output

- [ ] **Step 7: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `175 passed; 0 failed; 1 ignored`. The drawing test that asserts `becomes <dir>/portal` under the folder field covers `clone_target`.

- [ ] **Step 8: Commit**

```bash
git add -A src
git commit -m "refactor: make the folder prompt generic and move it to atoms/ui/path"
```

---

### Task 5: Final check

- [ ] **Step 1: Confirm the atoms name no feature**

Run: `grep -rnE 'gitops|github|gitprogress|config::|LocalRepo|RemoteRepo|crate::app|crate::ui|MenuItem|Purpose' src/atoms`
Expected: no output

- [ ] **Step 2: Confirm the layout**

Run: `ls src/atoms/ui`
Expected: `dialog.rs  hits.rs  input.rs  list.rs  menu.rs  mod.rs  path.rs  progress.rs  text.rs`

- [ ] **Step 3: Full CI run**

Run: `cargo fmt --check; echo "fmt=$?"; cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"; cargo test --locked`
Expected: `fmt=0`, `clippy=0`, `175 passed; 0 failed; 1 ignored`

- [ ] **Step 4: Run against the real settings**

Run: `cargo run -- --doctor; echo "exit=$?"`
Expected: `exit=0`

- [ ] **Step 5: Try the keys by hand**

Run `cargo run` and check (keys are in `docs/USAGE.md`):

- Filter the list: type, Backspace, Ctrl-U, Ctrl-W.
- Add: type an account, Backspace, Ctrl-W.
- Move to a group: type a name; Ctrl-W now deletes a word; Up and Down walk the existing groups.
- Import: type a path including `\`; Tab completes; Ctrl-W drops a path segment.
- Settings: type in each field; Tab moves on.
- Actions menu: arrows, `j`/`k`, a line's own key, Esc.

### Changelog, when this is released

Under `### Changed`:

```markdown
- In the group dialog, Ctrl-W deletes the last word, as it does in the other
  text fields. Ctrl-Shift-U and Ctrl-Shift-W now work wherever Ctrl-U and
  Ctrl-W do.
```
