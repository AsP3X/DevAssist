//! The Push dialog ([`super::Which::Push`]), for the core's `Mode::Service`: it pushes a complete copy of one
//! repository to another Git service, as the design's `15-push` draws it, 600 wide: the header, "Address of the new
//! repository" over the address field (the link icon, a hint while it is empty), the line under it (that Git signs in,
//! or the core's reason in the danger colour when it refused the address, wrapped, never cut), a box with the core's
//! facts (`Repos::service_facts`), each with a check (it goes over or is done) or a cross (it stays as it is or is not
//! done), and the buttons: Cancel and the main "Push".
//!
//! The field has the focus when the dialog opens. Its text is the core's (`Repos::set_service_input` as it is typed);
//! when the core changes it, the field follows with the cursor at its end. The facts are asked for when the dialog
//! opens and again only when the text changes (the last one names the remote the address is kept as). They take the
//! height their wrapped sentences need; in a window too short for that, the box scrolls with the wheel and the buttons
//! stay in view.
//!
//! "Push" is off while the address is empty. Enter in the field or with nothing focused, and "Push", ask the core to push
//! (`Repos::accept_service`): when it refuses the address, the dialog stays with the core's reason under the field
//! until the text changes; else the dialog closes and the push is the core's run, whose progress dialog the bridge
//! opens. A push to another service cannot be undone there: this dialog is its confirmation, so nothing but a fresh
//! press made in it pushes (the overlay's rules: not the Enter or the click that opened it, not a held Enter). Esc,
//! Cancel and the scrim close it through the core (`Repos::close_service`).

use std::sync::Arc;

use devassist_core::repos::state::{Fact, Repos};
use egui::text::LayoutJob;
use egui::{Align2, Color32, FontFamily, Galley, Id, Key, Painter, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, folder, inset, main_id, take_press};
use crate::assets::{self, Icon};
use crate::overlay::{self, Kind};
use crate::theme::{
    BUTTON_HEIGHT, DIALOG_FIELD_HEIGHT, DIALOG_GAP, DIALOG_MARGIN, DIALOG_PAD, FIELD_EDGE_OPACITY, HAIRLINE, HEADER_LINE_HEIGHT, ICON, LINE_OPACITY,
    LIST_GAP, LIST_PAD, LIST_ROW, PUSH_ADDRESS_GAP, PUSH_DIALOG_WIDTH, PUSH_FACT_PAD, Palette, TEXT,
};
use crate::widgets::{self, font};

/// The quiet line under the title.
const LINE: &str = "A complete copy on GitLab, Codeberg, Bitbucket or a server of your own. Make an empty repository there first, without a README.";
/// The words over the field.
const LABEL: &str = "Address of the new repository";
/// What the field says while it is empty.
const HINT: &str = "https://gitlab.com/you/name.git or git@codeberg.org:you/name.git";
/// The line under the field, in the quiet colour, while the core has refused nothing.
const NOTE: &str = "Signs in with what Git already has for that host. DevAssist asks for no password.";
/// The button that closes the dialog.
const CANCEL: &str = "Cancel";
/// The main button.
const PUSH: &str = "Push";

/// The title for the repository `name`.
fn title(name: &str) -> String {
    format!("Push {name} to another service")
}

/// Whether an address `text` can be asked for: it is not empty.
fn pushable(text: &str) -> bool {
    !text.trim().is_empty()
}

/// The id of the address field, which has the focus when the dialog opens.
pub(super) fn field_id() -> Id {
    Id::unique("dialog-push-field")
}

/// The id of the Cancel button.
fn cancel_id() -> Id {
    Id::unique("dialog-push-cancel")
}

/// What the Push dialog keeps of its own from one frame to the next, made when it opens ([`State::new`]).
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// The field's text as it stood last: the core's, which the dialog follows when the core changes it.
    text: String,
    /// The core's facts for it (`Repos::service_facts`), asked for again only when it changes.
    facts: Vec<Fact>,
}

impl State {
    /// The state of a Push dialog that opens on `repos`: the core's text, and its facts.
    pub(super) fn new(repos: &Repos) -> State {
        let mut state = State::default();
        state.follow(repos, repos.service_prompt.as_ref().map_or_default(|prompt| prompt.input.clone()));
        state
    }

    /// The text is `text` now, the core's: asks the core for its facts again.
    fn follow(&mut self, repos: &Repos, text: String) {
        self.facts = repos.service_facts();
        self.text = text;
    }
}

/// The words of the Push dialog's buttons in `repos`, from the left.
#[cfg(test)]
pub(super) fn said(repos: &Repos) -> Vec<&'static str> {
    if repos.service_prompt.is_some() { vec![CANCEL, PUSH] } else { Vec::new() }
}

/// The line under the field at `places` while it says why the core refused the address of `repos`.
#[cfg(test)]
pub(super) fn reason_box(places: Places, repos: &Repos) -> Option<Rect> {
    repos.service_prompt.as_ref().and_then(|prompt| prompt.error.as_ref()).map(|_| places.under)
}

/// The parts of the Push dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The words over the field.
    pub(super) label: Rect,
    /// The address field.
    pub(super) field: Rect,
    /// The line under the field: the note, or the core's reason.
    pub(super) under: Rect,
    /// The box of the facts.
    pub(super) facts: Rect,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// The height of a line of 14-point text.
fn line_height() -> f32 {
    TEXT * HEADER_LINE_HEIGHT
}

/// Where the Push dialog's parts are in `window`, with a header `header` points tall, a line under the field `under`
/// points tall and facts that want `facts` points: the box takes what they want, as far as the window lets it; the
/// buttons stay inside the pane.
fn push_places(window: Rect, header: f32, under: f32, facts: f32) -> Places {
    let fixed = 2.0 * DIALOG_PAD
        + header
        + DIALOG_GAP
        + line_height()
        + PUSH_ADDRESS_GAP
        + DIALOG_FIELD_HEIGHT
        + PUSH_ADDRESS_GAP
        + under
        + 2.0 * DIALOG_GAP
        + BUTTON_HEIGHT;
    let spare = (window.height() - 2.0 * DIALOG_MARGIN - fixed).max(0.0);
    let room = facts.min(spare);
    let pane = overlay::dialog_rect(window, vec2(PUSH_DIALOG_WIDTH, fixed + room));
    let inside = pane.shrink(DIALOG_PAD);
    let at = |top: f32, height: f32| Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), height));
    let header = at(inside.top(), header);
    let label = at(header.bottom() + DIALOG_GAP, line_height());
    let field = at(label.bottom() + PUSH_ADDRESS_GAP, DIALOG_FIELD_HEIGHT);
    let under = at(field.bottom() + PUSH_ADDRESS_GAP, under);
    let facts = at(under.bottom() + DIALOG_GAP, room);
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    Places { pane, header, label, field, under, facts, buttons }
}

/// How wide a fact's words are in a box `width` points wide: what its room, its icon and the gap leave.
fn fact_width(width: f32) -> f32 {
    (width - 2.0 * LIST_PAD[1] - ICON - LIST_GAP).max(0.0)
}

/// The words of `facts` laid out with `painter`'s faces in `width`: 14-point text at the header's line height,
/// wrapped as far as it takes, never cut.
fn fact_galleys(painter: &Painter, facts: &[Fact], width: f32) -> Vec<Arc<Galley>> {
    facts
        .iter()
        .map(|fact| {
            let mut job = LayoutJob::simple(fact.words.clone(), font(TEXT, FontFamily::Proportional), Color32::PLACEHOLDER, width);
            for section in &mut job.sections {
                section.format.line_height = Some(line_height());
            }
            painter.layout_job(job)
        })
        .collect()
}

/// How tall a fact's row is with its words `galley`: a list row, or more when the words wrap.
fn row_height(galley: &Galley) -> f32 {
    (galley.size().y + 2.0 * PUSH_FACT_PAD).max(LIST_ROW)
}

/// How tall the box of the facts laid out as `galleys` wants to be: the rows and the room above and below them.
fn facts_height(galleys: &[Arc<Galley>]) -> f32 {
    2.0 * LIST_PAD[0] + galleys.iter().map(|galley| row_height(galley)).sum::<f32>()
}

/// Where the Push dialog of `repos` stands in `window`, its header, the line under its field and its facts (as `state`
/// has them) laid out with `painter`'s faces; `None` without the core's prompt.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos, state: &State) -> Option<Plan> {
    let prompt = repos.service_prompt.as_ref()?;
    let width = PUSH_DIALOG_WIDTH - 2.0 * DIALOG_PAD;
    let header = Header::new(painter, &title(&prompt.repo), LINE, width);
    let under = folder::under_galley(painter, prompt.error.as_deref().unwrap_or(NOTE), width).size().y;
    let facts = facts_height(&fact_galleys(painter, &state.facts, fact_width(width)));
    let places = push_places(window, header.height(), under, facts);
    Some(Plan::dialog(header, Parts::Push(places)))
}

/// What a person did in the Push dialog in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The text typed, when it changed.
    typed: Option<String>,
    /// The main button, or Enter in the field.
    accept: bool,
    /// Cancel, Esc or the scrim.
    close: bool,
}

/// The Push dialog at `places`, with `header`, for `repos`'s service question, on the overlay in `ui`; `scrim` is
/// whether the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(mut state) = dialogs.push.take() else { return };
    let Some((input, error)) = repos.service_prompt.as_ref().map(|prompt| (prompt.input.clone(), prompt.error.clone())) else {
        dialogs.push = Some(state);
        return;
    };
    if input != state.text {
        // The core changed the text: the field follows it, typing goes on at its end, and the facts are asked for.
        widgets::put_cursor_at_end(ui.ctx(), field_id(), &input);
        state.follow(repos, input);
    }
    let mut ids = vec![field_id(), cancel_id()];
    if pushable(&state.text) {
        ids.push(main_id());
    }
    overlay::walk(ui, &ids);
    // Enter and Esc are the dialog's, taken before the field sees them: a held key acts once. Enter in the field or
    // with nothing focused does what the main button does; on a button, the button takes it.
    let mut done = Done { close: scrim, ..Done::default() };
    if ui.memory(|memory| memory.has_focus(field_id()) || memory.focused().is_none()) {
        done.accept |= take_press(ui, Key::Enter, None);
    }
    done.close |= take_press(ui, Key::Escape, None);
    header.draw(ui, places.header, palette);
    ui.painter().text(places.label.left_center(), Align2::LEFT_CENTER, LABEL, font(TEXT, assets::medium_family()), palette.on_glass);

    let radius = DIALOG_FIELD_HEIGHT / 2.0;
    ui.painter().rect_filled(places.field, radius, palette.field);
    let edge = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(FIELD_EDGE_OPACITY));
    ui.painter().rect_stroke(places.field, radius, edge, StrokeKind::Inside);
    let mut text = state.text.clone();
    widgets::field(ui, places.field, &mut text, HINT, Icon::Link, palette, field_id());
    if text != state.text {
        done.typed = Some(text);
    }

    // Under the field: the core's refusal while it stands (until the text changes), else the note; wrapped, never cut.
    let (words, colour) = match error.filter(|_| done.typed.is_none()) {
        Some(error) => (error, palette.danger),
        None => (NOTE.to_owned(), palette.quiet),
    };
    ui.painter().galley(places.under.min, folder::under_galley(ui.painter(), &words, places.under.width()), colour);
    facts(ui, places.facts, &state.facts, palette, dialogs);

    let on = pushable(done.typed.as_deref().unwrap_or(&state.text));
    let widths = [widgets::button_width(ui, CANCEL), widgets::button_width(ui, PUSH)];
    if let [cancel, main] = overlay::buttons(places.buttons, &widths).as_slice() {
        done.close |= widgets::button(ui, *cancel, CANCEL, Kind::Plain, palette, cancel_id());
        if pushable(&state.text) {
            done.accept |= widgets::button(ui, *main, PUSH, Kind::Main, palette, main_id());
        } else {
            widgets::button_off(ui, *main, PUSH, palette);
        }
    }
    done.accept &= on;
    act(ui, repos, &mut state, &done);
    dialogs.push = Some(state);
}

/// Draws `facts` in the box at `rect`, scrolled by the wheel: for each, a check (it goes) or a cross (it does not) and
/// its words, wrapped; a line between two.
fn facts(ui: &Ui, rect: Rect, facts: &[Fact], palette: Palette, dialogs: &mut Dialogs) {
    let inset = inset(ui, rect, palette);
    let galleys = fact_galleys(ui.painter(), facts, fact_width(rect.width()));
    let most = (facts_height(&galleys) - rect.height()).max(0.0);
    dialogs.list_scroll = (dialogs.list_scroll - inset.wheel).clamp(0.0, most);
    let (left, right) = (rect.left() + LIST_PAD[1], rect.right() - LIST_PAD[1]);
    let mut top = rect.top() + LIST_PAD[0] - dialogs.list_scroll;
    for (index, (fact, galley)) in facts.iter().zip(galleys).enumerate() {
        let row = Rect::from_min_max(pos2(left, top), pos2(right, top + row_height(&galley)));
        top = row.bottom();
        if !row.intersects(rect) {
            continue;
        }
        if index + 1 != facts.len() {
            let line = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(LINE_OPACITY));
            inset.painter.hline(row.x_range(), row.bottom() - HAIRLINE / 2.0, line);
        }
        let mark = if fact.goes { Icon::Check } else { Icon::X };
        let centre = pos2(left + ICON / 2.0, row.top() + PUSH_FACT_PAD + line_height() / 2.0);
        widgets::draw_icon(&inset.painter, centre, mark, ICON, palette.quiet);
        inset.painter.galley(pos2(left + ICON + LIST_GAP, row.top() + PUSH_FACT_PAD), galley, palette.on_glass);
    }
}

/// Does in `repos` what a person did in the Push dialog in one frame; keeps the dialog's own `state`.
fn act(ui: &Ui, repos: &mut Repos, state: &mut State, done: &Done) {
    if done.close {
        repos.close_service();
        return;
    }
    if let Some(text) = &done.typed {
        repos.set_service_input(text);
        state.follow(repos, text.clone());
    }
    if done.accept {
        repos.accept_service();
        if repos.service_prompt.is_some() {
            // Refused: the reason stands under the field in the next frame, and the field has the keys again.
            ui.memory_mut(|memory| memory.request_focus(field_id()));
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use devassist_core::repos::state::testing::hold_github;
    use devassist_core::repos::state::{HELD, MigrateItem, Mode as CoreMode};
    use egui::{Event, Memory, Modifiers, PointerButton, Pos2};

    use super::super::Which;
    use super::super::testing::{Window, behind, git, until, wheel};
    use super::*;
    use crate::demo::PUSH_ADDRESS;
    use crate::session::Session;
    use crate::theme::{MIN_WINDOW, WINDOW};

    /// The Push dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Push(places) = window.parts() else { panic!("not the Push dialog") };
        places
    }

    fn state(window: &Window) -> &State {
        window.dialogs.push.as_ref().unwrap()
    }

    /// The core's address now.
    fn input(window: &Window) -> String {
        window.repos().service_prompt.as_ref().map_or_default(|prompt| prompt.input.clone())
    }

    /// The core's reason now.
    fn error(window: &Window) -> Option<String> {
        window.repos().service_prompt.as_ref().and_then(|prompt| prompt.error.clone())
    }

    fn key(key: Key, pressed: bool, repeat: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat, modifiers }
    }

    /// A press of `code` typing `text`, and its release.
    fn tap(window: &mut Window, code: Key, text: Option<&str>) {
        let mut press = vec![key(code, true, false, Modifiers::NONE)];
        press.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(press);
        window.frame(vec![key(code, false, false, Modifiers::NONE)]);
    }

    /// Ctrl-A in the field, then `text` typed over it (Backspace for none).
    fn type_over(window: &mut Window, text: &str) {
        window.frame(vec![key(Key::A, true, false, Modifiers::COMMAND)]);
        window.frame(vec![key(Key::A, false, false, Modifiers::COMMAND)]);
        if text.is_empty() {
            tap(window, Key::Backspace, None);
        } else {
            window.frame(vec![Event::Text(text.to_owned())]);
        }
        window.frame(Vec::new());
    }

    /// A click at `at` whose press is at egui's clock `pressed`.
    fn click_at_time(window: &mut Window, at: Pos2, pressed: f64) {
        let button = |down| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: down, modifiers: Modifiers::NONE };
        window.frame_at(vec![Event::PointerMoved(at)], Some(pressed - 0.01));
        window.frame_at(vec![button(true)], Some(pressed));
        window.frame_at(vec![button(false)], Some(pressed + 0.05));
    }

    /// The demo with the Push dialog open as `--show push` opens it, in a window of `size`; one frame drawn.
    fn push_dialog(size: [f32; 2]) -> Window {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "push").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.size = size;
        window.frame(Vec::new());
        window
    }

    /// Where the text `words` was painted in the last frame, in the order of the texts: its index.
    fn painted(window: &Window, words: &str) -> usize {
        window.texts.iter().position(|(said, _, _)| said == words).unwrap_or_else(|| panic!("not painted: {words}"))
    }

    #[test]
    fn the_facts_are_the_cores() {
        let mut window = push_dialog(WINDOW);
        assert!(window.shows(Which::Push));
        let facts = window.repos().service_facts();
        assert_eq!(state(&window).facts, facts);
        assert_eq!(facts.len(), 7);
        // The demo's api has only its origin: no remote of that name to point elsewhere.
        assert_eq!(facts[3].words, "This checkout keeps its files and origin, and keeps the address as the remote codeberg.");
        // Each drawn whole, in the core's order, after its check or cross; the box takes all of them.
        let mut last = 0;
        let mut wrapped = false;
        for fact in &facts {
            let at = painted(&window, &fact.words);
            assert!(at > last, "{} is out of order", fact.words);
            last = at;
            let (_, rect, cut) = &window.texts[at];
            assert!(!cut, "cut short: {}", fact.words);
            assert!(parts(&window).facts.contains_rect(*rect), "{} leaves the box", fact.words);
            wrapped |= rect.height() > 1.5 * line_height();
            let mark = assets::icon(if fact.goes { Icon::Check } else { Icon::X }).to_string();
            assert_eq!(window.texts[at - 1].0, mark, "the mark of {}", fact.words);
        }
        assert!(wrapped, "no fact wraps: the test does not see a wrapped one");
        assert!(window.dialogs.list_scroll.abs() < 1e-3);
        // Another address: the core names another remote, and the dialog shows it.
        type_over(&mut window, "https://gitlab.com/acme/api.git");
        assert_eq!(state(&window).facts, window.repos().service_facts());
        let words = state(&window).facts[3].words.clone();
        assert!(words.ends_with("the remote gitlab."), "{words}");
        painted(&window, &words);
    }

    #[test]
    fn an_address_git_cannot_push_to_is_refused_in_the_cores_words() {
        let mut window = push_dialog(WINDOW);
        assert_eq!(window.said(), ["Cancel", "Push"]);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        painted(&window, NOTE);
        type_over(&mut window, "my server");
        assert_eq!(input(&window), "my server", "the text is the core's");
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Push), "the dialog closed on an address Git cannot push to");
        let reason = error(&window).expect("the core's reason");
        assert!(reason.starts_with("That is not an address Git can push to."), "{reason}");
        assert_ne!(window.repos().status, HELD, "the refusal is no held work");
        window.frame(Vec::new());
        // Under the field, wrapped and whole, where the note was; the field keeps the keys.
        let at = painted(&window, &reason);
        let (_, rect, cut) = window.texts[at].clone();
        assert!(!cut && parts(&window).under.contains_rect(rect), "{rect:?} in {:?}", parts(&window).under);
        assert!(parts(&window).under.bottom() <= parts(&window).facts.top());
        assert!(!window.texts.iter().any(|(said, _, _)| said == NOTE));
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        // The button says it too; typing takes the reason away.
        window.press("Push");
        assert!(window.shows(Which::Push) && error(&window).is_some());
        let field = parts(&window).field;
        window.click(field.center());
        tap(&mut window, Key::X, Some("x"));
        assert_eq!(error(&window), None);
        window.frame(Vec::new());
        painted(&window, NOTE);
        // Esc closes it through the core.
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    #[test]
    fn push_is_off_while_the_address_is_empty() {
        let mut window = push_dialog(WINDOW);
        type_over(&mut window, "");
        assert_eq!(input(&window), "");
        assert_eq!(window.said(), ["Cancel", "Push"]);
        // Enter and a click on it do nothing; the core is not asked, so it gives no reason.
        tap(&mut window, Key::Enter, None);
        let main = window.buttons()[1].0;
        window.click(main.center());
        assert!(window.shows(Which::Push), "the button that is off pushed");
        assert_eq!(error(&window), None);
        assert_ne!(window.repos().status, HELD);
        // Nor does Enter with nothing focused.
        window.ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Push) && error(&window).is_none(), "Enter without a focus pushed while Push is off");
        // Tab goes from the field to Cancel and back: the button that is off takes no focus.
        let field = parts(&window).field;
        window.click(field.center());
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        // Typed again, it is on.
        tap(&mut window, Key::G, Some("g"));
        window.press("Push");
        assert!(window.shows(Which::Push), "the core takes no address g");
        assert!(error(&window).is_some());
    }

    #[test]
    fn letters_typed_into_the_address_only_type() {
        let mut window = push_dialog(WINDOW);
        let before = (window.repos().marked.clone(), window.repos().selected.clone(), window.repos().status.clone());
        let letters = [
            (Key::A, "a"),
            (Key::I, "i"),
            (Key::D, "d"),
            (Key::U, "u"),
            (Key::W, "w"),
            (Key::M, "m"),
            (Key::L, "l"),
            (Key::S, "s"),
            (Key::Space, " "),
        ];
        for (code, letter) in letters {
            window.typed(code, letter);
            assert!(window.shows(Which::Push), "{letter} closed the dialog or opened another");
        }
        assert_eq!(input(&window), format!("{PUSH_ADDRESS}aiduwmls "));
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone(), window.repos().status.clone()), before);
        assert!(window.repos().mode == CoreMode::Service && window.repos().queue.is_empty());
    }

    #[test]
    fn the_field_follows_a_text_the_core_changes() {
        let mut window = push_dialog(WINDOW);
        window.session.repos_mut().unwrap().set_service_input("https://gitlab.com/acme/api.git");
        window.frame(Vec::new());
        assert_eq!(state(&window).text, "https://gitlab.com/acme/api.git");
        assert!(state(&window).facts[3].words.ends_with("the remote gitlab."));
        tap(&mut window, Key::X, Some("x"));
        assert_eq!(input(&window), "https://gitlab.com/acme/api.gitx", "typing goes on at the end");
    }

    #[test]
    fn an_enter_held_into_the_first_frame_does_not_push() {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "push").unwrap();
        let mut window = Window::new(Session::demo(repos));
        // The Enter that opened it (it is down in the frame the dialog opens), then held: it repeats.
        window.frame(vec![key(Key::Enter, true, false, Modifiers::NONE)]);
        for _ in 0..4 {
            window.frame(vec![key(Key::Enter, true, true, Modifiers::NONE)]);
        }
        assert!(window.shows(Which::Push), "the Enter that opened it pushed");
        assert_ne!(window.repos().status, HELD);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        window.frame(vec![key(Key::Enter, false, false, Modifiers::NONE)]);
        // Tab from the field goes to Cancel, then Push; from no focus, Tab and Shift-Tab go to the field.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
            window.ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
            window.frame(vec![key(Key::Tab, true, false, modifiers)]);
            window.frame(Vec::new());
            assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "{modifiers:?}");
        }
        // With nothing focused, a fresh Enter of its own pushes: the held demo refuses the work and says so.
        window.ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        window.frame(Vec::new());
        assert_eq!(window.ctx.memory(Memory::focused), None);
        tap(&mut window, Key::Enter, None);
        assert!(!window.open(), "Enter without a focus did not push");
        assert_eq!(window.repos().status, HELD);
    }

    #[test]
    fn the_second_click_of_a_double_click_that_opened_it_does_not_push() {
        let at = push_dialog(WINDOW).buttons()[1].0.center();
        let mut repos = crate::demo::repos();
        repos.toggle_mark_of("web");
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        click_at_time(&mut window, at, 5.0);
        // The first click opened it, as the Migrate menu's push line will.
        let repos = window.session.repos_mut().unwrap();
        repos.run_migrate(MigrateItem::Service);
        repos.set_service_input(PUSH_ADDRESS);
        window.frame_at(Vec::new(), Some(5.1));
        assert!(window.shows(Which::Push), "{}", window.repos().status);
        assert!(window.buttons()[1].0.contains(at), "the point is on Push");
        click_at_time(&mut window, at, 5.2);
        window.frame_at(Vec::new(), Some(5.3));
        assert!(window.shows(Which::Push), "the second click pushed");
        assert_ne!(window.repos().status, HELD);
        // A click of its own, later, is one.
        click_at_time(&mut window, at, 9.0);
        window.frame_at(Vec::new(), Some(9.1));
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
    }

    #[test]
    fn the_facts_scroll_and_the_buttons_stay_in_view_in_the_smallest_window() {
        // Placed: whatever the line under the field and the facts want, everything stays inside the pane, and the box
        // takes what the window leaves.
        for size in [MIN_WINDOW, WINDOW] {
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for under in [17.4, 3.0 * 17.4] {
                for facts in [100.0, 400.0, 2000.0] {
                    let places = push_places(screen, 67.0, under, facts);
                    assert!(screen.contains_rect(places.pane), "{size:?} {under} {facts}");
                    for part in [places.header, places.label, places.field, places.under, places.facts, places.buttons] {
                        assert!(places.pane.contains_rect(part), "{size:?} {under} {facts}: {part:?} outside {:?}", places.pane);
                    }
                    assert!(places.under.bottom() <= places.facts.top() && places.facts.bottom() <= places.buttons.top());
                    assert!(places.facts.height() <= facts + 1e-3);
                }
            }
        }
        // Drawn in the smallest window: the facts want more than the window leaves, so the box scrolls.
        let mut window = push_dialog(MIN_WINDOW);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
        let places = parts(&window);
        assert!(screen.contains_rect(places.pane) && places.pane.contains_rect(places.buttons));
        let last = state(&window).facts.last().unwrap().words.clone();
        let whole = |window: &Window| window.texts.iter().any(|(said, rect, cut)| *said == last && !cut && places.facts.contains_rect(*rect));
        assert!(!whole(&window), "the last fact is in view unscrolled");
        // The wheel reaches the last fact, whole, and no further.
        window.frame(vec![Event::PointerMoved(places.facts.center())]);
        for _ in 0..60 {
            window.frame(vec![wheel(-120.0)]);
        }
        let end = window.dialogs.list_scroll;
        assert!(end > 0.0);
        window.frame(vec![wheel(-120.0)]);
        window.frame(Vec::new());
        assert!((window.dialogs.list_scroll - end).abs() < 1e-3, "it scrolled past the end");
        assert!(whole(&window), "the last fact is not in view at the end");
        // The buttons are there to press.
        window.press("Cancel");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    /// The commit `name` is at in the repository `dir`.
    fn tip(dir: &Path, name: &str) -> String {
        let output = std::process::Command::new("git").arg("-C").arg(dir).args(["rev-parse", "--verify", "-q", name]).output().unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn push_sends_a_checkout_to_a_bare_repository_and_the_progress_dialog_shows_it() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("push", &["api"]);
        assert!(settings.config_path.starts_with(&root.0), "the settings are kept in the temporary folder");
        let target = root.0.join("target.git");
        std::fs::create_dir_all(&target).unwrap();
        git(&target, &["init", "-q", "--bare"]);
        let mut repos = Repos::new(settings);
        // Nothing asks GitHub: no test reaches the network.
        hold_github(&mut repos);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        until(&mut window, "read", |window| !window.repos().waiting() && window.repos().installed.len() == 1);
        // The Migrate menu's push line, as the terminal app opens it.
        let repos = window.session.repos_mut().unwrap();
        repos.pick("api");
        repos.run_migrate(MigrateItem::Service);
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.shows(Which::Push), "{}", window.repos().status);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        let address = target.display().to_string();
        type_over(&mut window, &address);
        assert_eq!(input(&window), address);
        window.press("Push");
        // The dialog went; the run's progress dialog shows its task until it has ended.
        until(&mut window, "pushed", |window| window.repos().job.as_ref().is_some_and(|job| !job.running()) && !window.repos().busy);
        assert!(window.shows(Which::Progress), "the run's dialog is not shown: {}", window.repos().status);
        assert_eq!(window.job().title, format!("Pushing api to {address}"));
        assert_eq!(window.job().items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api"]);
        assert!(matches!(&window.job().result, Some(Ok(_))), "{:?}", window.job().result);
        // Origin was fetched first: its newer main went, not the one the checkout is behind with.
        let checkout = root.0.join("api");
        assert_eq!(tip(&target, "refs/heads/main"), tip(&root.0.join("api-origin"), "refs/heads/main"));
        assert_ne!(tip(&checkout, "refs/heads/main"), tip(&target, "refs/heads/main"));
        // The checkout keeps its origin and keeps the address as the remote mirror.
        let config = std::fs::read_to_string(checkout.join(".git").join("config")).unwrap();
        assert!(config.contains("[remote \"origin\"]") && config.contains("[remote \"mirror\"]"), "{config}");
        window.session.close();
    }
}
