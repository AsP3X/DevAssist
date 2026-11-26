//! What every menu and dialog shares: where it stands, the layer it is drawn on, the shield and the scrim under
//! it, its glass pane, its buttons and how the keyboard walks them.
//!
//! **[`open`] is the one way to open an overlay**, in the frame the key or press that asks for it comes, as soon
//! as that is known (before the screen is drawn when the window can tell, else right after). It gives the focus
//! to the control Enter should press, if any, and makes sure the key or the press that opened the overlay does
//! nothing in it: its Enter or Space presses no button (not in that frame, not while the key is held), a held
//! letter types nothing into its field, and the rest of its click (a double click's second click) does not land
//! on the scrim. From the moment it is called, nothing under the overlay is hovered or turned by the wheel.
//!
//! Then, every frame while it is open, the overlay is drawn after the screen, in [`show`]: a layer of its own
//! above the screen's, which egui treats as modal. On it come, in this order, the [`shield`] (one area over the
//! whole window that takes every click and wheel turn, and says whether the scrim itself was clicked), for a
//! dialog the [`scrim`], the pane's [`glass`] (baked from the wallpaper like every pane, from [`want_dialog`] or
//! [`want_menu`]), the [`pane`] that keeps a click on the pane's empty part off the scrim, then [`walk`] and the
//! controls. The screen under an open overlay ([`is_open`]) is told it has no keyboard and takes no press
//! (`repos_screen::show` takes `keys`). It closes when it is no longer shown. Only the window's own top row stays
//! above it ([`above`]): the window can be moved, minimised, maximised and closed under a dialog.
//!
//! The keyboard: only the controls in an overlay take egui's focus ([`crate::widgets::button`]). [`walk`] moves it
//! among them on Tab and Shift-Tab, and nowhere else; Enter and Space press the one that has it, and nothing else.
//! With no control given to [`open`], Enter and Space do nothing until Tab or Shift-Tab has chosen one, and either
//! chooses the first control, never the last.

use egui::{
    Align2, Area, AreaState, Color32, Context, Event, FocusDirection, Id, Key, LayerId, Memory, Mesh, Order, PointerButton, Rect, Response, Sense,
    Ui, Vec2, pos2, vec2,
};

use crate::baker::{Drawn, Want};
use crate::theme::{BUTTON_GAP, BUTTON_HEIGHT, DIALOG_MARGIN, DIALOG_RADIUS, MENU_GAP, MENU_RADIUS, Palette, SUBMENU_GAP, Surface};

/// The glass pane of a dialog.
pub const DIALOG: &str = "dialog";
/// The glass pane of a menu.
pub const MENU: &str = "menu";
/// The glass pane of the side menu a menu's line opens, beside the menu: a second pane of the menu's overlay.
pub const SUBMENU: &str = "submenu";

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

/// The glass pane of a menu's side menu at `rect`, to add to the panes baked this frame: the one way a side menu
/// gets its glass, as [`want_menu`] gives the menu its own.
#[must_use]
pub fn want_submenu(rect: Rect) -> Want {
    Want { id: SUBMENU, rect, radius: MENU_RADIUS, surface: Surface::Menu }
}

/// Whether the baked picture named `id` is an overlay's pane, which is drawn on the overlay over its scrim
/// ([`glass`]), not under the screen.
#[must_use]
pub fn is_overlay_pane(id: &str) -> bool {
    id == DIALOG || id == MENU || id == SUBMENU
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

/// Where the side menu of `size` stands in `window` for the line `line` of the menu at `menu`: at the menu's left,
/// [`SUBMENU_GAP`] away, its bottom level with the line's; at the menu's right when the left has no room. Neither
/// side leaves the window's [`DIALOG_MARGIN`]: in a window too narrow for both, it lies over the menu at the side
/// with more room. It is pushed up or down to stay inside the margin, and cut to the room when larger.
#[must_use]
pub fn submenu_rect(menu: Rect, line: Rect, size: Vec2, window: Rect) -> Rect {
    let room = window.shrink(DIALOG_MARGIN);
    let size = size.min(room.size()).max(Vec2::ZERO);
    let left = menu.left() - SUBMENU_GAP - size.x;
    let right = menu.right() + SUBMENU_GAP;
    let x = if left >= room.left() {
        left
    } else if right + size.x <= room.right() {
        right
    } else if menu.left() - room.left() >= room.right() - menu.right() {
        room.left()
    } else {
        room.right() - size.x
    };
    let y = (line.bottom() - size.y).min(room.bottom() - size.y).max(room.top());
    Rect::from_min_size(pos2(x, y), size)
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

/// The keys a button takes as a press.
const PRESS_KEYS: [Key; 2] = [Key::Enter, Key::Space];

/// What the open overlay keeps from one frame to the next.
#[derive(Clone, Debug, Default)]
struct Opening {
    /// The pass it was opened in.
    pass: u64,
    /// The last pass it was shown in.
    shown: Option<u64>,
    /// Enter and Space, when they were down as it opened: until they come up, their presses (the key's repeats)
    /// are not the overlay's.
    held: Vec<Key>,
    /// Every key that was down as it opened (the letter that asked for it, say): until all of them come up, no text
    /// is typed in it, so a held `m` does not repeat into the group dialog's field.
    typing: Vec<Key>,
    /// How many frames had a pointer button pressed on the overlay after the pass it opened in: a click whose
    /// presses (one, two for a double click, three for a triple) all came since is a person's click on the overlay,
    /// not the rest of the gesture that opened it ([`fresh_click`]).
    presses: u32,
}

/// Where egui keeps the [`Opening`].
fn opening_id() -> Id {
    Id::unique("overlay-opening")
}

/// The overlay's layer: one overlay is open at a time.
#[must_use]
pub fn layer() -> LayerId {
    LayerId::new(Order::Foreground, Id::unique("overlay"))
}

/// The open overlay's [`Opening`], if one was opened.
fn opening(ctx: &Context) -> Option<Opening> {
    ctx.data(|data| data.get_temp::<Opening>(opening_id()))
}

/// Opens an overlay, the one way to: gives egui's keyboard focus to `focus` (the control Enter should press; none
/// takes the focus from whatever held it), and keeps what opened it from acting in it. This frame's presses of
/// Enter and Space are taken, and while either stays down its repeats are no press; while any key that was down
/// stays down, no text is typed in it; a click on the scrim counts only once a button was pressed after this
/// frame, and never as the second click of a double click. From now on nothing under the overlay's layer is
/// hovered or turned by the wheel, also for the rest of this frame.
pub fn open(ctx: &Context, focus: Option<Id>) {
    let (held, typing) = ctx.input_mut(|input| {
        input.events.retain(|event| !matches!(event, Event::Key { key: Key::Enter | Key::Space, pressed: true, .. }));
        (PRESS_KEYS.into_iter().filter(|key| input.key_down(*key)).collect(), input.keys_down.iter().copied().collect())
    });
    let opened = Opening { pass: ctx.cumulative_pass_nr(), shown: None, held, typing, presses: 0 };
    ctx.data_mut(|data| data.insert_temp(opening_id(), opened));
    seed(ctx, layer(), ctx.content_rect());
    ctx.memory_mut(|memory| match (focus, memory.focused()) {
        (Some(id), _) => memory.request_focus(id),
        (None, Some(other)) => memory.surrender_focus(other),
        (None, None) => {}
    });
}

/// Whether an overlay is open: opened this frame, or shown in the last one. The screen under it then has no
/// keyboard and takes no press.
#[must_use]
pub fn is_open(ctx: &Context) -> bool {
    let now = ctx.cumulative_pass_nr();
    opening(ctx).is_some_and(|opening| opening.pass == now || opening.shown.is_some_and(|shown| shown + 1 >= now))
}

/// Runs `contents` on the overlay's own layer, over the whole window and above the screen, and gives back what it
/// returned. Called every frame the overlay is open, after the screen; an overlay not shown in a frame is closed.
/// One shown without [`open`] is opened here, with no control given the focus. The layer is modal: nothing under
/// it gets the pointer (no hover, no click, no wheel) or the keyboard's focus. It appears at once, whole, with no
/// fade.
pub fn show<R>(ctx: &Context, contents: impl FnOnce(&mut Ui) -> R) -> R {
    if !is_open(ctx) {
        open(ctx, None);
    }
    let now = ctx.cumulative_pass_nr();
    let mut opened = opening(ctx).unwrap_or_default();
    ctx.input_mut(|input| {
        // A held Enter or Space repeats; its presses up to its release are the key that opened the overlay.
        let mut held = std::mem::take(&mut opened.held);
        input.events.retain(|event| match event {
            Event::Key { key, pressed, .. } if held.contains(key) => {
                if !*pressed {
                    held.retain(|known| known != key);
                }
                !*pressed
            }
            _ => true,
        });
        held.retain(|key| input.key_down(*key));
        opened.held = held;
        // The text of a key held since the overlay opened (its repeats) is not the overlay's.
        opened.typing.retain(|key| input.key_down(*key));
        if !opened.typing.is_empty() {
            input.events.retain(|event| !matches!(event, Event::Text(_)));
        }
        if now > opened.pass && input.pointer.any_pressed() {
            opened.presses = opened.presses.saturating_add(1);
        }
    });
    opened.shown = Some(now);
    ctx.data_mut(|data| data.insert_temp(opening_id(), opened));
    area(ctx, layer(), ctx.content_rect(), |ui| {
        let layer = ui.layer_id();
        ui.ctx().memory_mut(|memory| memory.set_modal_layer(layer));
        contents(ui)
    })
}

/// Runs `contents` on a small layer `id` at `rect`, above an open overlay: for the window's own top row, which
/// moves, minimises, maximises and closes the window under a dialog as everywhere else.
pub fn above<R>(ctx: &Context, id: Id, rect: Rect, contents: impl FnOnce(&mut Ui) -> R) -> R {
    area(ctx, LayerId::new(Order::Tooltip, id), rect, contents)
}

/// Tells egui the area of `layer` is at `rect`, whole and touchable. A new area is otherwise laid out unseen and
/// untouchable in its first frame, to learn its size, and loses the focus given to what is in it.
fn seed(ctx: &Context, layer: LayerId, rect: Rect) {
    let state = AreaState {
        pivot_pos: Some(rect.min),
        pivot: Align2::LEFT_TOP,
        size: Some(rect.size()),
        interactable: true,
        ..AreaState::load(ctx, layer.id).unwrap_or_default()
    };
    ctx.memory_mut(|memory| memory.areas_mut().set_state(layer, state));
}

/// Runs `contents` on the area of `layer` at `rect`, there from its first frame ([`seed`]), clipped to `rect`.
fn area<R>(ctx: &Context, layer: LayerId, rect: Rect, contents: impl FnOnce(&mut Ui) -> R) -> R {
    seed(ctx, layer, rect);
    Area::new(layer.id)
        .order(layer.order)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            ui.expand_to_include_rect(rect);
            contents(ui)
        })
        .inner
}

/// The shield of the overlay in `ui` (from [`show`]): one area over the whole `window` that takes every click and
/// wheel turn, so nothing under it gets them. Says whether the scrim itself was clicked: not what lies over it on
/// the overlay ([`pane`], the controls), not the rest of the click that opened the overlay, not the second click
/// of a double click. Added before anything else on the overlay.
pub fn shield(ui: &mut Ui, window: Rect, id: Id) -> bool {
    fresh_click(ui, &ui.interact(window, id, Sense::CLICK))
}

/// Whether `response`, of a control on the open overlay, was pressed by a person in this frame: by the keyboard
/// (Enter or Space on the control with the focus; the key that opened the overlay is already kept from it, see
/// [`open`]), or by a click whose every press came after the overlay opened. A press that is part of the gesture
/// that opened the overlay never acts inside it: not the rest of the click that opened it, not the second click of
/// a double click (nor the second and third of a triple click) whose first click opened it, also when the overlay
/// it opened has since handed over to another (a menu's line that opens a dialog). A double click made wholly on
/// the overlay is a click as egui says it is. Every control on an overlay is asked through this, never through
/// `clicked` alone: the scrim ([`shield`]), the dialogs' buttons ([`crate::widgets::button`]), the rows of their
/// lists and the menu's lines.
#[must_use]
pub fn fresh_click(ui: &Ui, response: &Response) -> bool {
    if !response.clicked() {
        return false;
    }
    if !response.clicked_by(PointerButton::Primary) {
        // Enter or Space on the control with the focus.
        return true;
    }
    opening(ui.ctx()).is_some_and(|opening| opening.presses >= clicks(response))
}

/// How many presses the click on `response` is made of: three for a triple click, two for a double click, else one.
fn clicks(response: &Response) -> u32 {
    if response.triple_clicked() {
        3
    } else if response.double_clicked() {
        2
    } else {
        1
    }
}

/// What a menu's open side menu keeps from one frame to the next: as [`Opening`] for the overlay, for the side menu
/// that opens on the menu's overlay without opening another.
#[derive(Clone, Debug, Default)]
struct SideOpening {
    /// The pass it was opened in.
    pass: u64,
    /// The last pass it was shown in.
    shown: Option<u64>,
    /// Every key that was down as it opened (the Enter, the Right or the letter that asked for it): until each comes
    /// up, its presses (its repeats) are not the side menu's, and while any is down, no text is.
    held: Vec<Key>,
    /// How many frames had a pointer button pressed after the pass it opened in ([`fresh_submenu_click`]).
    presses: u32,
}

/// Where egui keeps the [`SideOpening`].
fn side_opening_id() -> Id {
    Id::unique("overlay-side-opening")
}

/// Opens a menu's side menu on the open overlay, the one way the window opens one: from now on the keys that are
/// down (the one that asked for it) and the rest of the click that asked for it do nothing in it
/// ([`submenu_events`], [`fresh_submenu_click`]). Its glass comes with the next bake ([`want_submenu`]).
pub fn open_submenu(ctx: &Context) {
    let held = ctx.input(|input| input.keys_down.iter().copied().collect());
    let opened = SideOpening { pass: ctx.cumulative_pass_nr(), shown: None, held, presses: 0 };
    ctx.data_mut(|data| data.insert_temp(side_opening_id(), opened));
}

/// This frame's events for the open side menu: without the presses of a key held since it opened (its repeats), and
/// without text while such a key is down. Called once every frame the side menu is open, before its lines are asked
/// for clicks; one the core opened without [`open_submenu`] is opened here, with the keys down now held.
#[must_use]
pub fn submenu_events(ctx: &Context) -> Vec<Event> {
    let now = ctx.cumulative_pass_nr();
    let kept = ctx
        .data(|data| data.get_temp::<SideOpening>(side_opening_id()))
        .filter(|opened| opened.pass + 1 >= now || opened.shown.is_some_and(|shown| shown + 1 >= now));
    let mut opened = kept.unwrap_or_else(|| SideOpening {
        pass: now,
        shown: None,
        held: ctx.input(|input| input.keys_down.iter().copied().collect()),
        presses: 0,
    });
    let events = ctx.input(|input| {
        let held = opened.held.clone();
        opened.held.retain(|key| input.key_down(*key));
        if now > opened.pass && input.pointer.any_pressed() {
            opened.presses = opened.presses.saturating_add(1);
        }
        let typing = !opened.held.is_empty();
        input
            .events
            .iter()
            .filter(|event| match event {
                Event::Key { key, pressed: true, .. } => !held.contains(key),
                Event::Text(_) => !typing,
                _ => true,
            })
            .cloned()
            .collect()
    });
    opened.shown = Some(now);
    ctx.data_mut(|data| data.insert_temp(side_opening_id(), opened));
    events
}

/// The keys that were down as a menu's side menu closed (the Esc, `q` or Left that closed it): until each comes up,
/// its presses (its repeats) are not the menu's, and while any is down, no text is.
#[derive(Clone, Debug, Default)]
struct SideClosing {
    held: Vec<Key>,
}

/// Where egui keeps the [`SideClosing`].
fn side_closing_id() -> Id {
    Id::unique("overlay-side-closing")
}

/// Closes a menu's side menu on the open overlay, as the window closes one: from now on the keys that are down (the
/// one that closed it) do nothing in the menu while they stay down ([`menu_events`]).
pub fn close_submenu(ctx: &Context) {
    let held = ctx.input(|input| input.keys_down.iter().copied().collect());
    ctx.data_mut(|data| data.insert_temp(side_closing_id(), SideClosing { held }));
}

/// This frame's events for an open menu whose side menu is closed: without the presses of a key held since the side
/// menu closed (its repeats), and without text while such a key is down. Called once every such frame.
#[must_use]
pub fn menu_events(ctx: &Context) -> Vec<Event> {
    let mut closing = ctx.data(|data| data.get_temp::<SideClosing>(side_closing_id())).unwrap_or_default();
    let events = ctx.input(|input| {
        let held = closing.held.clone();
        closing.held.retain(|key| input.key_down(*key));
        let typing = !closing.held.is_empty();
        input
            .events
            .iter()
            .filter(|event| match event {
                Event::Key { key, pressed: true, .. } => !held.contains(key),
                Event::Text(_) => !typing,
                _ => true,
            })
            .cloned()
            .collect()
    });
    ctx.data_mut(|data| data.insert_temp(side_closing_id(), closing));
    events
}

/// Whether `response`, of a line of a menu whose side menu is open, was clicked by a person: as [`fresh_click`], and
/// with every press of the click made after the side menu opened, so the second click of a double click whose first
/// click opened it does nothing in it, nor on the menu's line that opened it.
#[must_use]
pub fn fresh_submenu_click(ui: &Ui, response: &Response) -> bool {
    if !fresh_click(ui, response) {
        return false;
    }
    !response.clicked_by(PointerButton::Primary)
        || ui.ctx().data(|data| data.get_temp::<SideOpening>(side_opening_id())).is_some_and(|opened| opened.presses >= clicks(response))
}

/// Lays the scrim over `window`, under a dialog. A menu has none.
pub fn scrim(ui: &Ui, window: Rect, palette: Palette) {
    ui.painter().rect_filled(window, 0.0, palette.scrim);
}

/// Paints the overlays' panes among `drawn` (the baker's pictures of this frame) on the overlay in `ui`, over its
/// scrim; the others are painted under the screen. A menu's side menu is not among them: its menu paints its glass
/// ([`submenu_glass`]) once it knows the side menu is shown in this frame.
pub fn glass(ui: &Ui, drawn: &[Drawn]) {
    paint_panes(ui, drawn, |id| is_overlay_pane(id) && id != SUBMENU);
}

/// Paints the glass of a menu's side menu among `drawn` on the overlay in `ui`, under the side menu's lines.
pub fn submenu_glass(ui: &Ui, drawn: &[Drawn]) {
    paint_panes(ui, drawn, |id| id == SUBMENU);
}

/// Paints the pictures among `drawn` whose id `which` takes.
fn paint_panes(ui: &Ui, drawn: &[Drawn], which: impl Fn(&str) -> bool) {
    for pane in drawn.iter().filter(|drawn| which(drawn.id)) {
        let mut mesh = Mesh::with_texture(pane.texture);
        mesh.add_rect_with_uv(pane.rect, pane.uv, Color32::WHITE);
        ui.painter().add(mesh);
    }
}

/// Takes the clicks on the pane at `rect`, so that a click on its empty part is not a click on the scrim.
pub fn pane(ui: &Ui, rect: Rect, id: Id) {
    ui.interact(rect, id, Sense::CLICK);
}

/// Keeps egui's keyboard focus among the controls `ids`, in their Tab order: Tab gives it to the next, Shift-Tab to
/// the one before, round from the end to the start. When none has it, either key gives it to the first: a dialog
/// lists its controls so that the first is the safe one (Cancel), and one key press never lands on the last, which
/// may be the one that deletes.
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
        // From no focus, both ways start at the first control, never at the last.
        (None, _) => 0,
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
    #[expect(clippy::cast_precision_loss, reason = "window sizes and places of a few thousand points")]
    fn the_side_menu_stays_inside_the_window() {
        use crate::theme::{SUBMENU_GAP, SUBMENU_WIDTH};
        // The drawing's menu with Migrate open: its line from 616 to 648, the placed menu of two lines at its left.
        let (menu_size, size) = (vec2(256.0, 314.0), vec2(SUBMENU_WIDTH, 72.0));
        let line_of = |menu: Rect| Rect::from_min_size(pos2(menu.left() + 4.0, menu.top() + 4.0 + 7.0 * 32.0 + 9.0), vec2(248.0, 32.0));
        let menu = Rect::from_min_size(pos2(1164.0, 274.0), vec2(256.0, 415.0));
        let placed = submenu_rect(menu, Rect::from_min_size(pos2(1168.0, 616.0), vec2(248.0, 32.0)), size, window());
        assert_eq!(placed, Rect::from_min_max(pos2(892.0, 648.0 - 72.0), pos2(1156.0, 648.0)));
        // A menu at the window's left: the placed menu flips to its right.
        let left = Rect::from_min_size(pos2(DIALOG_MARGIN, 274.0), menu_size);
        let placed = submenu_rect(left, line_of(left), size, window());
        assert_eq!((placed.left(), placed.bottom()), (left.right() + SUBMENU_GAP, line_of(left).bottom()));
        // Every window from 960 by 600 up, a menu at buttons all along each edge and across the middle: the placed
        // menu is whole inside the window's margin, level with its line, never over the menu, at the left when
        // there is room there.
        for width in (960..=2560).step_by(160) {
            for height in (600..=1440).step_by(120) {
                let (w, h) = (width as f32, height as f32);
                let window = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
                let room = window.shrink(DIALOG_MARGIN);
                let mut anchors = Vec::new();
                for x in (0..=width).step_by(16) {
                    for y in [0.0, h / 2.0, h - 32.0] {
                        anchors.push(pos2(x as f32, y));
                    }
                }
                for y in (0..=height).step_by(16) {
                    for x in [0.0, w / 2.0, w - 32.0] {
                        anchors.push(pos2(x, y as f32));
                    }
                }
                for at in anchors {
                    let anchor = Rect::from_min_size(at, vec2(32.0, 32.0)).intersect(window);
                    let menu = menu_rect(anchor, menu_size, window);
                    let line = line_of(menu);
                    let placed = submenu_rect(menu, line, size, window);
                    let what = format!("{placed:?} for the menu {menu:?} in {w}x{h}");
                    assert!(room.contains_rect(placed), "{what} leaves the window");
                    assert_eq!(placed.size(), size, "{what} is cut");
                    assert!((placed.bottom() - line.bottom()).abs() < 1e-3, "{what} is not level with its line");
                    assert!(
                        placed.right() + SUBMENU_GAP <= menu.left() + 1e-3 || placed.left() + 1e-3 >= menu.right() + SUBMENU_GAP,
                        "{what} is over the menu"
                    );
                    if menu.left() - SUBMENU_GAP - size.x >= room.left() {
                        assert!((placed.right() + SUBMENU_GAP - menu.left()).abs() < 1e-3, "{what} is not at the menu's left");
                    }
                }
            }
        }
        // A window too narrow for it at either placed: over the menu, at the placed with more room, still inside.
        let narrow = Rect::from_min_size(Pos2::ZERO, vec2(520.0, 600.0));
        let menu = Rect::from_min_size(pos2(264.0, 40.0), menu_size);
        let placed = submenu_rect(menu, line_of(menu), size, narrow);
        assert_eq!((placed.left(), placed.bottom(), placed.size()), (DIALOG_MARGIN, line_of(menu).bottom(), size));
        let menu = Rect::from_min_size(pos2(0.0, 40.0), menu_size);
        let placed = submenu_rect(menu, line_of(menu), size, narrow);
        assert_eq!(placed.right(), 520.0 - DIALOG_MARGIN);
        // Low in a short window: pushed up to stay inside.
        let short = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 300.0));
        let menu = Rect::from_min_size(pos2(1164.0, 24.0), vec2(256.0, 252.0));
        let line = Rect::from_min_size(pos2(1168.0, 260.0), vec2(248.0, 32.0));
        assert_eq!(submenu_rect(menu, line, size, short).bottom(), 300.0 - DIALOG_MARGIN);
    }

    #[test]
    fn a_side_menu_is_an_overlay_pane() {
        assert!(is_overlay_pane(SUBMENU) && is_overlay_pane(MENU) && is_overlay_pane(DIALOG));
        assert!(!is_overlay_pane("sidebar"));
        let rect = Rect::from_min_size(pos2(892.0, 576.0), vec2(264.0, 72.0));
        assert_eq!(want_submenu(rect), Want { id: SUBMENU, rect, radius: MENU_RADIUS, surface: Surface::Menu });
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

    /// Runs frames of a clickable area under an overlay with these events. The overlay is opened in the frame at
    /// `open_at` (never when `None`), before the area is drawn, and shown from then on, after it.
    fn under(open_at: Option<usize>, frames: Vec<Vec<Event>>) -> Vec<Heard> {
        let ctx = context();
        let area = Rect::from_min_size(pos2(300.0, 200.0), vec2(600.0, 400.0));
        let dialog = Rect::from_min_size(pos2(500.0, 300.0), vec2(200.0, 100.0));
        frames
            .into_iter()
            .enumerate()
            .map(|(at, events)| {
                let input = RawInput { screen_rect: Some(window()), events, ..Default::default() };
                let mut heard = Heard::default();
                let shown = open_at.is_some_and(|open_at| at >= open_at);
                ctx.run_ui(input, |ui| {
                    if open_at == Some(at) {
                        open(ui.ctx(), None);
                    }
                    let response = ui.interact(area, Id::unique("under"), Sense::CLICK);
                    heard.hovered = ui.rect_contains_pointer(area);
                    heard.clicked = response.clicked();
                    if heard.hovered {
                        heard.wheel = ui.input(|input| input.smooth_scroll_delta().y);
                    }
                    if shown {
                        show(ui.ctx(), |ui| {
                            heard.scrim = shield(ui, window(), Id::unique("shield"));
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
        let bare = under(None, click_at(at));
        assert!(bare.iter().any(|heard| heard.hovered));
        assert!(bare.iter().any(|heard| heard.clicked));
        assert!(bare.iter().map(|heard| heard.wheel).sum::<f32>() != 0.0);
        // With it, nothing: no hover, no click, no wheel; the click was the scrim's.
        let shielded = under(Some(0), click_at(at));
        assert!(shielded.iter().all(|heard| !heard.hovered && !heard.clicked && heard.wheel == 0.0), "{shielded:?}");
        assert_eq!(shielded.iter().filter(|heard| heard.scrim).count(), 1, "{shielded:?}");
    }

    #[test]
    fn nothing_under_an_overlay_is_hovered_or_turned_in_the_frame_it_opens() {
        let at = pos2(350.0, 250.0);
        let wheel = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -40.0),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        };
        let frames = vec![Vec::new(), vec![Event::PointerMoved(at)], Vec::new(), vec![wheel]];
        let heard = under(Some(3), frames);
        assert!(heard[2].hovered, "the pointer is over the area before the overlay opens: {heard:?}");
        assert!(!heard[3].hovered && heard[3].wheel == 0.0, "in the frame it opens: {:?}", heard[3]);
    }

    #[test]
    fn a_click_on_the_pane_is_not_a_click_on_the_scrim() {
        let heard = under(Some(0), click_at(pos2(600.0, 350.0)));
        assert!(heard.iter().all(|heard| !heard.scrim && !heard.clicked), "{heard:?}");
    }

    #[test]
    fn the_second_click_of_a_double_click_that_opened_it_does_not_close_it() {
        let ctx = context();
        let at = pos2(350.0, 250.0);
        let target = Rect::from_center_size(at, vec2(40.0, 40.0));
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        // (seconds, events): a double click on the opener, then a single click on the scrim much later.
        let frames = [
            (0.0, vec![Event::PointerMoved(at)]),
            (0.05, vec![button(true)]),
            (0.1, vec![button(false)]),
            (0.15, vec![button(true)]),
            (0.2, vec![button(false)]),
            (0.25, Vec::new()),
            (1.5, vec![button(true)]),
            (1.6, vec![button(false)]),
        ];
        let mut opened = false;
        let mut said = Vec::new();
        for (time, events) in frames {
            let input = RawInput { screen_rect: Some(window()), time: Some(time), events, ..Default::default() };
            let mut scrim = false;
            ctx.run_ui(input, |ui| {
                if ui.interact(target, Id::unique("opener"), Sense::CLICK).clicked() && !opened {
                    open(ui.ctx(), None);
                    opened = true;
                }
                if opened {
                    show(ui.ctx(), |ui| scrim = shield(ui, window(), Id::unique("shield")));
                }
            })
            .drop_without_applying_deltas();
            said.push((opened, scrim));
        }
        assert!(said[2].0, "the first click opened it: {said:?}");
        assert!(said[..7].iter().all(|(_, scrim)| !scrim), "the double click's second click closed it: {said:?}");
        assert!(said[7].1, "a click of its own on the scrim is one: {said:?}");
    }

    #[test]
    fn a_tab_moves_to_the_next_round_to_the_start_and_shift_tab_back() {
        let ids = [Id::unique("a"), Id::unique("b"), Id::unique("c")];
        assert_eq!(step(&ids, None, false), Some(ids[0]));
        assert_eq!(step(&ids, None, true), Some(ids[0]), "from no focus, Shift-Tab starts at the first too");
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
                        show(ui.ctx(), |ui| {
                            shield(ui, window(), Id::unique("shield"));
                            walk(ui, &ids);
                            let places = buttons(Rect::from_min_size(pos2(500.0, 300.0), vec2(400.0, 100.0)), &[100.0; 3]);
                            for (index, (place, id)) in places.into_iter().zip(ids).enumerate() {
                                if widgets::button(ui, place, "Go", Kind::Plain, theme::palette(Mode::Light), id) {
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
    fn the_enter_that_opens_an_overlay_does_not_press_its_main_button() {
        let ctx = context();
        let main = Id::unique("main");
        let mut opened = false;
        let mut frame = |events: Vec<Event>| {
            let input = RawInput { screen_rect: Some(window()), events, ..Default::default() };
            let mut pressed = false;
            ctx.run_ui(input, |ui| {
                // The screen under it: Enter asks for the dialog.
                if !opened && ui.input(|input| input.key_pressed(Key::Enter)) {
                    open(ui.ctx(), Some(main));
                    opened = true;
                }
                if opened {
                    show(ui.ctx(), |ui| {
                        shield(ui, window(), Id::unique("shield"));
                        walk(ui, &[main]);
                        let place = Rect::from_min_size(pos2(600.0, 400.0), vec2(100.0, 36.0));
                        pressed = widgets::button(ui, place, "Go", Kind::Main, theme::palette(Mode::Light), main);
                    });
                }
            })
            .drop_without_applying_deltas();
            (pressed, ctx.memory(Memory::focused))
        };
        let enter = |pressed, repeat| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE };
        assert_eq!(frame(vec![enter(true, false)]), (false, Some(main)), "the Enter that opened it pressed its button");
        assert_eq!(frame(Vec::new()), (false, Some(main)), "the next frame, the key still down");
        assert_eq!(frame(vec![enter(true, true)]), (false, Some(main)), "the key's repeat");
        assert_eq!(frame(vec![enter(false, false)]), (false, Some(main)), "its release");
        assert_eq!(frame(vec![enter(true, false)]), (true, Some(main)), "a second Enter presses it");
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
        let mut session = Session::demo(demo::repos());
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
        let mut screen = Screen::default();
        let mut main = Rect::NOTHING;
        let mut shapes = Vec::new();
        // The first frames lay the faces out; the last is the one painted.
        for frame in 0..3 {
            let mut output = ctx.run_ui(input(), |ui| {
                if dialog && frame == 0 {
                    open(ui.ctx(), Some(ids[2]));
                }
                let status = repos_screen::status(&session);
                let shown = Shown { mode, waiting: 0, status: status.as_ref() };
                shell::draw(ui, &layout, &mut true, &mut Page::Repositories, shown);
                repos_screen::show(ui, &layout, Some(&plan), &mut session, Some(&sheet), mode, &mut screen, !dialog);
                if !dialog {
                    return;
                }
                show(ui.ctx(), |ui| {
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
