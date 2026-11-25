//! What every menu and dialog shares: where it stands, the layer it is drawn on, the shield and the scrim under
//! it, its glass pane, its buttons and how the keyboard walks them.
//!
//! An overlay is drawn after the screen, in [`show`]: a layer of its own above the screen's, which egui treats as
//! modal. On it come, in this order, the [`shield`] (one area over the whole window that takes every click and
//! wheel turn, and says whether the scrim itself was clicked), for a dialog the [`scrim`], the pane's [`glass`]
//! (baked from the wallpaper like every pane, from [`want_dialog`] or [`want_menu`]), the [`pane`] that keeps a
//! click on the pane's empty part off the scrim, then [`walk`] and the controls. The screen under an open overlay
//! is told it has no keyboard (`repos_screen::show` takes `keys`), so every key is the overlay's.
//!
//! The keyboard: only the controls in an overlay take egui's focus ([`crate::widgets::button`]). [`walk`] moves it
//! among them on Tab and Shift-Tab, and nowhere else; Enter and Space press the one that has it, and nothing else.
//! Whoever opens an overlay gives the focus to the control Enter should press, if any (`memory.request_focus`);
//! with none, Enter and Space do nothing until Tab has chosen one.

use egui::{
    Align2, Area, AreaState, Color32, Context, Event, FocusDirection, Id, Key, LayerId, Memory, Mesh, Order, Rect, Response, Sense, Ui, Vec2, pos2,
    vec2,
};

use crate::baker::{Drawn, Want};
use crate::theme::{BUTTON_GAP, BUTTON_HEIGHT, DIALOG_MARGIN, DIALOG_RADIUS, MENU_GAP, MENU_RADIUS, Palette, Surface};

/// The glass pane of a dialog.
pub const DIALOG: &str = "dialog";
/// The glass pane of a menu.
pub const MENU: &str = "menu";

/// What a button of a dialog is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One of the others, on the selection's fill.
    Plain,
    /// The one Enter presses, on the accent.
    Main,
    /// One that removes or loses something, on the danger tint.
    Danger,
}

/// The glass pane of a dialog at `rect`, to add to the panes baked this frame.
#[must_use]
pub fn want_dialog(rect: Rect) -> Want {
    Want { id: DIALOG, rect, radius: DIALOG_RADIUS, surface: Surface::Dialog }
}

/// The glass pane of a menu at `rect`, to add to the panes baked this frame.
#[must_use]
pub fn want_menu(rect: Rect) -> Want {
    Want { id: MENU, rect, radius: MENU_RADIUS, surface: Surface::Menu }
}

/// Whether the baked picture named `id` is an overlay's pane, which is drawn on the overlay over its scrim
/// ([`glass`]), not under the screen.
#[must_use]
pub fn is_overlay_pane(id: &str) -> bool {
    id == DIALOG || id == MENU
}

/// Where a dialog of `size` stands in `window`: centred in it, never closer than [`DIALOG_MARGIN`] to an edge. A
/// dialog taller or wider than the room is cut to the room.
#[must_use]
pub fn dialog_rect(window: Rect, size: Vec2) -> Rect {
    let room = window.shrink(DIALOG_MARGIN).size().max(Vec2::ZERO);
    Rect::from_center_size(window.center(), size.min(room).max(Vec2::ZERO))
}

/// Where a menu of `size` stands in `window` for the button at `anchor`: under it, [`MENU_GAP`] away, with its
/// right edge on the button's; above the button when there is no room below (and more above). It is pushed back
/// inside the window to [`DIALOG_MARGIN`] from an edge, or as near the edge as the button itself is, when that is
/// nearer: a menu keeps its right edge on a button at the window's edge. A menu larger than the room is cut to it.
#[must_use]
pub fn menu_rect(anchor: Rect, size: Vec2, window: Rect) -> Rect {
    let left = window.left() + DIALOG_MARGIN.min((anchor.left() - window.left()).max(0.0));
    let right = window.right() - DIALOG_MARGIN.min((window.right() - anchor.right()).max(0.0));
    let (top, bottom) = (window.top() + DIALOG_MARGIN, window.bottom() - DIALOG_MARGIN);
    let size = size.min(vec2(right - left, bottom - top)).max(Vec2::ZERO);
    let below = anchor.bottom() + MENU_GAP;
    let above = anchor.top() - MENU_GAP;
    let y = if below + size.y <= bottom || bottom - below >= above - top { below } else { above - size.y };
    let x = anchor.right() - size.x;
    Rect::from_min_size(pos2(x.min(right - size.x).max(left), y.min(bottom - size.y).max(top)), size)
}

/// The places of buttons of `widths`, in the order given, at the bottom of `room`: [`BUTTON_HEIGHT`] high,
/// [`BUTTON_GAP`] apart, the last one's right edge on `room`'s.
#[must_use]
pub fn buttons(room: Rect, widths: &[f32]) -> Vec<Rect> {
    let top = room.bottom() - BUTTON_HEIGHT;
    let mut right = room.right();
    let mut places: Vec<Rect> = widths
        .iter()
        .rev()
        .map(|width| {
            let place = Rect::from_min_max(pos2(right - width, top), pos2(right, room.bottom()));
            right = place.left() - BUTTON_GAP;
            place
        })
        .collect();
    places.reverse();
    places
}

/// Runs `contents` on the overlay's own layer `id`, over the whole `window` and above the screen, and gives back
/// what it returned. The layer is modal: nothing under it gets the pointer (no hover, no click, no wheel) or the
/// keyboard's focus. The screen is drawn before it in a frame, so that holds from the frame after the one it
/// opens in; the keyboard the screen is told it has not (`keys`) from the first. It appears at once, whole, with
/// no fade.
pub fn show<R>(ctx: &Context, id: Id, window: Rect, contents: impl FnOnce(&mut Ui) -> R) -> R {
    // egui lays a new area out unseen and untouchable in its first frame, to learn its size, and takes the focus
    // from what it holds. The overlay's size is the window's: told so, it is there, whole, from its first frame.
    let state =
        AreaState { pivot_pos: Some(window.min), pivot: Align2::LEFT_TOP, size: Some(window.size()), ..AreaState::load(ctx, id).unwrap_or_default() };
    ctx.memory_mut(|memory| memory.areas_mut().set_state(LayerId::new(Order::Foreground, id), state));
    Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(window.min)
        .default_size(window.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(window);
            let layer = ui.layer_id();
            ui.ctx().memory_mut(|memory| memory.set_modal_layer(layer));
            contents(ui)
        })
        .inner
}

/// The shield of the overlay in `ui` (from [`show`]): one area over the whole `window` that takes every click and
/// wheel turn, so nothing under it gets them. Its response is clicked when the scrim itself was, and not what
/// lies over it on the overlay ([`pane`], the controls). Added before anything else on the overlay.
pub fn shield(ui: &mut Ui, window: Rect, id: Id) -> Response {
    ui.expand_to_include_rect(window);
    ui.interact(window, id, Sense::CLICK)
}

/// Lays the scrim over `window`, under a dialog. A menu has none.
pub fn scrim(ui: &Ui, window: Rect, palette: Palette) {
    ui.painter().rect_filled(window, 0.0, palette.scrim);
}

/// Paints the overlays' panes among `drawn` (the baker's pictures of this frame) on the overlay in `ui`, over its
/// scrim; the others are painted under the screen.
pub fn glass(ui: &Ui, drawn: &[Drawn]) {
    for pane in drawn.iter().filter(|drawn| is_overlay_pane(drawn.id)) {
        let mut mesh = Mesh::with_texture(pane.texture);
        mesh.add_rect_with_uv(pane.rect, pane.uv, Color32::WHITE);
        ui.painter().add(mesh);
    }
}

/// Takes the clicks on the pane at `rect`, so that a click on its empty part is not a click on the scrim.
pub fn pane(ui: &Ui, rect: Rect, id: Id) {
    ui.interact(rect, id, Sense::CLICK);
}

/// Keeps egui's keyboard focus among the controls `ids`, in their Tab order: Tab gives it to the next (the first
/// when none has it), Shift-Tab to the one before (the last when none has it), round from the end to the start.
/// A focus outside them (the filter's, say) is taken away; the arrows and egui's own Tab move it nowhere. Called
/// on the overlay before its controls, every frame it is open; the Tab keys are its own, no control sees them.
pub fn walk(ui: &mut Ui, ids: &[Id]) {
    let presses: Vec<bool> = ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if !modifiers.any() || modifiers.shift_only() => Some(modifiers.shift),
                _ => None,
            })
            .collect()
    });
    ui.input_mut(|input| input.events.retain(|event| !matches!(event, Event::Key { key: Key::Tab, .. })));
    let focused = ui.memory(Memory::focused);
    let mut current = focused.filter(|id| ids.contains(id));
    for back in presses {
        current = step(ids, current, back);
    }
    ui.memory_mut(|memory| {
        match (current, focused) {
            (Some(id), _) => memory.request_focus(id),
            (None, Some(outside)) => memory.surrender_focus(outside),
            (None, None) => {}
        }
        // egui read the Tab and the arrows before this: they must not move the focus again at the frame's end.
        memory.move_focus(FocusDirection::None);
    });
}

/// The control among `ids` that one Tab (Shift-Tab when `back`) moves the focus to from `current`.
fn step(ids: &[Id], current: Option<Id>, back: bool) -> Option<Id> {
    let last = ids.len().checked_sub(1)?;
    let at = match (current.and_then(|id| ids.iter().position(|known| *known == id)), back) {
        (None, false) => 0,
        (None, true) => last,
        (Some(at), false) => {
            if at == last {
                0
            } else {
                at + 1
            }
        }
        (Some(at), true) => at.checked_sub(1).unwrap_or(last),
    };
    ids.get(at).copied()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use devassist_glass::raster::Bitmap;
    use egui::{FontFamily, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, RawInput, TextEdit, TextureId, TouchPhase};

    use super::*;
    use crate::baker::{Scene, Sheet, pane_picture, sheet_picture};
    use crate::contrast::{luminance, ratio};
    use crate::picture::{self, over};
    use crate::repos_screen::{self, Screen};
    use crate::session::Session;
    use crate::shell::{self, Page, Shown};
    use crate::theme::{self, DIALOG_GAP, DIALOG_PAD, DIALOG_TITLE, Mode, TEXT, WINDOW};
    use crate::widgets::{self, font, tests::context};
    use crate::{assets, demo};

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))
    }

    #[test]
    #[expect(clippy::cast_precision_loss, reason = "window and dialog sizes of a few thousand points")]
    fn a_dialog_is_centred_and_stays_inside_any_window() {
        let window = window();
        assert_eq!(dialog_rect(window, vec2(480.0, 300.0)).center(), window.center());
        assert_eq!(dialog_rect(window, vec2(480.0, 300.0)).size(), vec2(480.0, 300.0));
        for width in (400..=2000).step_by(80) {
            for height in (300..=1200).step_by(60) {
                let window = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32));
                let room = window.shrink(DIALOG_MARGIN);
                for w in (0..=800).step_by(100) {
                    for h in (0..=1000).step_by(100) {
                        let size = vec2(w as f32, h as f32);
                        let rect = dialog_rect(window, size);
                        assert!(room.contains_rect(rect), "{rect:?} for {size:?} leaves {room:?}");
                        assert!((rect.center() - window.center()).length() < 1e-3, "{rect:?} is not centred in {window:?}");
                        // Cut only where the room is too small.
                        assert_eq!(rect.width(), size.x.min(room.width()));
                        assert_eq!(rect.height(), size.y.min(room.height()));
                    }
                }
            }
        }
    }

    #[test]
    fn a_menu_opens_under_its_button_and_flips_or_shifts_to_stay_inside() {
        let window = window();
        let size = vec2(256.0, 340.0);
        // The drawing's place: the row's button at the right of the table.
        let anchor = Rect::from_min_max(pos2(1388.0, 242.0), pos2(1420.0, 274.0));
        let menu = menu_rect(anchor, size, window);
        assert_eq!(menu.min, pos2(1164.0, 278.0));
        assert_eq!((menu.right(), menu.top() - anchor.bottom()), (anchor.right(), MENU_GAP));
        // Near the bottom: above the button, MENU_GAP over its top.
        let low = anchor.translate(vec2(0.0, 800.0 - 242.0));
        let menu = menu_rect(low, size, window);
        assert_eq!(menu.bottom(), low.top() - MENU_GAP);
        assert_eq!(menu.right(), low.right());
        // Near the left edge: pushed in to the margin.
        let left = Rect::from_min_max(pos2(30.0, 242.0), pos2(62.0, 274.0));
        assert_eq!(menu_rect(left, size, window).min.x, DIALOG_MARGIN);
        // A window too short for it above or below: inside the top and bottom margins, cut to them, and still on
        // its button's right edge, which is nearer the window's than the margin.
        let short = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 300.0));
        let menu = menu_rect(Rect::from_min_max(pos2(1388.0, 120.0), pos2(1420.0, 152.0)), size, short);
        assert_eq!((menu.top(), menu.bottom(), menu.right()), (DIALOG_MARGIN, 300.0 - DIALOG_MARGIN, 1420.0), "{menu:?}");
        // Wherever its button is, it stays in the window.
        for x in (0..=1440).step_by(40) {
            for y in (0..=860).step_by(40) {
                #[expect(clippy::cast_precision_loss, reason = "places in a 1440 by 900 window")]
                let anchor = Rect::from_min_size(pos2(x as f32, y as f32), vec2(32.0, 32.0)).intersect(window);
                let menu = menu_rect(anchor, size, window);
                assert!(window.contains_rect(menu) && menu.height().to_bits() == size.y.to_bits(), "{menu:?} for {anchor:?}");
            }
        }
    }

    #[test]
    fn buttons_stand_at_the_right_in_the_order_given() {
        let room = Rect::from_min_max(pos2(500.0, 300.0), pos2(956.0, 520.0));
        let places = buttons(room, &[90.0, 100.0, 80.0]);
        let edges: Vec<(f32, f32)> = places.iter().map(|place| (place.left(), place.right())).collect();
        assert_eq!(edges, [(670.0, 760.0), (768.0, 868.0), (876.0, 956.0)]);
        assert_eq!(places[2].right(), room.right());
        assert_eq!(places[1].right() + BUTTON_GAP, places[2].left());
        assert_eq!(places[0].right() + BUTTON_GAP, places[1].left());
        assert!(places.iter().all(|place| place.max.y.to_bits() == room.max.y.to_bits() && place.height().to_bits() == BUTTON_HEIGHT.to_bits()));
        assert_eq!(places.iter().map(Rect::width).collect::<Vec<_>>(), [90.0, 100.0, 80.0]);
        assert_eq!(buttons(room, &[]), Vec::<Rect>::new());
    }

    /// What the screen under the overlay heard in a frame.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct Heard {
        /// The pointer was over its area, as the table asks before it lights a row or reads the wheel.
        hovered: bool,
        /// Its area was clicked.
        clicked: bool,
        /// The wheel it read while the pointer was over it.
        wheel: f32,
        /// The shield's scrim was clicked.
        scrim: bool,
    }

    /// Runs frames of a clickable area under an overlay (without one when `shielded` is false) with these events.
    fn under(shielded: bool, frames: Vec<Vec<Event>>) -> Vec<Heard> {
        let ctx = context();
        let area = Rect::from_min_size(pos2(300.0, 200.0), vec2(600.0, 400.0));
        let dialog = Rect::from_min_size(pos2(500.0, 300.0), vec2(200.0, 100.0));
        frames
            .into_iter()
            .map(|events| {
                let input = RawInput { screen_rect: Some(window()), events, ..Default::default() };
                let mut heard = Heard::default();
                ctx.run_ui(input, |ui| {
                    let response = ui.interact(area, Id::unique("under"), Sense::CLICK);
                    heard.hovered = ui.rect_contains_pointer(area);
                    heard.clicked = response.clicked();
                    if heard.hovered {
                        heard.wheel = ui.input(|input| input.smooth_scroll_delta().y);
                    }
                    if shielded {
                        show(ui.ctx(), Id::unique("overlay"), window(), |ui| {
                            heard.scrim = shield(ui, window(), Id::unique("shield")).clicked();
                            pane(ui, dialog, Id::unique("pane"));
                        });
                    }
                })
                .drop_without_applying_deltas();
                heard
            })
            .collect()
    }

    fn click_at(at: Pos2) -> Vec<Vec<Event>> {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let wheel = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -40.0),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        };
        let mut frames = vec![Vec::new(), Vec::new(), vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)]];
        frames.extend((0..5).map(|_| vec![wheel.clone()]));
        frames
    }

    #[test]
    fn nothing_under_a_shield_hears_a_click_or_a_wheel_turn() {
        let at = pos2(350.0, 250.0);
        // Without the shield the area hears it all: the test can tell.
        let bare = under(false, click_at(at));
        assert!(bare.iter().any(|heard| heard.hovered));
        assert!(bare.iter().any(|heard| heard.clicked));
        assert!(bare.iter().map(|heard| heard.wheel).sum::<f32>() != 0.0);
        // With it, nothing: no hover, no click, no wheel; the click was the scrim's.
        let shielded = under(true, click_at(at));
        assert!(shielded.iter().all(|heard| !heard.hovered && !heard.clicked && heard.wheel == 0.0), "{shielded:?}");
        assert_eq!(shielded.iter().filter(|heard| heard.scrim).count(), 1, "{shielded:?}");
    }

    #[test]
    fn a_click_on_the_pane_is_not_a_click_on_the_scrim() {
        let heard = under(true, click_at(pos2(600.0, 350.0)));
        assert!(heard.iter().all(|heard| !heard.scrim && !heard.clicked), "{heard:?}");
    }

    #[test]
    fn a_tab_moves_to_the_next_round_to_the_start_and_shift_tab_back() {
        let ids = [Id::unique("a"), Id::unique("b"), Id::unique("c")];
        assert_eq!(step(&ids, None, false), Some(ids[0]));
        assert_eq!(step(&ids, None, true), Some(ids[2]));
        assert_eq!(step(&ids, Some(ids[0]), false), Some(ids[1]));
        assert_eq!(step(&ids, Some(ids[2]), false), Some(ids[0]));
        assert_eq!(step(&ids, Some(ids[0]), true), Some(ids[2]));
        assert_eq!(step(&ids, Some(ids[1]), true), Some(ids[0]));
        assert_eq!(step(&ids, Some(Id::unique("elsewhere")), false), Some(ids[0]));
        assert_eq!(step(&[], Some(ids[0]), false), None);
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    /// A field under an overlay of three buttons: runs a frame with `events`, and says which button was pressed
    /// and whether the field's text changed.
    struct Walked {
        ctx: Context,
        text: String,
        ids: [Id; 3],
    }

    impl Walked {
        fn new() -> Walked {
            Walked { ctx: context(), text: String::new(), ids: [Id::unique("cancel"), Id::unique("keep"), Id::unique("go")] }
        }

        fn field() -> Id {
            Id::unique("field")
        }

        fn frame(&mut self, events: Vec<Event>, overlay: bool) -> Vec<usize> {
            let input = RawInput { screen_rect: Some(window()), events, ..Default::default() };
            let mut pressed = Vec::new();
            let (text, ids) = (&mut self.text, self.ids);
            self.ctx
                .run_ui(input, |ui| {
                    ui.put(Rect::from_min_size(pos2(20.0, 20.0), vec2(200.0, 30.0)), TextEdit::singleline(text).id(Walked::field()));
                    if overlay {
                        show(ui.ctx(), Id::unique("overlay"), window(), |ui| {
                            shield(ui, window(), Id::unique("shield"));
                            walk(ui, &ids);
                            let places = buttons(Rect::from_min_size(pos2(500.0, 300.0), vec2(400.0, 100.0)), &[100.0; 3]);
                            for (index, (place, id)) in places.into_iter().zip(ids).enumerate() {
                                if widgets::button(ui, place, "Go", Kind::Plain, theme::palette(Mode::Light), id).clicked() {
                                    pressed.push(index);
                                }
                            }
                        });
                    }
                })
                .drop_without_applying_deltas();
            pressed
        }

        fn focused(&self) -> Option<Id> {
            self.ctx.memory(Memory::focused)
        }
    }

    #[test]
    fn the_focus_given_as_an_overlay_opens_is_there_in_its_first_frame() {
        let mut walked = Walked::new();
        walked.frame(Vec::new(), false);
        let go = walked.ids[2];
        walked.ctx.memory_mut(|memory| memory.request_focus(go));
        // Enter in the overlay's very first frame presses the button it was opened with.
        assert_eq!(walked.frame(vec![key(Key::Enter, Modifiers::NONE)], true), [2]);
        assert_eq!(walked.focused(), Some(go));
    }

    #[test]
    fn tab_walks_the_overlays_controls_and_enter_and_space_press_the_current_one_only() {
        let mut walked = Walked::new();
        let [cancel, keep, go] = walked.ids;
        walked.frame(Vec::new(), false);
        walked.ctx.memory_mut(|memory| memory.request_focus(Walked::field()));
        walked.frame(Vec::new(), false);
        assert_eq!(walked.focused(), Some(Walked::field()));

        // The overlay opens: the field loses the focus, and nothing has it.
        for _ in 0..3 {
            walked.frame(Vec::new(), true);
        }
        assert_eq!(walked.focused(), None);
        // Enter and Space with nothing current press nothing.
        assert_eq!(walked.frame(vec![key(Key::Enter, Modifiers::NONE), key(Key::Space, Modifiers::NONE)], true), Vec::<usize>::new());

        let tab = || vec![key(Key::Tab, Modifiers::NONE)];
        let back = || vec![key(Key::Tab, Modifiers::SHIFT)];
        walked.frame(tab(), true);
        assert_eq!(walked.focused(), Some(cancel));
        walked.frame(tab(), true);
        walked.frame(tab(), true);
        assert_eq!(walked.focused(), Some(go));
        walked.frame(tab(), true);
        assert_eq!(walked.focused(), Some(cancel), "round to the start; never to the field under the overlay");
        walked.frame(back(), true);
        assert_eq!(walked.focused(), Some(go));
        walked.frame(back(), true);
        assert_eq!(walked.focused(), Some(keep));
        // The arrows do not move it.
        for arrow in [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown] {
            walked.frame(vec![key(arrow, Modifiers::NONE)], true);
            walked.frame(Vec::new(), true);
            assert_eq!(walked.focused(), Some(keep), "{arrow:?}");
        }

        // Space and Enter press the current button, once each, and nothing else.
        assert_eq!(walked.frame(vec![key(Key::Space, Modifiers::NONE)], true), [1]);
        assert_eq!(walked.frame(vec![key(Key::Enter, Modifiers::NONE)], true), [1]);
        assert_eq!(walked.text, "", "the field under the overlay got no key");
        // Two Tabs in one frame move twice.
        walked.frame([tab(), tab()].concat(), true);
        assert_eq!(walked.focused(), Some(cancel));
    }

    /// Writes `bitmap` to `target/shots/{name}`.
    fn write(bitmap: &Bitmap, name: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots").join(name);
        crate::screenshot::write_png(&path, &bitmap.image()).unwrap();
        eprintln!("wrote {}", path.display());
    }

    /// The test's dialog: its size, and its three buttons from the left with their kinds.
    const SIZE: [f32; 2] = [480.0, 170.0];
    const BUTTONS: [(&str, Kind); 3] = [("Cancel", Kind::Plain), ("Remove", Kind::Danger), ("Update", Kind::Main)];
    const TITLE: &str = "Update 2 repositories?";
    const BODY: &str = "api and web are pulled from their origins.";

    /// A picture of the demo's Repositories screen over the lake in `mode`, with a test dialog over it when
    /// `dialog`: the scrim, the dialog's glass, its title and line, and three buttons, the main one current. Gives
    /// back the picture and the dialog's main button.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss, reason = "pixel places on a 1440 by 900 window")]
    fn render(mode: Mode, dialog: bool) -> (Bitmap, Rect) {
        let palette = theme::palette(mode);
        let size = vec2(WINDOW[0], WINDOW[1]);
        let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: "lake".to_owned(), blur: true };
        let (wall, mut bitmap) = picture::wallpaper_window(&scene);
        let ctx = context();
        let mut session = Session::Demo(Box::new(demo::repos()));
        let layout = shell::layout(size, true, true);
        let input = || RawInput { screen_rect: Some(window()), ..Default::default() };
        let mut plan = None;
        ctx.run_ui(input(), |ui| plan = repos_screen::plan(ui.painter(), &layout, &session)).drop_without_applying_deltas();
        let plan = plan.unwrap();

        // The glass under the screen, laid on the wallpaper as the window paints it before the screen.
        let mut wants = shell::wants(&layout);
        plan.add_wants(&mut wants);
        for want in wants.as_slice() {
            let pane = pane_picture(&wall, &scene, want).unwrap();
            let (left, top) = (want.rect.min.x.round() as usize, want.rect.min.y.round() as usize);
            for y in 0..pane.h.min(bitmap.h - top) {
                for x in 0..pane.w.min(bitmap.w - left) {
                    let at = (top + y) * bitmap.w + left + x;
                    bitmap.px[at] = over(bitmap.px[at], pane.px[y * pane.w + x]);
                }
            }
        }
        let panel = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
        let sheet = Sheet { texture: TextureId::User(1), uv: panel.uv, edge: panel.edge };
        let rect = dialog_rect(window(), vec2(SIZE[0], SIZE[1]));
        let glass_picture = pane_picture(&wall, &scene, &want_dialog(rect)).unwrap();
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(rect.width() / glass_picture.w as f32, rect.height() / glass_picture.h as f32));
        let drawn = [Drawn { id: DIALOG, texture: TextureId::User(2), rect, uv }];
        let mut textures = HashMap::from([(sheet.texture, panel.bitmap.image()), (TextureId::User(2), glass_picture.image())]);

        let ids = BUTTONS.map(|(words, _)| Id::unique(("test-dialog", words)));
        ctx.memory_mut(|memory| memory.request_focus(ids[2]));
        let mut screen = Screen::default();
        let mut main = Rect::NOTHING;
        let mut shapes = Vec::new();
        // The first frames lay the faces out and size the overlay's layer; the last is the one painted.
        for _ in 0..3 {
            let mut output = ctx.run_ui(input(), |ui| {
                let status = repos_screen::status(&session);
                let shown = Shown { mode, waiting: 0, status: status.as_ref() };
                shell::draw(ui, &layout, &mut true, &mut Page::Repositories, shown);
                repos_screen::show(ui, &layout, Some(&plan), &mut session, Some(&sheet), mode, &mut screen, !dialog);
                if !dialog {
                    return;
                }
                show(ui.ctx(), Id::unique("test-overlay"), window(), |ui| {
                    shield(ui, window(), Id::unique("test-shield"));
                    scrim(ui, window(), palette);
                    glass(ui, &drawn);
                    pane(ui, rect, Id::unique("test-pane"));
                    let inside = rect.shrink(DIALOG_PAD);
                    let painter = ui.painter().clone();
                    let title =
                        painter.text(inside.left_top(), egui::Align2::LEFT_TOP, TITLE, font(DIALOG_TITLE, assets::medium_family()), palette.on_glass);
                    painter.text(
                        pos2(inside.left(), title.bottom() + DIALOG_GAP),
                        egui::Align2::LEFT_TOP,
                        BODY,
                        font(TEXT, FontFamily::Proportional),
                        palette.quiet,
                    );
                    walk(ui, &ids);
                    let widths = BUTTONS.map(|(words, _)| widgets::button_width(ui, words));
                    for ((place, (words, kind)), id) in buttons(inside, &widths).into_iter().zip(BUTTONS).zip(ids) {
                        widgets::button(ui, place, words, kind, palette, id);
                        if kind == Kind::Main {
                            main = place;
                        }
                    }
                });
            });
            picture::take_textures(std::mem::take(&mut output.textures_delta), &mut textures);
            shapes = std::mem::take(&mut output.shapes);
        }
        picture::paint(&mut bitmap, &ctx.tessellate(shapes, 1.0), &textures);
        (bitmap, main)
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel places on a 1440 by 900 window")]
    fn the_overlay_is_drawn_over_the_screen_in_both_modes() {
        let pixel = |bitmap: &Bitmap, at: Pos2| bitmap.px[at.y as usize * bitmap.w + at.x as usize];
        for (mode, name) in [(Mode::Light, "overlay-light.png"), (Mode::Dark, "overlay-dark.png")] {
            let (with, main) = render(mode, true);
            write(&with, name);
            let (without, _) = render(mode, false);
            // Away from the dialog, the scrim darkens the screen.
            let away = pos2(400.0, 200.0);
            assert!(luminance(pixel(&with, away)) < luminance(pixel(&without, away)), "{mode:?}: the scrim did not darken the screen");
            // The main button is the accent, and the current one: its ring, FOCUS_GAP out, against the glass past it.
            assert!(main.is_positive(), "{mode:?}: no main button was drawn");
            let accent = palette_rgb(theme::palette(mode).accent);
            let fill = pixel(&with, pos2(main.left() + 4.0, main.center().y));
            assert!((0..3).all(|i| (fill[i] - accent[i]).abs() < 0.02), "{mode:?}: the main button is {fill:?}, the accent {accent:?}");
            let ring = pixel(&with, pos2(main.right() + theme::FOCUS_GAP + theme::FOCUS_RING / 2.0, main.center().y));
            let glass = pixel(&with, pos2(main.right() + theme::FOCUS_GAP + theme::FOCUS_RING + 6.0, main.center().y));
            eprintln!("{mode:?}: the focus ring's contrast on the dialog's glass is {:.2}", ratio(luminance(ring), luminance(glass)));
        }
    }

    /// A colour's red, green and blue, 0 to 1, as the pictures hold them.
    fn palette_rgb(colour: Color32) -> [f32; 3] {
        [colour.r(), colour.g(), colour.b()].map(|channel| f32::from(channel) / 255.0)
    }
}
