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
    scale(2 * theme::RULE_HALF)
}

/// The ticks of a rule with room for `commits` commits from its left edge: one at each commit, the left edge's
/// included, each with its left edge and whether it is a fifth (every fifth from the left edge, which is one).
#[must_use]
pub fn scale(commits: u32) -> Vec<(f32, bool)> {
    (0..=commits).map(|place| (points(place), place % 5 == 0)).collect()
}

/// How wide a rule with room for `commits` commits is: the commits at [`theme::RULE_UNIT`] and the point of its
/// mark.
#[must_use]
pub fn rule_length(commits: u32) -> f32 {
    points(commits) + theme::RULE_ZERO_WIDTH
}

/// The forks table's rule of how far a fork is ahead of the original: from the origin mark at its left edge, a bar
/// to the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forward {
    /// The bar's left edge and width from the rule's left edge, or `None` for nothing ahead.
    pub bar: Option<(f32, f32)>,
    /// Whether more commits are ahead than the rule has room for: the bar is then drawn broken.
    pub over: bool,
}

/// The forks table's rule for `ahead` commits on a rule with room for `room`: the bar starts just after the origin
/// mark, [`theme::RULE_UNIT`] points a commit, held at `room` commits.
#[must_use]
pub fn forward(ahead: u32, room: u32) -> Forward {
    Forward { bar: (ahead > 0).then(|| (theme::RULE_ZERO_WIDTH, points(ahead.min(room)))), over: ahead > room }
}

/// The columns of a forks table row, each with its left edge and width inside the row, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForkColumns {
    /// The fork's `owner/name`; it takes what the others leave.
    pub fork: Span,
    /// Its branches, when there is room.
    pub branches: Option<Span>,
    /// Its pull requests, when there is room.
    pub pulls: Option<Span>,
    /// How far it is ahead: the rule and its words.
    pub ahead: Span,
    /// How many commits the ahead rule has room for: [`theme::FORK_RULE_COMMITS`] unless a narrow row shrank it.
    pub room: u32,
    /// The date of its newest commit, when there is room.
    pub newest: Option<Span>,
    /// Its state.
    pub state: Span,
    /// The room of the row's two buttons.
    pub actions: Span,
}

/// How many of the forks table's optional columns a narrow row can give up, one at a time: the newest commit,
/// branches, pull requests.
const FORK_GIVEN_UP: usize = 3;

/// What a forks table row has given up: how many optional columns, and the ahead rule's room in commits.
#[derive(Clone, Copy)]
struct ForkFit {
    given: usize,
    room: u32,
}

impl ForkFit {
    fn newest(self) -> bool {
        self.given < 1
    }

    fn branches(self) -> bool {
        self.given < 2
    }

    fn pulls(self) -> bool {
        self.given < FORK_GIVEN_UP
    }

    /// The ahead column's width: the drawing's, less the commits its rule gave up.
    fn ahead(self) -> f32 {
        theme::FORK_COLUMN_AHEAD - (rule_length(theme::FORK_RULE_COMMITS) - rule_length(self.room))
    }

    /// The fork column's width in a row of `row_width`, before it is held at zero.
    fn fork_width(self, row_width: f32) -> f32 {
        let others = [
            self.branches().then_some(theme::FORK_COLUMN_BRANCHES),
            self.pulls().then_some(theme::FORK_COLUMN_PULLS),
            Some(self.ahead()),
            self.newest().then_some(theme::FORK_COLUMN_NEWEST),
            Some(theme::FORK_COLUMN_STATE),
            Some(theme::FORK_COLUMN_ACTIONS),
        ];
        let (mut taken, mut gaps) = (theme::ROW_PAD_LEFT + theme::ROW_PAD_RIGHT, 0.0);
        for width in others.into_iter().flatten() {
            taken += width;
            gaps += 1.0;
        }
        row_width - taken - gaps * theme::COLUMN_GAP
    }

    /// What a row of `row_width` gives up: the newest commit, then branches, then pull requests, then commits of the
    /// ahead rule's room down to [`theme::FORK_RULE_LEAST`], until the fork column keeps [`theme::FORK_NAME_LEAST`].
    fn fitting(row_width: f32) -> ForkFit {
        let mut fit = ForkFit { given: 0, room: theme::FORK_RULE_COMMITS };
        while fit.given < FORK_GIVEN_UP && fit.fork_width(row_width) < theme::FORK_NAME_LEAST {
            fit.given += 1;
        }
        while fit.room > theme::FORK_RULE_LEAST && fit.fork_width(row_width) < theme::FORK_NAME_LEAST {
            fit.room -= 1;
        }
        fit
    }
}

/// The columns of a forks table row `row_width` points wide. The fork column starts at the row's left padding and
/// the others are anchored to the right padding, so a wider row only gives the fork more. A row too narrow for even
/// the columns that stay gets a fork column of no width; nothing is negative. A width that is no number, or an
/// infinite one, is taken as no width.
#[must_use]
pub fn fork_columns(row_width: f32) -> ForkColumns {
    let row_width = if row_width.is_finite() { row_width } else { 0.0 };
    let fit = ForkFit::fitting(row_width);
    let mut edge = row_width - theme::ROW_PAD_RIGHT;
    let mut place = |width: f32| {
        let span = Span { left: edge - width, width };
        edge = span.left - theme::COLUMN_GAP;
        span
    };
    let actions = place(theme::FORK_COLUMN_ACTIONS);
    let state = place(theme::FORK_COLUMN_STATE);
    let newest = fit.newest().then(|| place(theme::FORK_COLUMN_NEWEST));
    let ahead = place(fit.ahead());
    let pulls = fit.pulls().then(|| place(theme::FORK_COLUMN_PULLS));
    let branches = fit.branches().then(|| place(theme::FORK_COLUMN_BRANCHES));
    let fork = Span { left: theme::ROW_PAD_LEFT, width: fit.fork_width(row_width).max(0.0) };
    ForkColumns { fork, branches, pulls, ahead, room: fit.room, newest, state, actions }
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

    /// Every shown span of a forks row, left to right, by name.
    fn forks_in_order(c: &ForkColumns) -> Vec<(&'static str, Span)> {
        let optional = [("branches", c.branches), ("pulls", c.pulls)];
        let mut all = vec![("fork", c.fork)];
        all.extend(optional.into_iter().filter_map(|(name, span)| span.map(|span| (name, span))));
        all.push(("ahead", c.ahead));
        all.extend(c.newest.map(|span| ("newest", span)));
        all.extend([("state", c.state), ("actions", c.actions)]);
        all
    }

    #[test]
    fn at_the_design_width_the_forks_columns_are_the_drawings() {
        let c = fork_columns(DESIGN);
        let widths: Vec<(&str, f32)> = forks_in_order(&c).into_iter().map(|(name, span)| (name, span.width)).collect();
        assert_eq!(
            widths,
            [("fork", 256.0), ("branches", 80.0), ("pulls", 104.0), ("ahead", 264.0), ("newest", 112.0), ("state", 168.0), ("actions", 72.0)]
        );
        assert_eq!(c.room, theme::FORK_RULE_COMMITS);
        assert_eq!((c.fork.left, c.actions.right()), (theme::ROW_PAD_LEFT, DESIGN - theme::ROW_PAD_RIGHT));
        for pair in forks_in_order(&c).windows(2) {
            assert!((pair[1].1.left - (pair[0].1.right() + theme::COLUMN_GAP)).abs() < 1e-3, "{pair:?}");
        }
        // The rule has room for twenty commits from its left edge, the words the rest of the column.
        assert_eq!(rule_length(c.room), 161.0);
    }

    #[test]
    fn columns_give_way_before_the_fork_name_is_squeezed() {
        let mut lost = Vec::new();
        let mut before = [true; 3]; // newest, branches, pulls
        let mut room_before = theme::FORK_RULE_COMMITS;
        let mut shrank_at = None;
        for sidebar in [true, false] {
            for width in (960..=2000).rev() {
                #[expect(clippy::cast_precision_loss, reason = "window sizes of a few thousand points")]
                let size = egui::vec2(width as f32, 600.0);
                let panel = crate::shell::layout(size, sidebar, false).panel.width();
                let c = fork_columns(panel);
                let now = [c.newest.is_some(), c.branches.is_some(), c.pulls.is_some()];
                for (index, name) in ["newest", "branches", "pulls"].into_iter().enumerate() {
                    assert!(!now[index] || before[index], "{name} came back at {width} (sidebar {sidebar})");
                    if before[index] && !now[index] {
                        lost.push(name);
                    }
                }
                // The rule shrinks only once the three are gone, and never under its least.
                assert!(c.room == theme::FORK_RULE_COMMITS || now == [false; 3], "{width}: {c:?}");
                assert!(c.room >= theme::FORK_RULE_LEAST && c.room <= room_before, "{width}: {c:?}");
                if c.room < theme::FORK_RULE_COMMITS && shrank_at.is_none() {
                    shrank_at = Some((width, sidebar));
                }
                // In every window the app allows, the fork keeps its least.
                assert!(c.fork.width >= theme::FORK_NAME_LEAST, "{width} (sidebar {sidebar}): the fork is {} wide", c.fork.width);
                assert_eq!(c.ahead.width, theme::FORK_COLUMN_AHEAD - 8.0 * f32::from(u16::try_from(theme::FORK_RULE_COMMITS - c.room).unwrap()));
                let spans = forks_in_order(&c);
                for pair in spans.windows(2) {
                    assert!(pair[1].1.left >= pair[0].1.right() + theme::COLUMN_GAP - 1e-3, "{width}: {pair:?} overlap");
                }
                assert!((c.actions.right() - (panel - theme::ROW_PAD_RIGHT)).abs() < 1e-3);
                (before, room_before) = (now, c.room);
            }
            (before, room_before) = ([true; 3], theme::FORK_RULE_COMMITS);
        }
        assert_eq!(lost, ["newest", "branches", "pulls", "newest"], "in order with the sidebar; only the newest without it");
        assert_eq!(shrank_at.map(|(_, sidebar)| sidebar), Some(true), "the rule shrinks in the narrowest windows with the sidebar");
        // A row narrower than anything the app allows holds nothing negative.
        for width in [0.0, 300.0, f32::NAN, f32::INFINITY] {
            let c = fork_columns(width);
            assert!(forks_in_order(&c).iter().all(|(_, span)| span.width >= 0.0 && span.left.is_finite()), "{width}: {c:?}");
        }
    }

    #[test]
    fn the_forks_rule_starts_at_its_left_edge_and_breaks_past_its_room() {
        assert_eq!(forward(0, 20), Forward { bar: None, over: false });
        assert_eq!(forward(9, 20), Forward { bar: Some((1.0, 72.0)), over: false });
        assert_eq!(forward(20, 20), Forward { bar: Some((1.0, 160.0)), over: false });
        assert_eq!(forward(47, 20), Forward { bar: Some((1.0, 160.0)), over: true });
        assert_eq!(forward(13, 12), Forward { bar: Some((1.0, 96.0)), over: true });
        let s = scale(20);
        assert_eq!(s.len(), 21);
        assert_eq!(s.iter().filter(|(_, fifth)| *fifth).map(|(x, _)| *x).collect::<Vec<_>>(), [0.0, 40.0, 80.0, 120.0, 160.0]);
        assert_eq!(s, ticks(), "the table's rule has the same ticks");
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
