//! **Port-only, fork-only** (not in the game): the main menu's "Check for Update" page (docs/plan/check_for_update.md).
//! Two lines (this build's version, the newest release or the check's state), a status line, then the rows
//! **Update** (only when a newer release can be installed) and **Back**. The engine owns the network and the install
//! (rc-engine `update`): it fills the view before each front-end menu tick ([`PageMenu::set_update_view`]), starts the
//! check when the page is entered (`MenuOut::entered`) and takes ✕ on Update after it ([`PageMenu::take_update_pick`]).
//!
//! Built like the Mods page ([`super::mods`]): the Port Options' main-menu page frames with the list panel stretched,
//! a copy of the port title label, rows drawn and driven like the camera list (Up/Down without wrap with sound 1, ✕
//! with sound 0, the generic Start/Select/R3 and △ keys; ✕ on Back is △). Entry: one item "Check for Update"
//! (action 3 → [`PAGE`]) in both of the front end's Options lists, before the port's Quit Game
//! ([`super::port::front_options_insert`]).

use super::super::{text as print, MenuAssets, MenuDraw, MenuInput, MenuSound};
use super::options::{keys, right};
use super::{port, Data, Item, MenuOut, Page, PageMenu, Widget, LIGHT_BLUE, YELLOW};
use crate::pad::button;
use rc_formats::font::Font;

/// The page and its two own widgets (title label, rows).
pub const PAGE: u32 = 0x7f00_0060;
pub const TITLE_W: u32 = 0x7f00_0120;
pub const LIST_W: u32 = 0x7f00_0220;
/// The rows' callbacks (dispatched like the game's by address).
pub const UPDATE: u32 = 0x7f00_1018;
pub const DRAW: u32 = 0x7f00_101c;
/// The longest value drawn.
const VALUE_MAX: usize = 40;

/// What the engine shows: the build version, the newest release (or the check's state), a status line, and whether
/// Update is offered. `busy` (download / install running) holds the keys but △.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateView {
    pub current: Vec<u8>,
    pub newest: Vec<u8>,
    pub status: Vec<u8>,
    pub can_update: bool,
    pub busy: bool,
}

/// The rows' run-time state: the view, the cursor (0 = the first selectable row) and ✕ on Update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateRows {
    pub view: UpdateView,
    pub cursor: i32,
    pub pick: bool,
}

impl UpdateRows {
    /// The selectable rows: Update (when offered and not busy), Back.
    fn rows(&self) -> &'static [i32] {
        if self.view.can_update && !self.view.busy { &[port::text::UPDATE_ACTION, port::text::BACK] } else { &[port::text::BACK] }
    }
}

fn rows_mut(m: &mut PageMenu) -> Option<&mut UpdateRows> {
    match m.widgets.get_mut(&LIST_W).map(|w| &mut w.data) {
        Some(Data::Update(p)) => Some(p),
        _ => None,
    }
}

/// A string as the font can draw it: printable ASCII, at most [`VALUE_MAX`] characters.
fn drawable(s: &str) -> Vec<u8> {
    let mut b: Vec<u8> = s.bytes().map(|c| if (0x20..0x7f).contains(&c) { c } else { b'?' }).collect();
    if b.len() > VALUE_MAX {
        b.truncate(VALUE_MAX - 3);
        b.extend_from_slice(b"...");
    }
    b
}

impl PageMenu {
    /// Adds the page, its widgets and the entry of both front-end Options lists. Needs the Port Options' main-menu
    /// page ([`PageMenu::install_port_page`]), whose frames and title it copies. False when it could not be added.
    pub fn install_update_page(&mut self) -> bool {
        if self.pages.contains_key(&PAGE) { return true; }
        let Some(model) = self.pages.get(&port::FRONT_PAGE).cloned() else { return false };
        let Some(mut title) = self.widgets.get(&port::TITLE_W).cloned() else { return false };
        let Data::Label(l) = &mut title.data else { return false };
        l.id = port::text::UPDATE_ENTRY as u32;
        title.addr = TITLE_W;
        let rows = Widget { addr: LIST_W, update: UPDATE, draw: DRAW, enter: 0, leave: 0, dflags: 0, moby: 0, rect: [0; 4], raw: [0; 8], data: Data::Update(UpdateRows::default()) };
        let mut widgets = model.widgets;
        widgets[2] = TITLE_W;
        widgets[port::LIST_SLOT] = LIST_W;
        let page = Page { addr: PAGE, focus: LIST_W, widgets, ..model };
        for list in &mut self.saves.front_options {
            port::front_options_insert(list, Item { label: port::text::UPDATE_ENTRY as i16, action: 3, arg: PAGE, sublabel: 0, hl: 0 });
        }
        self.widgets.insert(TITLE_W, title);
        self.widgets.insert(LIST_W, rows);
        self.pages.insert(PAGE, page);
        true
    }

    /// The view (strings as found; non-ASCII drawn as '?'). The cursor stays on a row.
    pub fn set_update_view(&mut self, current: &str, newest: &str, status: &str, can_update: bool, busy: bool) {
        let Some(p) = rows_mut(self) else { return };
        p.view = UpdateView { current: drawable(current), newest: drawable(newest), status: drawable(status), can_update, busy };
        p.cursor = p.cursor.clamp(0, p.rows().len() as i32 - 1);
    }

    /// ✕ on Update since the last call.
    pub fn take_update_pick(&mut self) -> bool { rows_mut(self).is_some_and(|p| std::mem::take(&mut p.pick)) }
}

/// The rows' update, as the Mods list's; ✕ on Update asks the engine, ✕ on Back is △. While busy only △ (and the
/// generic close keys) work.
pub fn update(m: &mut PageMenu, w: u32, inp: &MenuInput, out: &mut MenuOut) -> i32 {
    if !m.is_focus(w) { return 0; }
    if let Some(r) = keys(m, inp, 1) { return r; }
    let Some(p) = rows_mut(m) else { return 0 };
    if p.view.busy { return 0; }
    let rows = p.rows();
    let start = p.cursor;
    if inp.pressed_u & button::UP != 0 && p.cursor != 0 { p.cursor -= 1; }
    if inp.pressed_u & button::DOWN != 0 && (p.cursor as usize + 1) < rows.len() { p.cursor += 1; }
    if p.cursor != start { out.sounds.push(MenuSound::Cursor); }
    if inp.pressed_u & button::CROSS != 0 {
        out.sounds.push(MenuSound::Confirm);
        if rows[p.cursor as usize] == port::text::UPDATE_ACTION {
            p.pick = true;
        } else {
            let parent = m.pages.get(&m.current).map_or(0, |pg| pg.parent);
            if parent != 0 { m.target = parent; }
        }
    }
    0
}

/// The rows' draw (panel-local): this version (label, then the value on its own row), label left / value right for
/// the newest, the status line, then
/// the selectable rows, spaced like the Port Options' (`h/(n+1)`).
pub fn draw(m: &mut PageMenu, w: u32, a: &MenuAssets, out: &mut Vec<MenuDraw>) -> u32 {
    let Some(wd) = m.widgets.get(&w) else { return 1 };
    let [_, _, ww, wh] = wd.rect;
    let Data::Update(p) = &wd.data else { return 1 };
    let rows = p.rows();
    let n = 4 + rows.len() as i32;
    let step = (wh / (n + 1)).max(port::ROW_STEP);
    let mut y = step - 8;
    // The build version is too long to share its label's row: it goes right-aligned on the next.
    print(out, Font::Regular, 0xc, y, LIGHT_BLUE, a.msg(port::text::UPDATE_CURRENT));
    right(out, a, ww - 0xc, y + step, LIGHT_BLUE, &p.view.current);
    y += 2 * step;
    print(out, Font::Regular, 0xc, y, LIGHT_BLUE, a.msg(port::text::UPDATE_NEWEST));
    right(out, a, ww - 0xc, y, LIGHT_BLUE, &p.view.newest);
    y += step;
    print(out, Font::Regular, 0xc, y, LIGHT_BLUE, &p.view.status);
    y += step;
    for (i, &id) in rows.iter().enumerate() {
        let col = if i as i32 == p.cursor { YELLOW } else { LIGHT_BLUE };
        print(out, Font::Regular, 0xc, y, col, a.msg(id));
        y += step;
    }
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_drawable_and_bounded() {
        assert_eq!(drawable("0.6.0-beta"), b"0.6.0-beta");
        assert_eq!(drawable("\u{2026}"), b"???");
        let d = drawable(&"x".repeat(50));
        assert_eq!((d.len(), &d[VALUE_MAX - 3..]), (VALUE_MAX, &b"..."[..]));
    }

    #[test]
    fn update_is_a_row_only_when_offered_and_idle() {
        let mut r = UpdateRows::default();
        assert_eq!(r.rows(), [port::text::BACK]);
        r.view.can_update = true;
        assert_eq!(r.rows(), [port::text::UPDATE_ACTION, port::text::BACK]);
        r.view.busy = true;
        assert_eq!(r.rows(), [port::text::BACK]);
    }
}
