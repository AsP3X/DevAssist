//! The folder dialog ([`super::Which::Folder`]), for the core's `Mode::Path`: it asks for a folder, and the core does
//! with it what its question is for (`Purpose`). For now that is Import: the GitHub clones in the folder, and the
//! folder itself, are added to the overview. The design draws no such dialog; it is built from the Add dialog's
//! parts, 600 wide: the header, "Folder" over the folder field with the folders that match under it, the core's
//! reason under the field when it refused the folder, and the buttons.
//!
//! What differs between the purposes is said in one place ([`words`]): the title, the quiet line and the main
//! button's word. A purpose the dialog does not show yet has no words, and the bridge shows no dialog for it.
//!
//! The field has the focus when the dialog opens. Its text is the core's (`Repos::set_path_input` as it is typed); the
//! folders that match are asked for once when it opens and again only when the text changes. Enter in the field
//! (with no suggestion lit) and the main button ask the core to go ahead (`Action::Confirm`): when it refuses the
//! folder, the dialog stays with the core's reason under the field, in the danger colour, until the text changes;
//! else the dialog closes and the core's status line says what was done. Esc, Cancel and the scrim close it through
//! the core (`Repos::close_path`). The import runs in the core on the window's thread.

use devassist_core::repos::state::{Action, Purpose, Repos};
use egui::{Align2, FontFamily, Id, Key, Rect, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, folder_suggestions, main_id, take_press};
use crate::assets;
use crate::overlay::{self, Kind};
use crate::theme::{
    ADD_FOLDER_GAP, BUTTON_HEIGHT, DIALOG_FIELD_HEIGHT, DIALOG_GAP, DIALOG_MARGIN, DIALOG_PAD, FOLDER_DIALOG_WIDTH, HEADER_LINE_HEIGHT, Palette,
    TEXT, TEXT_SMALL,
};
use crate::widgets::{self, font};

/// The words over the field.
const FOLDER: &str = "Folder";
/// The button that closes the dialog.
const CANCEL: &str = "Cancel";

/// What the dialog says for one purpose.
struct Words {
    /// The title.
    title: &'static str,
    /// The quiet line under it.
    line: &'static str,
    /// The main button's word.
    button: &'static str,
}

/// Import's words.
const IMPORT: Words = Words {
    title: "Import clones you already have",
    line: "Adds the GitHub clones in a folder to the overview. The folder itself counts too. Nothing on disk is changed.",
    button: "Import",
};

/// The dialog's words for `purpose`; `None` for a purpose it does not show (a move, for now; the core's own folder
/// question of a clone, which Add stands for).
fn words(purpose: &Purpose) -> Option<&'static Words> {
    match purpose {
        Purpose::Import => Some(&IMPORT),
        Purpose::Move(_) | Purpose::Clone(_) => None,
    }
}

/// Whether the dialog shows the core's folder question for `purpose`.
pub(super) fn shown(purpose: &Purpose) -> bool {
    words(purpose).is_some()
}

/// The id of the folder field, which has the focus when the dialog opens.
pub(super) fn field_id() -> Id {
    Id::unique("dialog-folder-field")
}

/// The id of the Cancel button.
fn cancel_id() -> Id {
    Id::unique("dialog-folder-cancel")
}

/// What the folder dialog keeps of its own from one frame to the next, made when it opens ([`State::new`]).
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// The field's text as it stood last: the core's, which the dialog follows when the core changes it.
    text: String,
    /// The folders that match it ([`folder_suggestions`]), asked for again only when it changes.
    suggestions: Vec<String>,
}

impl State {
    /// The state of a folder dialog that opens on `repos`: the core's text, and the folders that match it, asked for
    /// once (a text the dialog did not type never says it changed).
    pub(super) fn new(repos: &Repos) -> State {
        let text = repos.prompt.as_ref().map_or_default(|prompt| prompt.input.clone());
        let suggestions = folder_suggestions(repos, &text);
        State { text, suggestions }
    }
}

/// The words of the folder dialog's buttons in `repos`, from the left.
#[cfg(test)]
pub(super) fn said(repos: &Repos) -> Vec<&'static str> {
    repos.prompt.as_ref().and_then(|prompt| words(&prompt.purpose)).map_or_default(|words| vec![CANCEL, words.button])
}

/// The parts of the folder dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// "Folder".
    pub(super) label: Rect,
    /// The folder field with the room its rows take.
    pub(super) field: Rect,
    /// The core's reason under it, when it refused the folder.
    pub(super) note: Option<Rect>,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// The height of a line of `size`-point text.
fn line_height(size: f32) -> f32 {
    size * HEADER_LINE_HEIGHT
}

/// Where the folder dialog's parts are in `window`, with a header `header` points tall, the field showing `rows`
/// suggestions, and the core's reason under it or not. The dialog grows with the rows, as far as the window lets it;
/// the rows give up their room before anything else, and the buttons stay inside the pane.
fn folder_places(window: Rect, header: f32, rows: usize, note: bool) -> Places {
    let note_want = if note { ADD_FOLDER_GAP + line_height(TEXT_SMALL) } else { 0.0 };
    let fixed =
        2.0 * DIALOG_PAD + header + DIALOG_GAP + line_height(TEXT) + ADD_FOLDER_GAP + DIALOG_FIELD_HEIGHT + note_want + DIALOG_GAP + BUTTON_HEIGHT;
    let room = window.height() - 2.0 * DIALOG_MARGIN;
    let rows_want = widgets::folder_field_height(rows) - DIALOG_FIELD_HEIGHT;
    let rows_height = rows_want.min((room - fixed).max(0.0));
    let pane = overlay::dialog_rect(window, vec2(FOLDER_DIALOG_WIDTH, fixed + rows_height));
    let inside = pane.shrink(DIALOG_PAD);
    let at = |top: f32, height: f32| Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), height));
    let header = at(inside.top(), header);
    let label = at(header.bottom() + DIALOG_GAP, line_height(TEXT));
    let field = at(label.bottom() + ADD_FOLDER_GAP, DIALOG_FIELD_HEIGHT + rows_height);
    let note = note.then(|| at(field.bottom() + ADD_FOLDER_GAP, line_height(TEXT_SMALL)));
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    Places { pane, header, label, field, note, buttons }
}

/// Where the folder dialog of `repos` stands in `window`, its header laid out with `painter`'s faces, its field as
/// `state` has it; `None` without a folder question the dialog shows.
pub(super) fn plan(painter: &egui::Painter, window: Rect, repos: &Repos, state: &State) -> Option<Plan> {
    let prompt = repos.prompt.as_ref()?;
    let words = words(&prompt.purpose)?;
    let header = Header::new(painter, words.title, words.line, FOLDER_DIALOG_WIDTH - 2.0 * DIALOG_PAD);
    let rows = widgets::folder_field_rows(painter.ctx(), field_id(), state.suggestions.len());
    let places = folder_places(window, header.height(), rows, prompt.error.is_some());
    Some(Plan::dialog(header, Parts::Folder(places)))
}

/// What a person did in the folder dialog in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The text typed, or a suggestion taken, when it changed.
    typed: Option<String>,
    /// The main button, or Enter in the field.
    accept: bool,
    /// Cancel, Esc or the scrim.
    close: bool,
}

/// The folder dialog at `places`, with `header`, for `repos`'s folder question, on the overlay in `ui`; `scrim` is
/// whether the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(mut state) = dialogs.folder.take() else { return };
    let Some((words, input, error)) =
        repos.prompt.as_ref().and_then(|prompt| words(&prompt.purpose).map(|words| (words, prompt.input.clone(), prompt.error.clone())))
    else {
        dialogs.folder = Some(state);
        return;
    };
    overlay::walk(ui, &[field_id(), cancel_id(), main_id()]);
    if input != state.text {
        // The core changed the text: the field follows it, typing goes on at its end, and its folders are asked for.
        widgets::put_cursor_at_end(ui.ctx(), field_id(), &input);
        state.suggestions = folder_suggestions(repos, &input);
        state.text = input;
    }
    header.draw(ui, places.header, palette);
    ui.painter().text(places.label.left_center(), Align2::LEFT_CENTER, FOLDER, font(TEXT, assets::medium_family()), palette.on_glass);

    // The field first: it takes Esc, Enter, Up and Down while its rows show; the dialog's own come after.
    let focused = ui.memory(|memory| memory.has_focus(field_id()));
    let mut text = state.text.clone();
    let did = widgets::folder_field(ui, places.field, &mut text, &state.suggestions, palette, field_id());
    let mut done = Done { close: scrim, ..Done::default() };
    if did.changed {
        done.typed = Some(text);
    }
    if focused {
        done.accept |= take_press(ui, Key::Enter, None);
    }
    done.close |= take_press(ui, Key::Escape, None);

    if let (Some(rect), Some(reason)) = (places.note, error.filter(|_| !did.changed)) {
        let small = font(TEXT_SMALL, FontFamily::Proportional);
        let reason = widgets::ellipsis(&reason, &small, rect.width(), ui);
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, reason, small, palette.danger);
    }

    let widths = [widgets::button_width(ui, CANCEL), widgets::button_width(ui, words.button)];
    if let [cancel, main] = overlay::buttons(places.buttons, &widths).as_slice() {
        done.close |= widgets::button(ui, *cancel, CANCEL, Kind::Plain, palette, cancel_id());
        done.accept |= widgets::button(ui, *main, words.button, Kind::Main, palette, main_id());
    }
    act(ui, repos, &mut state, &done);
    dialogs.folder = Some(state);
}

/// Does in `repos` what a person did in the folder dialog in one frame; keeps the dialog's own `state`.
fn act(ui: &Ui, repos: &mut Repos, state: &mut State, done: &Done) {
    if done.close {
        repos.close_path();
        return;
    }
    if let Some(text) = &done.typed {
        repos.set_path_input(text);
        state.suggestions = folder_suggestions(repos, text);
        text.clone_into(&mut state.text);
    }
    if done.accept {
        repos.run(Action::Confirm);
        if repos.prompt.is_some() {
            // Refused: the reason stands under the field in the next frame, and the field has the keys again.
            ui.memory_mut(|memory| memory.request_focus(field_id()));
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::testing::{hold_github, test_settings};
    use devassist_core::repos::state::{HELD, Mode as CoreMode};
    use egui::{Event, Memory, Modifiers, Pos2};

    use super::super::Which;
    use super::super::testing::{Temp, Window, git, until};
    use super::*;
    use crate::session::Session;
    use crate::theme::{MIN_WINDOW, WINDOW};

    /// The folder dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Folder(places) = window.parts() else { panic!("not the folder dialog") };
        places
    }

    fn state(window: &Window) -> &State {
        window.dialogs.folder.as_ref().unwrap()
    }

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    /// A press of `code` typing `text`, and its release.
    fn tap(window: &mut Window, code: Key, text: Option<&str>) {
        let mut press = vec![key(code, true, Modifiers::NONE)];
        press.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(press);
        window.frame(vec![key(code, false, Modifiers::NONE)]);
    }

    /// Ctrl-A in the field, then `text` typed over it.
    fn type_over(window: &mut Window, text: &str) {
        window.frame(vec![key(Key::A, true, Modifiers::COMMAND)]);
        window.frame(vec![key(Key::A, false, Modifiers::COMMAND), Event::Text(text.to_owned())]);
        window.frame(Vec::new());
    }

    /// A temporary folder `name` with two clones in it whose origins name GitHub (nothing is fetched from them) and a
    /// folder that is no clone, and the settings, kept inside the temporary folder, of a session without repositories.
    fn two_clones(name: &str) -> (Temp, std::path::PathBuf, devassist_core::config::Settings) {
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-{name}-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&root.0);
        let code = root.0.join("code");
        for repo in ["alpha", "beta"] {
            let dir = code.join(repo);
            std::fs::create_dir_all(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("readme"), repo).unwrap();
            git(&dir, &["add", "readme"]);
            git(&dir, &["commit", "-q", "-m", "one"]);
            git(&dir, &["remote", "add", "origin", &format!("https://github.com/acme/{repo}.git")]);
        }
        std::fs::create_dir_all(code.join("notes")).unwrap();
        let settings = devassist_core::config::Settings {
            config_path: root.0.join("devassist.toml"),
            clone_dir: root.0.clone(),
            owners: Vec::new(),
            ..test_settings()
        };
        (root, code, settings)
    }

    #[test]
    fn import_adds_the_clones_in_a_folder_and_says_how_many() {
        let _live = crate::session::tests::live_git();
        let (root, code, settings) = two_clones("import");
        let mut repos = Repos::offline(settings);
        // The reload after the import asks GitHub nothing: no test reaches the network.
        hold_github(&mut repos);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        // i opens Import, its field at the clone folder and with the keys; the i is not typed into it.
        tap(&mut window, Key::I, Some("i"));
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder), "{}", window.repos().status);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        assert_eq!(state(&window).text, root.0.display().to_string(), "the i was typed");
        assert_eq!(window.said(), ["Cancel", "Import"]);
        type_over(&mut window, &code.display().to_string());
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, code.display().to_string(), "the text is the core's");
        tap(&mut window, Key::Enter, None);
        assert!(!window.open(), "Import stayed open: {:?}", window.repos().prompt.as_ref().and_then(|prompt| prompt.error.clone()));
        assert_eq!(window.repos().status, format!("Added 2 repositories from {}.", code.display()));
        until(&mut window, "read", |window| !window.repos().waiting());
        let mut folders: Vec<&str> = window.repos().installed.iter().map(|repo| repo.folder.as_str()).collect();
        folders.sort_unstable();
        assert_eq!(folders, ["alpha", "beta"]);
        assert!(root.0.join("devassist.toml").is_file(), "the settings were not saved in the temporary folder");
        // Import again, by its button: nothing new, in the core's words.
        tap(&mut window, Key::I, Some("i"));
        window.frame(Vec::new());
        type_over(&mut window, &code.display().to_string());
        window.press("Import");
        assert_eq!(window.repos().status, format!("Everything in {} is on the list already.", code.display()));
        window.session.close();
    }

    #[test]
    fn a_folder_that_is_a_file_is_refused_in_the_cores_words() {
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-import-file-{}", std::process::id())));
        std::fs::create_dir_all(&root.0).unwrap();
        let file = root.0.join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder));
        assert_eq!(parts(&window).note, None, "nothing refused yet");
        type_over(&mut window, &file.display().to_string());
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "Import closed on a file");
        let reason = window.repos().prompt.as_ref().unwrap().error.clone().unwrap();
        assert_eq!(reason, format!("{} is a file, not a folder.", file.display()));
        let note = parts(&window).note.expect("the reason under the field");
        assert!(note.top() >= parts(&window).field.bottom() && note.bottom() <= parts(&window).buttons.top());
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "the field keeps the keys");
        assert_ne!(window.repos().status, HELD, "the refusal is no held work");
        // The button says it too; typing takes the reason away.
        window.press("Import");
        assert!(window.shows(Which::Folder) && window.repos().prompt.as_ref().unwrap().error.is_some());
        let field = parts(&window).field;
        window.click(Pos2::new(field.center().x, field.top() + DIALOG_FIELD_HEIGHT / 2.0));
        tap(&mut window, Key::X, Some("x"));
        assert_eq!(window.repos().prompt.as_ref().unwrap().error, None);
        window.frame(Vec::new());
        assert_eq!(parts(&window).note, None);
        // Esc closes it through the core.
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    #[test]
    fn a_held_import_of_a_real_folder_changes_nothing() {
        // The held core looks at the folder (a read), then refuses the work and says so.
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let before = repos.checkouts.clone();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        type_over(&mut window, &std::env::temp_dir().display().to_string());
        window.press("Import");
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
        assert_eq!(window.repos().checkouts, before);
        assert!(!window.repos().config_path.exists());
    }

    #[test]
    fn the_dialog_fits_every_window_with_its_rows() {
        for size in [MIN_WINDOW, WINDOW] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for (rows, note) in [(0, false), (0, true), (6, false), (6, true)] {
                let places = folder_places(window, 60.0, rows, note);
                assert!(window.contains_rect(places.pane), "{size:?} {rows} {note}");
                assert!((places.pane.width() - FOLDER_DIALOG_WIDTH).abs() < 1e-3);
                for part in [places.header, places.label, places.field, places.buttons] {
                    assert!(places.pane.contains_rect(part), "{size:?} {rows} {note}: {part:?}");
                }
                assert!(places.note.is_some() == note && places.note.is_none_or(|rect| rect.bottom() <= places.buttons.top()));
                assert!(places.field.height() >= DIALOG_FIELD_HEIGHT);
            }
        }
        // The dialog grows with the rows.
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
        assert!(folder_places(window, 60.0, 3, false).pane.height() > folder_places(window, 60.0, 0, false).pane.height());
    }

    #[test]
    fn the_demos_import_suggests_its_own_folders_and_never_asks_the_disk() {
        let before = super::super::disk_reads();
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        assert_eq!(state(&window).text, crate::demo::CODE_DIR);
        type_over(&mut window, &format!("{}{}", crate::demo::CODE_DIR, std::path::MAIN_SEPARATOR));
        assert_eq!(state(&window).suggestions.len(), 4, "{:?}", state(&window).suggestions);
        assert_eq!(super::super::disk_reads(), before, "the held demo read the disk for folders");
        // A suggestion taken by Enter is the core's text too, and imports nothing.
        tap(&mut window, Key::ArrowDown, None);
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "the Enter that took a row went on to import");
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, state(&window).text);
        assert!(state(&window).text.ends_with(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn the_enter_that_opens_import_does_not_import() {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let mut window = Window::new(Session::demo(repos));
        let enter = |pressed, repeat| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE };
        window.frame(vec![enter(true, false)]);
        for _ in 0..3 {
            window.frame(vec![enter(true, true)]);
        }
        assert!(window.shows(Which::Folder), "the Enter that opened Import pressed Import");
        assert_ne!(window.repos().status, HELD);
        window.frame(vec![enter(false, false)]);
        // Tab from the field goes to Cancel, then Import.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
    }
}
