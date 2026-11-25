//! Where the table's columns sit for a row of any width, and the parts of the rule (the small to-scale ruler of
//! commits behind and ahead). Plain geometry in points; nothing here draws.

use crate::theme;

/// A column's left edge and width inside the row, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    /// The distance from the row's left edge to the column's left edge.
    pub left: f32,
    /// The column's width.
    pub width: f32,
}

impl Span {
    /// The distance from the row's left edge to the column's right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.left + self.width
    }
}

/// The columns a table of this width shows, each with its left edge and width inside the row, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    /// The checkbox.
    pub check: Span,
    /// The repository's name; it takes what the others leave.
    pub name: Span,
    /// The repository's folder, when there is room.
    pub folder: Option<Span>,
    /// The checked-out branch, when there is room.
    pub branch: Option<Span>,
    /// Whether the checkout has uncommitted changes, when there is room.
    pub changes: Option<Span>,
    /// The pull requests, when there is room.
    pub pulls: Option<Span>,
    /// How the repository stands to its remote: the rule and its words.
    pub origin: Span,
    /// The room of the row's menu button, kept for when it has a menu; nothing is drawn there yet.
    pub action: Span,
}

/// How many optional columns a narrow row can give up, one at a time: folder, changes, branch, pulls.
const GIVEN_UP_IN_ORDER: usize = 4;

/// How many of the optional columns have been given up: the folder first, then changes, branch and pulls.
#[derive(Clone, Copy)]
struct Given(usize);

impl Given {
    fn folder(self) -> bool {
        self.0 < 1
    }

    fn changes(self) -> bool {
        self.0 < 2
    }

    fn branch(self) -> bool {
        self.0 < 3
    }

    fn pulls(self) -> bool {
        self.0 < GIVEN_UP_IN_ORDER
    }

    /// The columns to the right of the name with their widths, the permanent ones included.
    fn right_of_name(self) -> impl Iterator<Item = f32> {
        [
            self.folder().then_some(theme::COLUMN_FOLDER),
            self.branch().then_some(theme::COLUMN_BRANCH),
            self.changes().then_some(theme::COLUMN_CHANGES),
            self.pulls().then_some(theme::COLUMN_PULLS),
            Some(theme::COLUMN_ORIGIN),
            Some(theme::COLUMN_ACTION),
        ]
        .into_iter()
        .flatten()
    }

    /// The name's width in a row of `row_width`, before it is held at zero (so a row too narrow shows by how much).
    fn name_width(self, row_width: f32) -> f32 {
        let (mut taken, mut gaps) = (theme::ROW_PAD_LEFT + theme::COLUMN_CHECK + theme::ROW_PAD_RIGHT, 0.0);
        for width in self.right_of_name() {
            taken += width;
            gaps += 1.0;
        }
        // One gap after the check, then one before each column right of the name.
        row_width - taken - (gaps + 1.0) * theme::COLUMN_GAP
    }

    /// What a row of `row_width` gives up: optional columns go one at a time, folder, changes, branch, pulls, until
    /// the name keeps [`theme::COLUMN_NAME_LEAST`] or none is left.
    fn fitting(row_width: f32) -> Given {
        let mut given = Given(0);
        while given.0 < GIVEN_UP_IN_ORDER && given.name_width(row_width) < theme::COLUMN_NAME_LEAST {
            given.0 += 1;
        }
        given
    }
}

/// The columns of a row `row_width` points wide. The check starts at the row's left padding and the columns from
/// the origin on are anchored to the right padding, so a wider row only gives the name more. A row too narrow for
/// even the permanent columns gets a name of no width and columns that may overlap it; nothing is negative. A width
/// that is no number, or an infinite one, is taken as no width.
#[must_use]
pub fn columns(row_width: f32) -> Columns {
    let row_width = if row_width.is_finite() { row_width } else { 0.0 };
    let shown = Given::fitting(row_width);
    let mut edge = row_width - theme::ROW_PAD_RIGHT;
    // From the right: each column ends at the edge, and the next one ends a gap before its left.
    let mut place = |width: f32| {
        let span = Span { left: edge - width, width };
        edge = span.left - theme::COLUMN_GAP;
        span
    };
    let action = place(theme::COLUMN_ACTION);
    let origin = place(theme::COLUMN_ORIGIN);
    let pulls = shown.pulls().then(|| place(theme::COLUMN_PULLS));
    let changes = shown.changes().then(|| place(theme::COLUMN_CHANGES));
    let branch = shown.branch().then(|| place(theme::COLUMN_BRANCH));
    let folder = shown.folder().then(|| place(theme::COLUMN_FOLDER));
    let check = Span { left: theme::ROW_PAD_LEFT, width: theme::COLUMN_CHECK };
    let name = Span { left: check.right() + theme::COLUMN_GAP, width: shown.name_width(row_width).max(0.0) };
    Columns { check, name, folder, branch, changes, pulls, origin, action }
}

/// The points that `commits` commits take on the rule.
fn points(commits: u32) -> f32 {
    f32::from(u16::try_from(commits).unwrap_or(u16::MAX)) * theme::RULE_UNIT
}

/// The rule's parts for a row: the bar for commits behind (left of the zero line), the bar for commits ahead (right
/// of it), and whether each is past the scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// Where the zero line ([`theme::RULE_ZERO_WIDTH`] wide) is, from the rule's left edge.
    pub zero: f32,
    /// The behind bar's left edge and width, or `None` for no commits behind.
    pub behind: Option<(f32, f32)>,
    /// The ahead bar's left edge and width, or `None` for no commits ahead.
    pub ahead: Option<(f32, f32)>,
    /// Whether more commits are behind than the rule shows.
    pub behind_over: bool,
    /// Whether more commits are ahead than the rule shows.
    pub ahead_over: bool,
}

/// The rule's parts for `behind` commits behind and `ahead` ahead, [`theme::RULE_UNIT`] points a commit, each bar
/// held at [`theme::RULE_HALF`] commits.
#[must_use]
pub fn rule(behind: u32, ahead: u32) -> Rule {
    let zero = points(theme::RULE_HALF);
    let behind_width = points(behind.min(theme::RULE_HALF));
    let ahead_width = points(ahead.min(theme::RULE_HALF));
    Rule {
        zero,
        behind: (behind > 0).then_some((zero - behind_width, behind_width)),
        // The ahead bar starts just after the zero line.
        ahead: (ahead > 0).then_some((zero + theme::RULE_ZERO_WIDTH, ahead_width)),
        behind_over: behind > theme::RULE_HALF,
        ahead_over: ahead > theme::RULE_HALF,
    }
}

/// Where each of the rule's ticks is (its left edge), and whether it is a fifth: one tick for each commit from
/// [`theme::RULE_HALF`] behind to [`theme::RULE_HALF`] ahead, the middle one included.
#[must_use]
pub fn ticks() -> Vec<(f32, bool)> {
    (0..=2 * theme::RULE_HALF).map(|place| (points(place), place.abs_diff(theme::RULE_HALF) % 5 == 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESIGN: f32 = 1148.0;

    fn shown_of(c: &Columns) -> [bool; 4] {
        [c.folder.is_some(), c.changes.is_some(), c.branch.is_some(), c.pulls.is_some()]
    }

    /// Every shown span, left to right.
    fn in_order(c: &Columns) -> Vec<Span> {
        let mut all = vec![c.check, c.name];
        all.extend([c.folder, c.branch, c.changes, c.pulls].into_iter().flatten());
        all.extend([c.origin, c.action]);
        all
    }

    #[test]
    fn at_the_design_width_every_column_is_there_and_none_overlaps() {
        let c = columns(DESIGN);
        assert!(c.folder.is_some() && c.branch.is_some() && c.changes.is_some() && c.pulls.is_some());
        assert_eq!(c.origin.width, theme::COLUMN_ORIGIN);
        assert_eq!(c.action.right(), DESIGN - theme::ROW_PAD_RIGHT);
        assert_eq!(c.check.left, theme::ROW_PAD_LEFT);
        assert_eq!(c.name.left, c.check.right() + theme::COLUMN_GAP);
        for pair in in_order(&c).windows(2) {
            assert!((pair[1].left - (pair[0].right() + theme::COLUMN_GAP)).abs() < 1e-3, "{pair:?}");
        }
        assert!(c.name.width >= theme::COLUMN_NAME_LEAST);
    }

    #[test]
    fn at_the_design_width_each_column_has_its_theme_width() {
        let c = columns(DESIGN);
        assert_eq!(c.check.width, theme::COLUMN_CHECK);
        assert_eq!(c.folder.map(|s| s.width), Some(theme::COLUMN_FOLDER));
        assert_eq!(c.branch.map(|s| s.width), Some(theme::COLUMN_BRANCH));
        assert_eq!(c.changes.map(|s| s.width), Some(theme::COLUMN_CHANGES));
        assert_eq!(c.pulls.map(|s| s.width), Some(theme::COLUMN_PULLS));
        assert_eq!(c.action.width, theme::COLUMN_ACTION);
        // The panel is 1148 wide; the name takes what the seven fixed columns, seven gaps and the paddings leave.
        let fixed = theme::COLUMN_CHECK
            + theme::COLUMN_FOLDER
            + theme::COLUMN_BRANCH
            + theme::COLUMN_CHANGES
            + theme::COLUMN_PULLS
            + theme::COLUMN_ORIGIN
            + theme::COLUMN_ACTION;
        let want = DESIGN - fixed - 7.0 * theme::COLUMN_GAP - theme::ROW_PAD_LEFT - theme::ROW_PAD_RIGHT;
        assert!((c.name.width - want).abs() < 1e-3);
    }

    #[test]
    fn a_narrow_table_gives_up_columns_in_order() {
        let mut lost = Vec::new(); // the order in which the columns went, found by stepping the width down
        let mut before = shown_of(&columns(DESIGN)); // folder, changes, branch, pulls
        let mut width = DESIGN;
        while width >= 400.0 {
            let c = columns(width);
            let now = shown_of(&c);
            for (index, name) in ["folder", "changes", "branch", "pulls"].into_iter().enumerate() {
                assert!(!now[index] || before[index], "{name} came back at {width}");
                if before[index] && !now[index] {
                    lost.push(name);
                }
            }
            before = now;
            // The permanent columns are there and the name keeps its least while a column could still go.
            assert!(c.check.width > 0.0 && c.origin.width > 0.0 && c.action.width > 0.0);
            assert!(c.name.width >= theme::COLUMN_NAME_LEAST || now == [false; 4], "{width}: {c:?}");
            width -= 1.0;
        }
        assert_eq!(lost, ["folder", "changes", "branch", "pulls"]);
    }

    #[test]
    fn a_column_goes_only_when_the_name_would_otherwise_be_too_narrow() {
        // All eight columns have seven gaps between them; the name keeps exactly its least at this width.
        let fixed = theme::COLUMN_CHECK
            + theme::COLUMN_FOLDER
            + theme::COLUMN_BRANCH
            + theme::COLUMN_CHANGES
            + theme::COLUMN_PULLS
            + theme::COLUMN_ORIGIN
            + theme::COLUMN_ACTION;
        let narrowest = theme::ROW_PAD_LEFT + theme::ROW_PAD_RIGHT + fixed + 7.0 * theme::COLUMN_GAP + theme::COLUMN_NAME_LEAST;
        assert_eq!(shown_of(&columns(narrowest)), [true; 4], "all four fit exactly at {narrowest}");
        assert_eq!(columns(narrowest - 1.0).folder, None);
        assert!(columns(narrowest - 1.0).changes.is_some());
    }

    #[test]
    fn a_table_narrower_than_its_fixed_columns_does_not_go_negative() {
        for width in [0.0, 100.0, 300.0] {
            let c = columns(width);
            assert!(c.name.width >= 0.0 && c.origin.width >= 0.0);
            for span in in_order(&c) {
                assert!(span.width >= 0.0 && !span.left.is_nan() && !span.width.is_nan(), "{width}: {span:?}");
            }
            assert_eq!(shown_of(&c), [false; 4]);
        }
    }

    #[test]
    fn a_row_width_that_is_no_number_gives_places_that_are() {
        for width in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let c = columns(width);
            for span in in_order(&c) {
                assert!(span.left.is_finite() && span.width.is_finite() && span.width >= 0.0, "{width}: {span:?}");
            }
            assert_eq!(c, columns(0.0), "{width}");
        }
    }

    #[test]
    fn the_name_reaches_zero_where_the_four_permanent_columns_just_fit() {
        let permanent =
            theme::ROW_PAD_LEFT + theme::ROW_PAD_RIGHT + theme::COLUMN_CHECK + theme::COLUMN_ORIGIN + theme::COLUMN_ACTION + 3.0 * theme::COLUMN_GAP;
        assert!(columns(permanent).name.width.abs() < 1e-3);
        assert!(columns(permanent + 10.0).name.width > 9.0);
        assert_eq!(columns(permanent - 10.0).name.width, 0.0);
    }

    #[test]
    fn a_wider_row_only_gives_the_name_more() {
        let (a, b) = (columns(DESIGN), columns(DESIGN + 100.0));
        assert!((b.name.width - a.name.width - 100.0).abs() < 1e-3);
        assert_eq!(b.name.left, a.name.left);
        assert_eq!(b.action.right(), DESIGN + 100.0 - theme::ROW_PAD_RIGHT);
    }

    #[test]
    fn the_rule_is_to_scale_up_to_ten_each_way() {
        let r = rule(4, 1);
        assert_eq!(r.zero, 80.0);
        assert_eq!(r.behind, Some((48.0, 32.0))); // left edge, width: four commits left of zero
        assert_eq!(r.ahead, Some((81.0, 8.0)));
        assert!(!r.behind_over && !r.ahead_over);
        let over = rule(25, 0);
        assert_eq!(over.behind, Some((0.0, 80.0)));
        assert!(over.behind_over && !over.ahead_over);
        assert_eq!(over.ahead, None);
        assert_eq!(rule(0, 0).behind, None);
        assert_eq!(rule(0, 0).ahead, None);
    }

    #[test]
    fn the_rule_is_full_at_ten_and_over_from_eleven() {
        let ten = rule(10, 10);
        assert_eq!(ten.behind, Some((0.0, 80.0)));
        assert_eq!(ten.ahead, Some((81.0, 80.0)));
        assert!(!ten.behind_over && !ten.ahead_over);
        let eleven = rule(11, 11);
        assert_eq!(eleven.behind, ten.behind);
        assert_eq!(eleven.ahead, ten.ahead);
        assert!(eleven.behind_over && eleven.ahead_over);
        assert_eq!(rule(u32::MAX, u32::MAX).ahead, ten.ahead);
        assert_eq!(ten.ahead.map(|(left, width)| left + width), Some(theme::RULE_WIDTH));
    }

    #[test]
    fn the_rule_has_a_tick_a_commit_and_a_taller_one_every_fifth() {
        let t = ticks();
        assert_eq!(t.len(), 21);
        assert_eq!(t.iter().filter(|(_, fifth)| *fifth).count(), 5); // -10, -5, 0, 5, 10
        let fifths: Vec<f32> = t.iter().filter(|(_, fifth)| *fifth).map(|(x, _)| *x).collect();
        assert_eq!(fifths, [0.0, 40.0, 80.0, 120.0, 160.0]);
        assert_eq!(t.first().map(|(x, _)| *x), Some(0.0));
        assert_eq!(t.get(10).map(|(x, _)| *x), Some(rule(0, 0).zero));
        assert_eq!(t.last().map(|(x, _)| *x), Some(160.0));
        for pair in t.windows(2) {
            assert!((pair[1].0 - pair[0].0 - theme::RULE_UNIT).abs() < f32::EPSILON);
        }
    }
}
