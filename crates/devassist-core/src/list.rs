//! Moving through a list whose rows go by name, and keeping the chosen
//! row on screen.

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
