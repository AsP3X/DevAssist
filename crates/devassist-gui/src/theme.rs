//! The window's colours, sizes and materials, from the Quill design.

use std::time::Duration;

use devassist_core::config::Theme;
use devassist_glass::material::{self, Lookup, Material, Params};
use egui::Color32;
use serde_json::{Value, json};

/// The window's size when it opens.
pub const WINDOW: [f32; 2] = [1440.0, 900.0];
/// The smallest the window can be made.
pub const MIN_WINDOW: [f32; 2] = [960.0, 600.0];
/// The gap between the window's edge and its panes, and between panes.
pub const INSET: f32 = 12.0;
/// The sidebar's width.
pub const SIDEBAR_WIDTH: f32 = 256.0;
/// The height of the bar at the top.
pub const BAR_HEIGHT: f32 = 36.0;
/// The height of the status bar.
pub const STATUS_HEIGHT: f32 = 32.0;
/// The height of a row in the sidebar's navigation.
pub const NAV_ROW: f32 = 32.0;
/// The corner radius of small things.
pub const RADIUS_SMALL: f32 = 4.0;
/// The corner radius of a row.
pub const RADIUS_ROW: f32 = 12.0;
/// The corner radius of a panel.
pub const RADIUS_PANEL: f32 = 16.0;
/// The corner radius of a pane.
pub const RADIUS_PANE: f32 = 20.0;
/// The size of body text.
pub const TEXT: f32 = 14.0;
/// The size of small text.
pub const TEXT_SMALL: f32 = 12.0;
/// The size of a title.
pub const TEXT_TITLE: f32 = 16.0;
/// The size of an icon.
pub const ICON: f32 = 16.0;
/// How far the backdrop is blurred behind the panes.
pub const BACKDROP_BLUR: f32 = 24.0;
/// How strong the quiet text colour is in the light mode, of the full text colour.
pub const QUIET_OPACITY_LIGHT: f32 = 0.74;
/// How strong the quiet text colour is in the dark mode, of the full text colour: stronger than in the light mode,
/// so that it holds its contrast on the table's fills (the user's choice of 2026-11-25).
pub const QUIET_OPACITY_DARK: f32 = 0.89;

/// How strong the quiet text colour is in `mode`, of the full text colour.
#[must_use]
pub fn quiet_opacity(mode: Mode) -> f32 {
    match mode {
        Mode::Light => QUIET_OPACITY_LIGHT,
        Mode::Dark => QUIET_OPACITY_DARK,
    }
}
/// How far up from the window's bottom the veil keeps its lower strength, as a share of the height; above, it
/// rises evenly to its strength at the top.
pub const VEIL_KNEE: f32 = 0.45;
/// The width of one of the window's own caption buttons.
pub const CAPTION_BUTTON: f32 = 40.0;
/// The room kept at the left of the top bar for the traffic lights on macOS.
pub const TRAFFIC_LIGHT_SPACE: f32 = 90.0;
/// How far in from the window's edge a press resizes it, where the window draws its frame itself. Quill's value.
pub const WINDOW_EDGE: f32 = 6.0;
/// How far along an edge from a corner a press resizes the window both ways. Quill's value.
pub const WINDOW_CORNER: f32 = 16.0;
/// The room between the sidebar's sides and foot and its rows.
pub const SIDEBAR_PADDING: f32 = 8.0;
/// The room between the sidebar's top and its first row.
pub const SIDEBAR_TOP: f32 = 16.0;
/// The room inside a row before its icon or text.
pub const ROW_PADDING: f32 = 8.0;
/// The gap between an icon and its text.
pub const ICON_GAP: f32 = 8.0;
/// The gap between two rows of the sidebar.
pub const ROW_GAP: f32 = 4.0;
/// The gap before each group of the sidebar's rows.
pub const GROUP_GAP: f32 = 20.0;
/// The height of a group's label in the sidebar.
pub const LABEL_ROW: f32 = 24.0;
/// The gap between a group's label and its first row.
pub const LABEL_GAP: f32 = 8.0;
/// The side of the accent square that marks the workspace at the sidebar's top.
pub const WORKSPACE_MARK: f32 = 32.0;
/// The gap between the sidebar button and the title.
pub const BAR_GAP: f32 = 12.0;
/// The room the top row keeps for the title, from its left edge; the title is shorter, and the window's own
/// buttons start further right.
pub const TITLE_ROOM: f32 = 472.0;
/// The closest any text comes to a panel's edge: the smaller of the two side paddings the design's table rows keep
/// (12 at the left, 8 at the right).
pub const PANEL_TEXT_INSET: f32 = 8.0;
/// The room at either end of the status bar before its text.
pub const STATUS_PADDING: f32 = 16.0;

/// The height of a repository row in the table.
pub const ROW_HEIGHT: f32 = 44.0;
/// The height of the table's heading row.
pub const HEAD_HEIGHT: f32 = 40.0;
/// The height of a group's heading row in the table.
pub const GROUP_HEIGHT: f32 = 36.0;
/// The room at the left of a table row before its first column.
pub const ROW_PAD_LEFT: f32 = 12.0;
/// The room at the right of a table row after its last column.
pub const ROW_PAD_RIGHT: f32 = 8.0;
/// The gap between two columns of the table.
pub const COLUMN_GAP: f32 = 12.0;
/// The width of the column with the row's checkbox.
pub const COLUMN_CHECK: f32 = 24.0;
/// The width of the column with the repository's folder.
pub const COLUMN_FOLDER: f32 = 176.0;
/// The width of the column with the checked-out branch.
pub const COLUMN_BRANCH: f32 = 136.0;
/// The width of the column that says whether the checkout has uncommitted changes.
pub const COLUMN_CHANGES: f32 = 104.0;
/// The width of the column with the ahead and behind rule and its counts.
pub const COLUMN_PULLS: f32 = 134.0;
/// The width of the column with the origin's address.
pub const COLUMN_ORIGIN: f32 = 288.0;
/// The width of the column with the row's menu button.
pub const COLUMN_ACTION: f32 = 32.0;
/// The least width the name column keeps; it takes what the other columns leave.
pub const COLUMN_NAME_LEAST: f32 = 120.0;
/// How many points one commit takes in the ahead and behind rule.
pub const RULE_UNIT: f32 = 8.0;
/// How many commits the rule shows each way from its middle; more are drawn as this many.
pub const RULE_HALF: u32 = 10;
/// The width of the rule: its commits each way, at [`RULE_UNIT`] points each, and one more point for the middle
/// line.
pub const RULE_WIDTH: f32 = rule_width();
/// The height of the rule.
pub const RULE_HEIGHT: f32 = 16.0;
/// The height of the bar that shows the commits ahead or behind.
pub const RULE_BAR: f32 = 6.0;
/// The distance from the rule's top to its bar.
pub const RULE_BAR_TOP: f32 = 2.0;
/// The height of the rule's small tick at each commit.
pub const RULE_TICK: f32 = 4.0;
/// The height of the taller tick at every fifth commit.
pub const RULE_TICK_FIFTH: f32 = 7.0;
/// The gap between the rule and the number beside it.
pub const RULE_LABEL_GAP: f32 = 12.0;
/// How strong the rule's small ticks are, of the text colour.
pub const RULE_TICK_OPACITY: f32 = 0.28;
/// How strong the rule's taller ticks are, of the text colour.
pub const RULE_FIFTH_OPACITY: f32 = 0.5;
/// The room between a view capsule's edge and the controls in it.
pub const CAPSULE_PAD: f32 = 4.0;
/// The gap between two controls in a capsule.
pub const CAPSULE_GAP: f32 = 2.0;
/// The height of an action button.
pub const ACTION_HEIGHT: f32 = 28.0;
/// The room inside an action button before its first item and after its last, left then right.
pub const ACTION_PAD: [f32; 2] = [8.0, 10.0];
/// The gap between the items of an action button, and between action buttons.
pub const ACTION_GAP: f32 = 6.0;
/// The room inside the top row's main button ("Add repository") before its icon and after its words, left then
/// right (`primary`'s `padding: [0, 16, 0, 12]`); its icon and words are [`ACTION_GAP`] apart (`gap: 6`), it is
/// [`BAR_HEIGHT`] high and round at its ends (`cornerRadius: 18`), and with its icon alone it is as wide as high.
pub const MAIN_ACTION_PAD: [f32; 2] = [12.0, 16.0];
/// The width of a search field.
pub const FIELD_WIDTH: f32 = 280.0;
/// The least width the filter field narrows to, in a window too narrow for it, the view and the selection bar
/// without its words: room for its icon and its hint.
pub const FIELD_MIN_WIDTH: f32 = 200.0;
/// The width of the line between a capsule's label and its actions (the selection's `divider()`, `width: 1`).
pub const DIVIDER_WIDTH: f32 = 1.0;
/// The height of that line (`height: 16`).
pub const DIVIDER_HEIGHT: f32 = 16.0;
/// How strong that line is, of the text colour (`fill: soft(0.16)`).
pub const DIVIDER_OPACITY: f32 = 0.16;
/// The room inside a search field before its icon.
pub const FIELD_PAD: f32 = 12.0;
/// The side of a checkbox.
pub const CHECKBOX: f32 = 16.0;
/// The gap between the fold arrow, the name and the note in a group's heading row.
pub const GROUP_ROW_GAP: f32 = 8.0;
/// The gap between the controls in the bar above the table.
pub const TOOLBAR_GAP: f32 = 8.0;
/// How strong the lines of the table are, of the text colour.
pub const LINE_OPACITY: f32 = 0.10;
/// How strong the fill of the table's heading row is, of the text colour.
pub const HEAD_FILL_OPACITY: f32 = 0.06;
/// How strong the fill of a group's heading row is, of the text colour.
pub const GROUP_FILL_OPACITY: f32 = 0.03;
/// How strong the fill under the pointer is, of the text colour.
pub const HOVER_OPACITY: f32 = 0.05;
/// How strong the outline of an unchecked checkbox is, of the text colour.
pub const CHECKBOX_STROKE_OPACITY: f32 = 0.4;
/// How strong the fill of a selected row is in the light mode, of the accent colour.
pub const SELECTED_OPACITY_LIGHT: f32 = 0.14;
/// How strong the fill of a selected row is in the dark mode, of the accent colour.
pub const SELECTED_OPACITY_DARK: f32 = 0.32;

/// The width of the lines under the table's rows and of the rule's ticks (`LINE`'s `strokeWidth: { bottom: 1 }`
/// and the ticks' `size: 1` in `v5-app.mjs`).
pub const HAIRLINE: f32 = 1.0;
/// The width of the rule's zero line, between the bars behind and ahead (the origin's `width: 1` in `v5-app.mjs`).
pub const RULE_ZERO_WIDTH: f32 = 1.0;
/// The size of a small icon: the watched eye, the sort arrow and the pull request states (`icon(…, 14)` in
/// `reposTable` and `pullCell`).
pub const ICON_SMALL: f32 = 14.0;
/// The size of the mark in a ticked checkbox (`icon("check", WHITE, 12)` in `checkbox`).
pub const CHECK_MARK: f32 = 12.0;
/// The gap between a repository's name and its eye (the `Name` row's `gap: 8` in `reposTable`).
pub const NAME_ICON_GAP: f32 = 8.0;
/// The gap between an icon and its text in a cell: the branch, the sort arrow, a failure (`gap: 6` in
/// `reposTable` and `origin`).
pub const CELL_ICON_GAP: f32 = 6.0;
/// The gap between a pull request state's icon and its number (`piece`'s `gap: 4` in `pullCell`).
pub const PULL_ICON_GAP: f32 = 4.0;
/// The gap between the parts of the pull requests cell (`gap: 8` in `pullCell`).
pub const PULL_GAP: f32 = 8.0;
/// The gap between the rule's zero line and the head's words "Behind" (ending before it) and "Ahead" (starting
/// after it): the legend's `fixed("Behind", ZERO - 8, …)` and `gap: 17` (8, the line, 8) in `reposTable`.
pub const RULE_LEGEND_GAP: f32 = 8.0;
/// The gap that breaks a bar past the rule's scale, near its outer end (the fork table's "Ahead, past the
/// scale", from 141 to 147, in `forward`).
pub const RULE_BREAK_GAP: f32 = 6.0;
/// The length of a broken bar's outer piece past the gap (the fork table's 14 in `forward`).
pub const RULE_BREAK_END: f32 = 14.0;
/// The height of a progress bar; its ends are round (`bar`'s `height: 6`, `cornerRadius: 3`).
pub const PROGRESS_HEIGHT: f32 = 6.0;
/// The least length of a progress bar's filled part (`Math.max(6, …)` in `bar`).
pub const PROGRESS_LEAST: f32 = 6.0;
/// How strong a progress bar's track is, of the text colour (`fill: soft(0.12)` in `bar`).
pub const PROGRESS_TRACK_OPACITY: f32 = 0.12;

/// The height of the badge with a count at the end of a sidebar row (`navRow`'s `Count`, `height: 20`).
pub const BADGE_HEIGHT: f32 = 20.0;
/// The room inside the badge at either side of its count (`padding: [0, 6]`).
pub const BADGE_PAD: f32 = 6.0;
/// The room between the badge and its row's right edge (`navRow`'s `padding: [0, 4, …]` with a count).
pub const BADGE_INSET: f32 = 4.0;
/// The room inside a capsule's label before its words and after them, left then right (the selection's `Count`,
/// `padding: [0, 8, 0, 10]`).
pub const LABEL_PAD: [f32; 2] = [10.0, 8.0];
/// The size of an icon in the status bar (`note`'s `icon(…, QUIET, 12)`).
pub const STATUS_ICON: f32 = 12.0;
/// The gap between a status note's icon and its words (`note`'s `gap: 6`).
pub const STATUS_ICON_GAP: f32 = 6.0;
/// The gap between the parts of the status bar (`statusBar`'s `gap: 24`).
pub const STATUS_GAP: f32 = 24.0;
/// The gap between the title of an empty or broken panel and the lines under it.
pub const STATE_GAP: f32 = 8.0;
/// How long the settings warning is the status bar's text after the window opens.
pub const NOTICE_TIME: Duration = Duration::from_secs(10);
/// How many rows Page Up and Page Down move the highlight, as in the terminal app.
pub const PAGE_ROWS: isize = 10;

/// The corner radius of a dialog's pane.
pub const DIALOG_RADIUS: f32 = 20.0;
/// The room between a dialog pane's edge and what is in it.
pub const DIALOG_PAD: f32 = 24.0;
/// The gap between the parts of a dialog: its title, its body and its buttons.
pub const DIALOG_GAP: f32 = 24.0;
/// The least room between a dialog or a menu and the window's edge.
pub const DIALOG_MARGIN: f32 = 24.0;
/// The size of a dialog's title, in the medium face.
pub const DIALOG_TITLE: f32 = 16.0;
/// The corner radius of a box inset in a dialog, such as its list of paths.
pub const INSET_RADIUS: f32 = 16.0;
/// The height of a dialog's button.
pub const BUTTON_HEIGHT: f32 = 36.0;
/// The corner radius of a dialog's button: half its height, so its ends are round.
pub const BUTTON_RADIUS: f32 = 18.0;
/// The room inside a dialog's button at either side of its words.
pub const BUTTON_PAD: f32 = 16.0;
/// The gap between two buttons of a dialog.
pub const BUTTON_GAP: f32 = 8.0;
/// The width of a menu.
pub const MENU_WIDTH: f32 = 256.0;
/// The corner radius of a menu's pane.
pub const MENU_RADIUS: f32 = 16.0;
/// The room between a menu pane's edge and its rows.
pub const MENU_PAD: f32 = 4.0;
/// The height of a menu's row.
pub const MENU_ROW: f32 = 32.0;
/// The corner radius of a menu row's fill.
pub const MENU_ROW_RADIUS: f32 = 12.0;
/// The height of the line between two parts of a menu, with the room above and below it.
pub const MENU_LINE: f32 = 9.0;
/// The gap between a menu and the button it opened from.
pub const MENU_GAP: f32 = 4.0;
/// The size of a key's name at the end of a menu row.
pub const KEY_TEXT: f32 = 12.0;
/// The room inside a menu's row before its icon and after its key, and at either side of the line between two
/// parts of a menu (`menuRow`'s and `menuLine`'s `padding: [0, 8]`).
pub const MENU_ROW_PAD: f32 = 8.0;
/// The gap between a menu row's icon, its words and its key (`menuRow`'s `gap: 8`).
pub const MENU_ROW_GAP: f32 = 8.0;
/// The width of the key's name at the end of a menu row; the name stands at its right (`fixed(key, 16, …)`).
pub const MENU_KEY_WIDTH: f32 = 16.0;
/// How strong the line between two parts of a menu is, of the text colour (`menuLine`'s `fill: soft(0.12)`).
pub const MENU_LINE_OPACITY: f32 = 0.12;
/// The width of the side menu a menu's line opens (the Migrate menu's `264`).
pub const SUBMENU_WIDTH: f32 = 264.0;
/// The gap between a menu and the side menu beside it (the Migrate menu ends at 892 + 264 = 1156, the actions menu
/// starts at 1164).
pub const SUBMENU_GAP: f32 = 8.0;
/// The size of the chevron at the end of a menu's line that opens a side menu (`menuRow`'s `more`:
/// `icon("chevron-right", QUIET)`, an icon's 16 points), drawn in the quiet colour.
pub const MENU_CHEVRON: f32 = 16.0;
/// The height of the fill behind a row's menu button, centred on the row; its ends are round (`Row actions`'s
/// `height: 32`, `cornerRadius: 16`).
pub const DOTS_HEIGHT: f32 = 32.0;
/// The corner radius of the fill behind a row's menu button.
pub const DOTS_RADIUS: f32 = 16.0;
/// The gap between a dialog's title and the quiet line under it (`dialogTitle`'s `gap: 4`).
pub const HEADER_GAP: f32 = 4.0;
/// The height of a line of a dialog's quiet line, of its size (`dialogTitle`'s `lineHeight: 1.45`).
pub const HEADER_LINE_HEIGHT: f32 = 1.45;
/// How many lines a dialog's quiet line takes at most; a longer one (a run's summary that names a hundred failed
/// repositories) ends with an ellipsis there, so the dialog's lists keep their room (the plan's own: the drawing's
/// lines take two).
pub const HEADER_LINES: usize = 3;
/// How strong a dialog's button is drawn while it cannot be pressed, of its full strength (not the design's: it
/// draws no such button; the plan's own).
pub const BUTTON_OFF_OPACITY: f32 = 0.5;
/// The width of the progress dialog (`dialog("Update", 640, …)`).
pub const PROGRESS_WIDTH: f32 = 640.0;
/// The height of a task's row in the progress dialog (`task`'s `height: 44`).
pub const TASK_HEIGHT: f32 = 44.0;
/// The room at either side of a task's row, and the gap between its parts (`task`'s `padding: [0, 12]`, `gap: 12`).
pub const TASK_PAD: f32 = 12.0;
/// The width of a task's name (`fixed(name, 120, …)`).
pub const TASK_NAME: f32 = 120.0;
/// The width of a task's bar (`bar(200, share)`).
pub const TASK_BAR: f32 = 200.0;
/// How many task rows the progress dialog shows before its list scrolls.
pub const TASK_ROWS: usize = 6;
/// The room inside the output's box at every side (`Output`'s `padding: 12`).
pub const OUTPUT_PAD: f32 = 12.0;
/// The gap between two lines of output (`Output`'s `gap: 8`).
pub const OUTPUT_GAP: f32 = 8.0;
/// The height of one line of output, in the 12-point monospaced face (the drawing's lines stand 22 points apart,
/// the gap being 8).
pub const OUTPUT_LINE: f32 = 14.0;
/// How many lines of output the progress dialog shows (the drawing's seven).
pub const OUTPUT_ROWS: usize = 7;
/// The size of the output's text (`mono`'s `fontSize: 12`).
pub const OUTPUT_TEXT: f32 = 12.0;
/// The width of the remove dialog (`dialog("Remove", 480, …)`), and of the group dialog, which the design does not
/// draw and which is built to the remove dialog's pattern.
pub const DIALOG_WIDTH: f32 = 480.0;
/// The room inside a dialog's list above its first row and below its last, then at either side of its rows
/// (`What goes`'s `padding: [4, 12]`).
pub const LIST_PAD: [f32; 2] = [4.0, 12.0];
/// The height of a row of a dialog's list (`Folder`'s `height: 36`).
pub const LIST_ROW: f32 = 36.0;
/// The gap between a path's icon, the path and its note in the remove dialog (`Folder`'s `gap: 8`).
pub const LIST_GAP: f32 = 8.0;
/// How many paths the remove dialog lists before its list scrolls.
pub const REMOVE_ROWS: usize = 8;
/// How many groups the group dialog lists before its list scrolls.
pub const GROUP_ROWS: usize = 6;
/// The height of a dialog's text field; its ends are round (`field`'s `height: 36`, `cornerRadius: 18`).
pub const DIALOG_FIELD_HEIGHT: f32 = 36.0;
/// The corner radius of a dialog's text field: half its height, so its ends are round (`field`'s `cornerRadius: 18`).
pub const DIALOG_FIELD_RADIUS: f32 = DIALOG_FIELD_HEIGHT / 2.0;
/// The height of a row of the folders a folder field suggests (the plan's own: the design draws no suggestions;
/// a menu's row, `MENU_ROW`).
pub const FOLDER_ROW: f32 = MENU_ROW;
/// How many suggested folders a folder field shows before its rows scroll.
pub const FOLDER_ROWS: usize = 6;
/// The gap between a folder field and the box of its suggestions (a menu's gap under its button, `MENU_GAP`).
pub const FOLDER_GAP: f32 = MENU_GAP;
/// The room inside the box of a folder field's suggestions above its first row and below its last (a menu's pad,
/// `MENU_PAD`).
pub const FOLDER_INSET_PAD: f32 = MENU_PAD;
/// The room between the box of a folder field's suggestions and a lit row's fill at either side, and that fill's
/// corner radius: a menu's row in its pane (`MENU_PAD`, `MENU_ROW_RADIUS`).
pub const FOLDER_PICK: [f32; 2] = [MENU_PAD, MENU_ROW_RADIUS];
/// How strong the outline of a dialog's text field is, of the text colour (`field`'s `stroke: soft(0.16)`).
pub const FIELD_EDGE_OPACITY: f32 = 0.16;
/// The gap between the group dialog's field, its note and its list of groups (not the design's: it draws no group
/// dialog; the plan's own, half a dialog's gap).
pub const GROUP_DIALOG_GAP: f32 = 12.0;
/// The room between the group list's box and a picked row's fill at either side, and that fill's corner radius:
/// a menu's row in its pane (`MENU_PAD`, `MENU_ROW_RADIUS`; not the design's: it draws no group dialog).
pub const GROUP_PICK: [f32; 2] = [4.0, 12.0];
/// The width of the Add dialog (`dialog("Add a repository", 600, …)`).
pub const ADD_WIDTH: f32 = 600.0;
/// The height of a row of the Add dialog's found repositories (`Found / …`'s `height: 52`).
pub const ADD_ROW: f32 = 52.0;
/// The room at either side of a found repository's row, and the gap between its checkbox, its words and its note
/// at the right (`Found / …`'s `padding: [0, 12]`, `gap: 12`).
pub const ADD_ROW_PAD: f32 = 12.0;
/// The gap between a found repository's name and its description (`Text`'s `gap: 2`).
pub const ADD_TEXT_GAP: f32 = 2.0;
/// How strong the checkbox of an installed row of the found repositories is drawn, which takes no click
/// (`opacity: 0.5`).
pub const ADD_INSTALLED_BOX_OPACITY: f32 = 0.5;
/// How strong the name and the "Installed" of an installed row are, of the text colour, in the light mode. The
/// design's `opacity: 0.5`, raised to the least that holds 4.5 everywhere (the controller's ruling of 2026-11-26).
pub const ADD_INSTALLED_INK_LIGHT: f32 = 0.57;
/// How strong the description of an installed row is, of the quiet colour, in the light mode.
pub const ADD_INSTALLED_QUIET_LIGHT: f32 = 0.77;
/// How strong the name and the "Installed" of an installed row are, of the text colour, in the dark mode.
pub const ADD_INSTALLED_INK_DARK: f32 = 0.86;
/// How strong the description of an installed row is, of the quiet colour, in the dark mode.
pub const ADD_INSTALLED_QUIET_DARK: f32 = 0.86;
/// The fewest rows of the found repositories the Add dialog keeps in view while its folder field shows no rows (the
/// plan's own: the design draws five).
pub const ADD_LEAST_ROWS: usize = 3;
/// The height of the line under the Add dialog's search field that says what the core is doing (the plan's own:
/// the design draws none; room for its Retry button).
pub const ADD_STATUS_ROW: f32 = 28.0;
/// The gap above and below that line (the plan's own, a sixth of a dialog's gap: with the line that names the
/// hidden ticks, three rows still fit the smallest window).
pub const ADD_STATUS_GAP: f32 = 4.0;
/// The gap between "Clone into", the folder field and the note under it (`Folder`'s `gap: 8`); also the gap
/// between the line that names the ticked repositories the search hides and the buttons.
pub const ADD_FOLDER_GAP: f32 = 8.0;
/// The room between the found repositories' box and the highlighted row's fill at either side, and that fill's
/// corner radius (the group dialog's, `GROUP_PICK`: the design draws no highlight).
pub const ADD_PICK: [f32; 2] = GROUP_PICK;
/// The width of the folder dialog (Import and Move): the Add dialog's, whose parts it is built from, and the design's
/// Move dialog's (`dialog("Move", 600, …)`; the design draws no Import dialog).
pub const FOLDER_DIALOG_WIDTH: f32 = ADD_WIDTH;
/// The most lines the folder dialog's line under its field wraps to (the core's reason, a move's hint): a long path in
/// a reason keeps its end.
pub const UNDER_LINES: usize = 3;
/// The most rows the Move dialog's "What moves" shows before it scrolls.
pub const MOVE_ROWS: usize = 6;
/// The fewest rows the Move dialog's "What moves" keeps (all, when fewer) while the window is short: the folder
/// field's rows give up their room first.
pub const MOVE_LEAST_ROWS: usize = 3;
/// The width the Move dialog's note on another drive wraps at (the design's `fixed(…, 420, …)`).
pub const MOVE_NOTE_WIDTH: f32 = 420.0;
/// The line height of the Move dialog's note on another drive, as a share of its size (the design's
/// `lineHeight: 1.4`).
pub const MOVE_NOTE_LINE_HEIGHT: f32 = 1.4;
/// The width of the Push dialog (`dialog("Push", 600, …)`).
pub const PUSH_DIALOG_WIDTH: f32 = 600.0;
/// The gap between "Address of the new repository", the address field and the line under it (`Address`'s `gap: 8`).
pub const PUSH_ADDRESS_GAP: f32 = 8.0;
/// The room above and below a fact's words in the Push dialog's "What happens": a fact of one line is a list row
/// tall (`Fact`'s `height: 36`), one that wraps grows by its lines.
pub const PUSH_FACT_PAD: f32 = 8.0;
/// The size of the spinner on the Add dialog's status line while the core looks something up (the plan's own: a
/// small icon's, `ICON_SMALL`'s width less the arc's stroke).
pub const SPINNER: f32 = 12.0;
/// The width of the spinner's arc.
pub const SPINNER_STROKE: f32 = 1.5;
/// How much of a full turn the spinner's arc covers.
pub const SPINNER_SWEEP: f32 = 0.75;
/// How many straight pieces the spinner's arc is drawn with.
pub const SPINNER_STEPS: u16 = 24;
/// How long the spinner takes for one turn, in seconds.
pub const SPINNER_TURN: f64 = 1.0;
/// How often the spinner is drawn again while it turns.
pub const SPINNER_FRAME: Duration = Duration::from_millis(33);
/// The gap between the spinner and the status line's words.
pub const SPINNER_GAP: f32 = 8.0;
/// The width of the ring around the control of an overlay that Enter and Space act on (not the design's: it has
/// no keyboard; the accent, as egui marks its focus).
pub const FOCUS_RING: f32 = 2.0;
/// The gap between that ring and its control, so the ring shows on an accent fill too.
pub const FOCUS_GAP: f32 = 2.0;

/// The value of [`RULE_WIDTH`].
#[expect(clippy::cast_precision_loss, reason = "RULE_HALF is a few commits, far below what an f32 holds exactly")]
const fn rule_width() -> f32 {
    (2 * RULE_HALF) as f32 * RULE_UNIT + RULE_ZERO_WIDTH
}

// Tuning, not design: how fine, how long and when the window makes and keeps its pictures. These set the work
// and memory the glass takes, not the design's look.

/// How many pixels of a blurred picture one standard deviation of its blur spans: a blurred picture holds no
/// detail finer than its blur, so it is made only this fine.
pub const COARSE_SIGMA_PX: f32 = 4.0;
/// The fewest pixels a coarse picture has along the window's shorter side.
pub const COARSE_LEAST_PIXELS: f32 = 8.0;
/// How many straight pieces a quarter of the panel's round corner is cut into where a row's fill or line is cut
/// to the panel's outline: their corners lie on the round, so the fill stays inside it.
pub const CORNER_STEPS: u16 = 8;
/// How much wider than the panel's own the round is that a row's fill or line is cut to at the panel's corners, in
/// points: egui paints the panel's round as straight pieces too and softens its edge, and the fill must not reach
/// past what it paints.
pub const CORNER_SLACK: f32 = 0.5;
/// A pane's picture that no frame has wanted for this many frames is dropped.
pub const PICTURE_KEEP_FRAMES: u64 = 120;
/// How many frames a `--screenshot` run draws before it takes the picture (fonts, layout and the glass have
/// settled by then).
pub const FRAMES_BEFORE_SHOT: u64 = 120;
/// How long the window must be left alone after a resize before its glass is redrawn at the new size.
pub const RESIZE_SETTLE: Duration = Duration::from_millis(150);
/// How long the window may sleep between two looks at the core's workers while the core is busy.
pub const TICK_BUSY: Duration = Duration::from_millis(50);
/// How long the window may sleep between two looks at the core while it is idle (the core's own timers, such as
/// the automatic pull of watched repositories, still run on these looks).
pub const TICK_IDLE: Duration = Duration::from_secs(1);

/// Whether the window is drawn light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Dark text on light glass.
    Light,
    /// Light text on dark glass.
    Dark,
}

impl Mode {
    /// The mode the theme setting comes to, `system_dark` being what the system asks for.
    #[must_use]
    pub fn of(theme: Theme, system_dark: bool) -> Mode {
        match theme {
            Theme::Dark => Mode::Dark,
            Theme::System if system_dark => Mode::Dark,
            Theme::Light | Theme::System => Mode::Light,
        }
    }
}

/// The window's colours in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Text and lines on glass.
    pub on_glass: Color32,
    /// Text on glass that is dark in both modes.
    pub on_dark_glass: Color32,
    /// Secondary text on glass: `on_glass` at [`quiet_opacity`].
    pub quiet: Color32,
    /// The fill of a selected row.
    pub selection: Color32,
    /// The accent colour.
    pub accent: Color32,
    /// The filled part of a progress bar, on its track: the accent in the light mode; in the dark mode a lighter blue
    /// of the accent's hue, the least step that holds 3.0 against the track on every backdrop, also where the
    /// track lies on a selected row's fill (the user's choice of 2026-11-26).
    pub progress: Color32,
    /// The ring around the control with the keyboard's focus: the accent in the light mode; in the dark mode a
    /// lighter blue of the accent's hue, the least step that holds 3.0 against a dialog's glass on every backdrop.
    pub focus: Color32,
    /// The fill of an input field.
    pub field: Color32,
    /// What is laid over the window behind a dialog.
    pub scrim: Color32,
    /// Text that warns.
    pub warning: Color32,
    /// Text that reports a failure.
    pub danger: Color32,
    /// The fill behind a failure.
    pub danger_tint: Color32,
    /// Text that reports success.
    pub ok: Color32,
    /// The colour of the veil laid over the backdrop.
    pub veil: Color32,
    /// The name and the "Installed" of an installed row of the Add dialog: set back, still readable.
    pub installed: Color32,
    /// The description of an installed row of the Add dialog.
    pub installed_quiet: Color32,
}

/// The window's colours in `mode`.
#[must_use]
pub fn palette(mode: Mode) -> Palette {
    let rgb = Color32::from_rgb;
    let rgba = Color32::from_rgba_unmultiplied;
    match mode {
        Mode::Light => Palette {
            on_glass: rgb(0x00, 0x00, 0x00),
            on_dark_glass: Color32::WHITE,
            quiet: rgb(0x00, 0x00, 0x00).gamma_multiply(quiet_opacity(mode)),
            selection: rgba(0, 0, 0, 0x1f),
            accent: rgb(0x0a, 0x47, 0xa3),
            progress: rgb(0x0a, 0x47, 0xa3),
            focus: rgb(0x0a, 0x47, 0xa3),
            field: rgba(255, 255, 255, 0x99),
            scrim: rgba(0, 0, 0, 0x33),
            warning: rgb(0x6e, 0x3a, 0x00),
            // Darkened from #a0221b, hue kept, the least that holds 4.5 everywhere it is used (the user's choice of
            // 2026-11-26).
            danger: rgb(0x83, 0x1c, 0x16),
            danger_tint: rgba(0xb3, 0x26, 0x1e, 0x1a),
            ok: rgb(0x1c, 0x6b, 0x3a),
            veil: rgb(0xff, 0xff, 0xff),
            installed: rgb(0x00, 0x00, 0x00).gamma_multiply(ADD_INSTALLED_INK_LIGHT),
            installed_quiet: rgb(0x00, 0x00, 0x00).gamma_multiply(quiet_opacity(mode) * ADD_INSTALLED_QUIET_LIGHT),
        },
        Mode::Dark => Palette {
            on_glass: rgb(0xff, 0xff, 0xff),
            on_dark_glass: Color32::WHITE,
            quiet: rgb(0xff, 0xff, 0xff).gamma_multiply(quiet_opacity(mode)),
            selection: rgba(255, 255, 255, 0x24),
            accent: rgb(0x2a, 0x66, 0xd6),
            progress: rgb(0xc7, 0xd7, 0xf4),
            focus: rgb(0x48, 0x7c, 0xdc),
            field: rgba(255, 255, 255, 0x1a),
            scrim: rgba(0, 0, 0, 0x73),
            warning: rgb(0xfa, 0xe1, 0xb0),
            danger: rgb(0xff, 0xc8, 0xc1),
            danger_tint: rgba(0x6b, 0x1d, 0x18, 0xcc),
            ok: rgb(0x77, 0xdb, 0xa0),
            veil: rgb(0x06, 0x0d, 0x18),
            installed: rgb(0xff, 0xff, 0xff).gamma_multiply(ADD_INSTALLED_INK_DARK),
            installed_quiet: rgb(0xff, 0xff, 0xff).gamma_multiply(quiet_opacity(mode) * ADD_INSTALLED_QUIET_DARK),
        },
    }
}

/// The fill of a selected row in `mode`: the accent colour at [`SELECTED_OPACITY_LIGHT`] or
/// [`SELECTED_OPACITY_DARK`].
#[must_use]
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "an opacity between 0 and 1 times 255 rounds to 0..=255")]
pub fn selected_fill(mode: Mode) -> Color32 {
    let opacity = match mode {
        Mode::Light => SELECTED_OPACITY_LIGHT,
        Mode::Dark => SELECTED_OPACITY_DARK,
    };
    let accent = palette(mode).accent;
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (255.0 * opacity).round() as u8)
}

/// The veil's two strengths over the backdrop `backdrop`: the first in the lower part of the window (its bottom
/// [`VEIL_KNEE`] of the height), the second at the very top; between them it rises evenly ([`veil_at`]). A key
/// that names no backdrop gets the values of `"lake"`.
#[must_use]
pub fn veil(backdrop: &str, mode: Mode) -> [f32; 2] {
    match (backdrop, mode) {
        ("midday", Mode::Dark) => [0.45, 0.72],
        ("dusk", Mode::Light) => [0.2, 0.62],
        ("dusk", Mode::Dark) => [0.2, 0.5],
        ("night", Mode::Dark) => [0.1, 0.35],
        // The lake, the forest and the golden hour in the dark mode, and the night in the light one.
        ("night", Mode::Light) | (_, Mode::Dark) => [0.5, 0.82],
        (_, Mode::Light) => [0.0, 0.0],
    }
}

/// The veil's strength `down` the window (0 at the top, 1 at the bottom) when its two numbers are `strengths`
/// (the strength in the lower part, the strength at the top): the lower one from the bottom up to
/// [`VEIL_KNEE`] of the height, rising evenly from there to the higher one at the very top.
#[must_use]
pub fn veil_at(strengths: [f32; 2], down: f32) -> f32 {
    let rise = ((1.0 - down - VEIL_KNEE) / (1.0 - VEIL_KNEE)).clamp(0.0, 1.0);
    (strengths[0] + (strengths[1] - strengths[0]) * rise).clamp(0.0, 1.0)
}

/// The veil's strength over the backdrop `backdrop` in `mode`, `down` the window (0 at the top, 1 at the bottom).
#[must_use]
pub fn veil_strength(backdrop: &str, mode: Mode, down: f32) -> f32 {
    veil_at(veil(backdrop, mode), down)
}

/// A kind of pane, each with its own material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    /// The sidebar: glass.
    Sidebar,
    /// A capsule floating over the content: glass.
    Capsule,
    /// The status bar: thin glass.
    StatusBar,
    /// A panel holding content: frosted, not glass.
    Panel,
    /// A dialog over the window: glass, the sidebar's.
    Dialog,
    /// A menu over the window: glass, the sidebar's.
    Menu,
}

impl Surface {
    /// Every surface.
    pub const ALL: [Surface; 6] = [Surface::Sidebar, Surface::Capsule, Surface::StatusBar, Surface::Panel, Surface::Dialog, Surface::Menu];

    /// Whether the surface is glass, whose panes are baked into pictures of their own; a frosted one is drawn
    /// from the baker's sheet instead ([`crate::frost`]).
    #[must_use]
    pub fn is_glass(self) -> bool {
        match self {
            Surface::Sidebar | Surface::Capsule | Surface::StatusBar | Surface::Dialog | Surface::Menu => true,
            Surface::Panel => false,
        }
    }
}

/// The material of `surface` in `mode`, as the design document writes it.
fn material_value(surface: Surface, mode: Mode) -> Value {
    let appearance = match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    match surface {
        // The design draws its dialogs and menus with the capsules' liquid glass at the depth "panel": the sidebar's.
        Surface::Sidebar | Surface::Dialog | Surface::Menu => json!({ "type": "liquid_glass", "depth": "panel", "appearance": appearance }),
        Surface::Capsule => json!({ "type": "liquid_glass", "appearance": appearance }),
        Surface::StatusBar => json!({ "type": "liquid_glass", "depth": "bar", "appearance": appearance }),
        Surface::Panel => json!({ "type": "frosted", "thickness": "thick", "appearance": appearance }),
    }
}

/// The numbers to draw `surface` with in `mode`, as a pane of `size`.
#[must_use]
pub fn params(surface: Surface, mode: Mode, size: (f32, f32)) -> Params {
    // A constant that fails to read is a programming error (a test covers them); draw frosted rather than panic.
    let material = Material::read(&material_value(surface, mode)).or_else(|| Material::read(&material::fresh(false))).unwrap_or_else(|| Material {
        kind: material::Kind::Frosted(material::Thickness::Regular),
        appearance: material::Appearance::Auto,
        tint: None,
        depth: None,
        advanced: serde_json::Map::new(),
    });
    let no_number = |_: &str| None;
    let no_colour = |_: &str| None;
    material.resolve(size, mode == Mode::Dark, &Lookup { number: &no_number, color: &no_colour })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colours_are_the_quill_documents() {
        let (light, dark) = (palette(Mode::Light), palette(Mode::Dark));
        assert_eq!(light.on_glass, Color32::from_rgb(0x00, 0x00, 0x00));
        assert_eq!(dark.on_glass, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(light.accent, Color32::from_rgb(0x0a, 0x47, 0xa3));
        assert_eq!(dark.accent, Color32::from_rgb(0x2a, 0x66, 0xd6));
        assert_eq!(light.warning, Color32::from_rgb(0x6e, 0x3a, 0x00));
        assert_eq!(dark.warning, Color32::from_rgb(0xfa, 0xe1, 0xb0));
        assert_eq!(light.danger, Color32::from_rgb(0x83, 0x1c, 0x16));
        assert_eq!((light.progress, light.focus), (light.accent, light.accent));
        assert_eq!(dark.progress, Color32::from_rgb(0xc7, 0xd7, 0xf4));
        assert_eq!(dark.focus, Color32::from_rgb(0x48, 0x7c, 0xdc));
        assert_eq!(dark.danger, Color32::from_rgb(0xff, 0xc8, 0xc1));
        assert_eq!(light.ok, Color32::from_rgb(0x1c, 0x6b, 0x3a));
        assert_eq!(dark.ok, Color32::from_rgb(0x77, 0xdb, 0xa0));
        assert_eq!(light.veil, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(dark.veil, Color32::from_rgb(0x06, 0x0d, 0x18));
        assert_eq!(light.selection, Color32::from_rgba_unmultiplied(0, 0, 0, 0x1f));
        assert_eq!(dark.selection, Color32::from_rgba_unmultiplied(255, 255, 255, 0x24));
        assert_eq!(light.field, Color32::from_rgba_unmultiplied(255, 255, 255, 0x99));
        assert_eq!(dark.field, Color32::from_rgba_unmultiplied(255, 255, 255, 0x1a));
        assert_eq!(light.scrim, Color32::from_rgba_unmultiplied(0, 0, 0, 0x33));
        assert_eq!(dark.scrim, Color32::from_rgba_unmultiplied(0, 0, 0, 0x73));
        assert_eq!(light.danger_tint, Color32::from_rgba_unmultiplied(0xb3, 0x26, 0x1e, 0x1a));
        assert_eq!(dark.danger_tint, Color32::from_rgba_unmultiplied(0x6b, 0x1d, 0x18, 0xcc));
        assert_eq!(light.on_dark_glass, Color32::WHITE);
        assert_eq!(light.quiet, Color32::from_rgba_unmultiplied(0, 0, 0, 189));
        assert_eq!(dark.quiet, Color32::from_rgba_unmultiplied(255, 255, 255, 227));
    }

    #[test]
    fn every_backdrop_has_its_veil_in_both_themes() {
        let want = [
            ("lake", [0.0, 0.0], [0.5, 0.82]),
            ("golden", [0.0, 0.0], [0.5, 0.82]),
            ("midday", [0.0, 0.0], [0.45, 0.72]),
            ("forest", [0.0, 0.0], [0.5, 0.82]),
            ("dusk", [0.2, 0.62], [0.2, 0.5]),
            ("night", [0.5, 0.82], [0.1, 0.35]),
        ];
        for (key, light, dark) in want {
            assert_eq!(veil(key, Mode::Light), light, "{key}");
            assert_eq!(veil(key, Mode::Dark), dark, "{key}");
        }
        assert_eq!(veil("moon", Mode::Light), veil("lake", Mode::Light));
        assert_eq!(veil("moon", Mode::Dark), veil("lake", Mode::Dark)); // an unknown key is the default backdrop
    }

    #[test]
    fn the_surfaces_are_the_designs_materials() {
        let size = (256.0, 876.0);
        assert!(params(Surface::Sidebar, Mode::Light, size).glass);
        assert!(params(Surface::Capsule, Mode::Dark, (120.0, 36.0)).glass);
        assert!(params(Surface::StatusBar, Mode::Light, (1148.0, 32.0)).glass);
        assert!(!params(Surface::Panel, Mode::Light, (1148.0, 600.0)).glass);
        assert!(params(Surface::Sidebar, Mode::Dark, size).dark);
        assert!(!params(Surface::Sidebar, Mode::Light, size).dark);
        for surface in Surface::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                assert!(Material::read(&material_value(surface, mode)).is_some(), "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn a_surface_is_baked_exactly_when_its_material_is_glass() {
        for surface in Surface::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                assert_eq!(surface.is_glass(), params(surface, mode, (300.0, 200.0)).glass, "{surface:?} {mode:?}");
            }
        }
        assert!(!Surface::Panel.is_glass());
    }

    #[test]
    fn each_surface_names_its_material_words_exactly() {
        use material::{Appearance, Depth, Kind, Thickness, Variant};
        let glass = Kind::Glass(Variant::Regular);
        let want = [
            (Surface::Sidebar, glass, Some(Depth::Panel)),
            (Surface::Capsule, glass, None),
            (Surface::StatusBar, glass, Some(Depth::Bar)),
            (Surface::Panel, Kind::Frosted(Thickness::Thick), None),
            (Surface::Dialog, glass, Some(Depth::Panel)),
            (Surface::Menu, glass, Some(Depth::Panel)),
        ];
        for (surface, kind, depth) in want {
            for (mode, appearance) in [(Mode::Light, Appearance::Light), (Mode::Dark, Appearance::Dark)] {
                let Some(read) = Material::read(&material_value(surface, mode)) else {
                    panic!("{surface:?} {mode:?} does not read");
                };
                assert_eq!(read.kind, kind, "{surface:?} {mode:?}");
                assert_eq!(read.depth, depth, "{surface:?} {mode:?}");
                assert_eq!(read.appearance, appearance, "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn the_mode_follows_the_theme_setting_or_the_system() {
        assert_eq!(Mode::of(Theme::Light, true), Mode::Light);
        assert_eq!(Mode::of(Theme::Dark, false), Mode::Dark);
        assert_eq!(Mode::of(Theme::System, true), Mode::Dark);
        assert_eq!(Mode::of(Theme::System, false), Mode::Light);
    }

    #[test]
    fn the_table_is_the_designs() {
        let want = [
            ("ROW_HEIGHT", ROW_HEIGHT, 44.0),
            ("HEAD_HEIGHT", HEAD_HEIGHT, 40.0),
            ("GROUP_HEIGHT", GROUP_HEIGHT, 36.0),
            ("ROW_PAD_LEFT", ROW_PAD_LEFT, 12.0),
            ("ROW_PAD_RIGHT", ROW_PAD_RIGHT, 8.0),
            ("COLUMN_GAP", COLUMN_GAP, 12.0),
            ("COLUMN_CHECK", COLUMN_CHECK, 24.0),
            ("COLUMN_FOLDER", COLUMN_FOLDER, 176.0),
            ("COLUMN_BRANCH", COLUMN_BRANCH, 136.0),
            ("COLUMN_CHANGES", COLUMN_CHANGES, 104.0),
            ("COLUMN_PULLS", COLUMN_PULLS, 134.0),
            ("COLUMN_ORIGIN", COLUMN_ORIGIN, 288.0),
            ("COLUMN_ACTION", COLUMN_ACTION, 32.0),
            ("COLUMN_NAME_LEAST", COLUMN_NAME_LEAST, 120.0),
            ("RULE_UNIT", RULE_UNIT, 8.0),
            ("RULE_HEIGHT", RULE_HEIGHT, 16.0),
            ("RULE_BAR", RULE_BAR, 6.0),
            ("RULE_BAR_TOP", RULE_BAR_TOP, 2.0),
            ("RULE_TICK", RULE_TICK, 4.0),
            ("RULE_TICK_FIFTH", RULE_TICK_FIFTH, 7.0),
            ("RULE_LABEL_GAP", RULE_LABEL_GAP, 12.0),
            ("RULE_TICK_OPACITY", RULE_TICK_OPACITY, 0.28),
            ("RULE_FIFTH_OPACITY", RULE_FIFTH_OPACITY, 0.5),
            ("CAPSULE_PAD", CAPSULE_PAD, 4.0),
            ("CAPSULE_GAP", CAPSULE_GAP, 2.0),
            ("ACTION_HEIGHT", ACTION_HEIGHT, 28.0),
            ("ACTION_GAP", ACTION_GAP, 6.0),
            ("FIELD_WIDTH", FIELD_WIDTH, 280.0),
            ("FIELD_PAD", FIELD_PAD, 12.0),
            ("CHECKBOX", CHECKBOX, 16.0),
            ("TOOLBAR_GAP", TOOLBAR_GAP, 8.0),
            ("LINE_OPACITY", LINE_OPACITY, 0.10),
            ("HEAD_FILL_OPACITY", HEAD_FILL_OPACITY, 0.06),
            ("GROUP_FILL_OPACITY", GROUP_FILL_OPACITY, 0.03),
            ("HOVER_OPACITY", HOVER_OPACITY, 0.05),
            ("CHECKBOX_STROKE_OPACITY", CHECKBOX_STROKE_OPACITY, 0.4),
            ("SELECTED_OPACITY_LIGHT", SELECTED_OPACITY_LIGHT, 0.14),
            ("SELECTED_OPACITY_DARK", SELECTED_OPACITY_DARK, 0.32),
            ("GROUP_ROW_GAP", GROUP_ROW_GAP, 8.0),
            ("HAIRLINE", HAIRLINE, 1.0),
            ("RULE_ZERO_WIDTH", RULE_ZERO_WIDTH, 1.0),
            ("ICON_SMALL", ICON_SMALL, 14.0),
            ("CHECK_MARK", CHECK_MARK, 12.0),
            ("NAME_ICON_GAP", NAME_ICON_GAP, 8.0),
            ("CELL_ICON_GAP", CELL_ICON_GAP, 6.0),
            ("PULL_ICON_GAP", PULL_ICON_GAP, 4.0),
            ("PULL_GAP", PULL_GAP, 8.0),
            ("RULE_LEGEND_GAP", RULE_LEGEND_GAP, 8.0),
            ("RULE_BREAK_GAP", RULE_BREAK_GAP, 6.0),
            ("RULE_BREAK_END", RULE_BREAK_END, 14.0),
            ("PROGRESS_HEIGHT", PROGRESS_HEIGHT, 6.0),
            ("PROGRESS_LEAST", PROGRESS_LEAST, 6.0),
            ("PROGRESS_TRACK_OPACITY", PROGRESS_TRACK_OPACITY, 0.12),
        ];
        for (constant, got, expected) in want {
            assert!((got - expected).abs() < f32::EPSILON, "{constant} is {got}, the design has {expected}");
        }
        assert_eq!(RULE_HALF, 10);
        assert_eq!(ACTION_PAD, [8.0, 10.0]);
        assert_eq!(LABEL_PAD, [10.0, 8.0]);
        for (constant, got, expected) in [
            ("BADGE_HEIGHT", BADGE_HEIGHT, 20.0),
            ("BADGE_PAD", BADGE_PAD, 6.0),
            ("BADGE_INSET", BADGE_INSET, 4.0),
            ("STATUS_ICON", STATUS_ICON, 12.0),
            ("STATUS_ICON_GAP", STATUS_ICON_GAP, 6.0),
            ("STATUS_GAP", STATUS_GAP, 24.0),
        ] {
            assert!((got - expected).abs() < f32::EPSILON, "{constant} is {got}, the design has {expected}");
        }
    }

    #[test]
    fn the_overlays_are_the_designs() {
        let want = [
            ("DIALOG_RADIUS", DIALOG_RADIUS, 20.0),
            ("DIALOG_PAD", DIALOG_PAD, 24.0),
            ("DIALOG_GAP", DIALOG_GAP, 24.0),
            ("DIALOG_MARGIN", DIALOG_MARGIN, 24.0),
            ("DIALOG_TITLE", DIALOG_TITLE, 16.0),
            ("INSET_RADIUS", INSET_RADIUS, 16.0),
            ("BUTTON_HEIGHT", BUTTON_HEIGHT, 36.0),
            ("BUTTON_RADIUS", BUTTON_RADIUS, 18.0),
            ("BUTTON_PAD", BUTTON_PAD, 16.0),
            ("BUTTON_GAP", BUTTON_GAP, 8.0),
            ("MENU_WIDTH", MENU_WIDTH, 256.0),
            ("MENU_RADIUS", MENU_RADIUS, 16.0),
            ("MENU_PAD", MENU_PAD, 4.0),
            ("MENU_ROW", MENU_ROW, 32.0),
            ("MENU_ROW_RADIUS", MENU_ROW_RADIUS, 12.0),
            ("MENU_LINE", MENU_LINE, 9.0),
            ("MENU_GAP", MENU_GAP, 4.0),
            ("KEY_TEXT", KEY_TEXT, 12.0),
            ("MENU_ROW_PAD", MENU_ROW_PAD, 8.0),
            ("MENU_ROW_GAP", MENU_ROW_GAP, 8.0),
            ("MENU_KEY_WIDTH", MENU_KEY_WIDTH, 16.0),
            ("MENU_LINE_OPACITY", MENU_LINE_OPACITY, 0.12),
            ("SUBMENU_WIDTH", SUBMENU_WIDTH, 264.0),
            ("SUBMENU_GAP", SUBMENU_GAP, 8.0),
            ("MENU_CHEVRON", MENU_CHEVRON, 16.0),
            ("DOTS_HEIGHT", DOTS_HEIGHT, 32.0),
            ("DOTS_RADIUS", DOTS_RADIUS, 16.0),
            ("HEADER_GAP", HEADER_GAP, 4.0),
            ("HEADER_LINE_HEIGHT", HEADER_LINE_HEIGHT, 1.45),
            ("PROGRESS_WIDTH", PROGRESS_WIDTH, 640.0),
            ("TASK_HEIGHT", TASK_HEIGHT, 44.0),
            ("TASK_PAD", TASK_PAD, 12.0),
            ("TASK_NAME", TASK_NAME, 120.0),
            ("TASK_BAR", TASK_BAR, 200.0),
            ("OUTPUT_PAD", OUTPUT_PAD, 12.0),
            ("OUTPUT_GAP", OUTPUT_GAP, 8.0),
            ("OUTPUT_LINE", OUTPUT_LINE, 14.0),
            ("OUTPUT_TEXT", OUTPUT_TEXT, 12.0),
            ("DIALOG_WIDTH", DIALOG_WIDTH, 480.0),
            ("LIST_ROW", LIST_ROW, 36.0),
            ("LIST_GAP", LIST_GAP, 8.0),
            ("DIALOG_FIELD_HEIGHT", DIALOG_FIELD_HEIGHT, 36.0),
            ("DIALOG_FIELD_RADIUS", DIALOG_FIELD_RADIUS, 18.0),
            ("FIELD_EDGE_OPACITY", FIELD_EDGE_OPACITY, 0.16),
            ("FOLDER_ROW", FOLDER_ROW, 32.0),
        ];
        assert_eq!(FOLDER_ROWS, 6);
        assert_eq!(LIST_PAD, [4.0, 12.0]);
        for (constant, got, expected) in want {
            assert!((got - expected).abs() < f32::EPSILON, "{constant} is {got}, the design has {expected}");
        }
        assert!((BUTTON_RADIUS * 2.0 - BUTTON_HEIGHT).abs() < f32::EPSILON, "a button's ends are round");
    }

    #[test]
    fn dialogs_and_menus_are_the_sidebars_glass() {
        for surface in [Surface::Dialog, Surface::Menu] {
            assert!(surface.is_glass());
            for mode in [Mode::Light, Mode::Dark] {
                assert_eq!(material_value(surface, mode), material_value(Surface::Sidebar, mode), "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn the_rule_is_twice_its_half_in_commits_and_the_middle_line() {
        let commits = f32::from(u16::try_from(2 * RULE_HALF).unwrap());
        assert!((RULE_WIDTH - (commits * RULE_UNIT + 1.0)).abs() < f32::EPSILON);
        assert!((RULE_WIDTH - 161.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_columns_fit_the_design_width() {
        let panel = 1148.0;
        let fixed = COLUMN_CHECK + COLUMN_FOLDER + COLUMN_BRANCH + COLUMN_CHANGES + COLUMN_PULLS + COLUMN_ORIGIN + COLUMN_ACTION;
        let columns = 8.0; // the seven fixed ones and the name
        let name = panel - fixed - (columns - 1.0) * COLUMN_GAP - ROW_PAD_LEFT - ROW_PAD_RIGHT;
        assert!(name >= COLUMN_NAME_LEAST, "the name gets {name} of the {panel} points, it needs {COLUMN_NAME_LEAST}");
    }

    #[test]
    fn the_selected_fill_is_the_accent_at_its_two_opacities() {
        for (mode, alpha) in [(Mode::Light, 36), (Mode::Dark, 82)] {
            let a = palette(mode).accent;
            assert_eq!(selected_fill(mode), Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), alpha), "{mode:?}");
        }
    }
}
